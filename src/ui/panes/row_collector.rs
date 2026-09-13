use std::collections::HashMap;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::SPAWN_BUTTON;
use super::row;
use crate::group::RepoGroup;
use crate::state::{AppState, Focus, RepoFilter, StatusFilter};
use crate::ui::text::display_width;

#[derive(Debug, Default)]
pub(super) struct CollectedRows {
    pub lines: Vec<Line<'static>>,
    pub line_to_row: Vec<Option<usize>>,
    pub pending_spawn: Vec<(usize, String, String, Option<String>)>,
    pub pending_remove: Vec<(usize, u16, String)>,
    pub pending_session_jump: Vec<(usize, String)>,
}

/// One block of the agent list: a repo group, or a tmux session holding no
/// agents at all, which has no repos to render underneath it.
enum Block<'a> {
    Repo(&'a RepoGroup),
    EmptySession(&'a str),
}

/// Interleave the agent-less sessions into the repo-group sequence by
/// session name, so an empty session lands in its alphabetical slot instead
/// of a trailing clump. Both inputs arrive sorted case-insensitively by
/// session name, so one merge pass suffices. `empty_sessions` is empty in
/// repository grouping, where this degenerates to the group list.
fn merge_blocks<'a>(groups: &'a [RepoGroup], empty_sessions: &'a [String]) -> Vec<Block<'a>> {
    if empty_sessions.is_empty() {
        return groups.iter().map(Block::Repo).collect();
    }

    let mut blocks = Vec::with_capacity(groups.len() + empty_sessions.len());
    let mut empties = empty_sessions.iter().peekable();
    for group in groups {
        let group_key = group.session.as_deref().unwrap_or_default().to_lowercase();
        while let Some(name) = empties.peek() {
            if name.to_lowercase() >= group_key {
                break;
            }
            blocks.push(Block::EmptySession(name.as_str()));
            empties.next();
        }
        blocks.push(Block::Repo(group));
    }
    blocks.extend(empties.map(|name| Block::EmptySession(name.as_str())));
    blocks
}

/// A title at column 0 with the `+` spawn button right-aligned on the same
/// row. Shared by the repo title row and, when a session holds a single repo
/// and that row is dropped, by the `[session]` header that takes its place.
fn title_with_spawn_button(
    title: &str,
    title_color: Color,
    button_color: Color,
    width: usize,
) -> Vec<Span<'static>> {
    let pad_width = width
        .saturating_sub(display_width(title))
        .saturating_sub(SPAWN_BUTTON.len());
    vec![
        Span::styled(title.to_string(), Style::default().fg(title_color)),
        Span::raw(" ".repeat(pad_width)),
        Span::styled(SPAWN_BUTTON, Style::default().fg(button_color)),
    ]
}

/// How many repo groups each named session will actually render. Counted
/// with the same two predicates as the render loop's guards, so a filter
/// that hides a session's second repo still leaves a single-repo block.
/// Blank session names are skipped: they emit no `[session]` header, so
/// their repo title is the only label they have.
fn renderable_groups_per_session(state: &AppState, filter: StatusFilter) -> HashMap<&str, usize> {
    let repo_filter = state.effective_repo_filter();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for group in &state.repo_groups {
        let Some(session) = group.session.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if !repo_filter.matches_group(&group.name) {
            continue;
        }
        if !group
            .panes
            .iter()
            .any(|(pane, _)| filter.matches(&pane.status))
        {
            continue;
        }
        *counts.entry(session).or_insert(0) += 1;
    }
    counts
}

pub(super) fn collect(state: &AppState, width: u16) -> CollectedRows {
    let width = width as usize;
    let theme = &state.theme;

    let mut collected = CollectedRows::default();
    let filter = state.effective_status_filter();
    let mut first_group = true;
    let mut prev_session: Option<String> = None;
    let mut row_index: usize = 0;

    // Under an active filter the list answers a narrower question, and a
    // session header with nothing beneath it reads as a rendering bug rather
    // than as information, so agent-less sessions drop out entirely.
    let unfiltered =
        filter == StatusFilter::All && matches!(state.effective_repo_filter(), RepoFilter::All);
    let empty_sessions: &[String] = if unfiltered {
        &state.empty_sessions
    } else {
        &[]
    };
    let groups_per_session = renderable_groups_per_session(state, filter);

    for block in merge_blocks(&state.repo_groups, empty_sessions) {
        let group = match block {
            Block::Repo(group) => group,
            Block::EmptySession(session_name) => {
                if !first_group {
                    collected.lines.push(Line::from(""));
                    collected.line_to_row.push(None);
                }
                first_group = false;

                // Dimmed, so an idle session is distinguishable at a glance
                // from one whose agents merely scrolled out of view.
                collected
                    .pending_session_jump
                    .push((collected.lines.len(), session_name.to_string()));
                collected.lines.push(Line::from(Span::styled(
                    format!("[{session_name}]"),
                    Style::default().fg(theme.text_inactive),
                )));
                collected.line_to_row.push(None);
                prev_session = Some(session_name.to_string());
                continue;
            }
        };

        if !state.effective_repo_filter().matches_group(&group.name) {
            continue;
        }
        let filtered_panes: Vec<_> = group
            .panes
            .iter()
            .filter(|(pane, _)| filter.matches(&pane.status))
            .collect();
        if filtered_panes.is_empty() {
            continue;
        }

        if !first_group {
            // Separate repo groups, but do not add a leading blank before
            // the first repo so the list starts immediately below the header.
            collected.lines.push(Line::from(""));
            collected.line_to_row.push(None);
        }
        first_group = false;

        // Session header in `SortMode::Session`: one `[name]` line above the
        // first repo of each session block, after the blank separator so the
        // blank reads as belonging to the session break. `None` (repository
        // mode) and blank session names emit nothing — a bare `[]` line is
        // worse than no line.
        let session = group.session.as_deref().filter(|s| !s.is_empty());

        let group_has_focused_pane = state
            .focus_state
            .focused_pane_id
            .as_ref()
            .is_some_and(|fid| group.panes.iter().any(|(p, _)| p.pane_id == *fid));

        // Only groups with a resolved repo_root get a spawn button — panes
        // outside a git repo have no root to spawn into.
        let repo_root = group
            .panes
            .iter()
            .find_map(|(_, git)| git.repo_root.clone());

        // A session holding exactly one renderable repo drops its repo title
        // row, which would sit directly under the `[session]` header and add
        // a second label to a block that already has one. The `+` moves up
        // onto the header line so the spawn affordance survives.
        let single_repo_session =
            session.is_some_and(|name| groups_per_session.get(name) == Some(&1));

        let mut session_header_emitted = false;
        if let Some(session_name) = session
            && prev_session.as_deref() != Some(session_name)
        {
            let session_has_focused_pane =
                state
                    .focus_state
                    .focused_pane_id
                    .as_ref()
                    .is_some_and(|fid| {
                        state
                            .repo_groups
                            .iter()
                            .filter(|g| g.session.as_deref() == Some(session_name))
                            .any(|g| g.panes.iter().any(|(p, _)| p.pane_id == *fid))
                    });
            let session_color = if session_has_focused_pane {
                theme.accent
            } else {
                theme.text_active
            };
            let header = format!("[{session_name}]");
            let spans = match repo_root.as_ref().filter(|_| single_repo_session) {
                Some(root) => {
                    collected.pending_spawn.push((
                        collected.lines.len(),
                        group.name.clone(),
                        root.clone(),
                        group.session.clone(),
                    ));
                    title_with_spawn_button(&header, session_color, session_color, width)
                }
                None => vec![Span::styled(header, Style::default().fg(session_color))],
            };
            collected.lines.push(Line::from(spans));
            collected.line_to_row.push(None);
            session_header_emitted = true;
        }
        prev_session = session.map(|s| s.to_string());

        // Plain repo header at column 0, with a `+` spawn button
        // right-aligned on the same row. Suppressed only when the session
        // header above it actually rendered and took the button with it —
        // never leave a block with no label at all.
        if !(single_repo_session && session_header_emitted) {
            let title = &group.name;
            let title_color = if group_has_focused_pane {
                theme.accent
            } else {
                theme.text_active
            };
            let spans: Vec<Span<'static>> = if let Some(ref root) = repo_root {
                collected.pending_spawn.push((
                    collected.lines.len(),
                    group.name.clone(),
                    root.clone(),
                    group.session.clone(),
                ));
                let button_color = if group_has_focused_pane {
                    theme.accent
                } else {
                    theme.text_active
                };
                title_with_spawn_button(title, title_color, button_color, width)
            } else {
                vec![Span::styled(
                    title.clone(),
                    Style::default().fg(title_color),
                )]
            };
            collected.lines.push(Line::from(spans));
            collected.line_to_row.push(None);
        }

        for (pane, git_info) in filtered_panes.iter() {
            let is_selected = state.focus_state.sidebar_focused
                && state.focus_state.focus == Focus::Panes
                && row_index == state.global.selected_pane_row;

            let is_active = state.focus_state.focused_pane_id.as_ref() == Some(&pane.pane_id);
            let is_same_window = state.sidebar_window_id.as_deref().is_some_and(|w| {
                !w.is_empty() && !pane.window_id.is_empty() && w == pane.window_id
            });

            let pane_state = state.pane_state(&pane.pane_id);
            let ports = pane_state.map(|s| s.ports.as_slice());
            let task_progress = pane_state.and_then(|s| s.task_progress.as_ref());
            let status_line_idx = collected.lines.len();
            let pane_lines = row::render_pane_lines_with_options(
                pane,
                git_info,
                ports,
                task_progress,
                is_selected,
                is_active,
                is_same_window,
                width,
                &state.icons,
                theme,
                state.spinner_frame,
                state.now,
                state.show_session_names,
                state.show_worktree_marker,
                state.compact_rows,
            );
            let pane_line_count = pane_lines.len();
            collected.lines.extend(pane_lines);
            for _ in 0..pane_line_count {
                collected.line_to_row.push(Some(row_index));
            }

            // The branch row is always `status_line_idx + 1` when
            // `branch_ports_row` emits a line (which requires a
            // non-empty branch). Look up the exact column of the
            // trailing `×` from the row helper so the click target
            // lines up with the rendered glyph even when the branch
            // name truncates.
            // Compact rows draw no `×`, so registering a target would put a
            // destructive click behind an invisible affordance. Removal is
            // still reachable there via the `x` keybinding.
            if !state.compact_rows
                && pane.sidebar_spawned
                && git_info.is_worktree
                && pane_line_count >= 2
                && let Some(x) =
                    row::sidebar_remove_marker_col(git_info, ports, true, width.saturating_sub(2))
            {
                collected
                    .pending_remove
                    .push((status_line_idx + 1, x, pane.pane_id.clone()));
            }

            row_index += 1;
        }
    }

    collected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{PaneGitInfo, RepoGroup};
    use crate::state::{AppState, StatusFilter};
    use crate::tmux::{AgentType, PaneInfo, PaneStatus, PermissionMode, WorktreeMetadata};

    fn make_pane(id: &str, status: PaneStatus) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: false,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/tmp/repo".into(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: WorktreeMetadata::default(),
            session_id: None,
            session_name: String::new(),
            tmux_session: String::new(),
            window_id: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }

    #[test]
    fn collect_empty_repo_groups_produces_no_lines() {
        let state = AppState::new("%0".into());
        let collected = collect(&state, 40);
        assert!(collected.lines.is_empty());
        assert!(collected.line_to_row.is_empty());
        assert!(collected.pending_spawn.is_empty());
        assert!(collected.pending_remove.is_empty());
    }

    #[test]
    fn collect_skips_group_when_status_filter_excludes_all_panes() {
        let mut state = AppState::new("%0".into());
        // The group has only Running panes, so filter to Waiting to drop them all.
        state.global.status_filter = StatusFilter::Waiting;
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            session: None,
            panes: vec![(make_pane("%1", PaneStatus::Running), PaneGitInfo::default())],
        }];
        let collected = collect(&state, 40);
        assert!(collected.lines.is_empty());
        assert!(collected.pending_spawn.is_empty());
    }

    #[test]
    fn collect_records_pending_spawn_when_repo_root_present() {
        let mut state = AppState::new("%0".into());
        let git_info = PaneGitInfo {
            repo_root: Some("/tmp/repo".into()),
            branch: None,
            is_worktree: false,
            worktree_name: None,
        };
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            session: None,
            panes: vec![(make_pane("%1", PaneStatus::Running), git_info)],
        }];
        let collected = collect(&state, 40);
        assert_eq!(
            collected.pending_spawn.len(),
            1,
            "groups with a repo_root should emit a spawn target"
        );
        assert_eq!(collected.pending_spawn[0].1, "repo");
        assert_eq!(collected.pending_spawn[0].2, "/tmp/repo");
        // At least the header plus one pane row should have been pushed.
        assert!(!collected.lines.is_empty());
    }

    #[test]
    fn collect_no_pending_spawn_without_repo_root() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![RepoGroup {
            name: "raw-path".into(),
            has_focus: false,
            session: None,
            panes: vec![(make_pane("%1", PaneStatus::Running), PaneGitInfo::default())],
        }];
        let collected = collect(&state, 40);
        assert!(
            collected.pending_spawn.is_empty(),
            "groups without repo_root must not produce spawn targets"
        );
    }

    #[test]
    fn collect_does_not_mark_empty_window_ids_as_same_window() {
        let mut state = AppState::new("%0".into());
        state.focus_state.focused_pane_id = Some("%other".into());
        state.sidebar_window_id = Some(String::new());
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            session: None,
            panes: vec![(make_pane("%1", PaneStatus::Running), PaneGitInfo::default())],
        }];

        let collected = collect(&state, 40);
        let marker_span = &collected.lines[1].spans[0];

        assert_eq!(marker_span.content, " ");
        assert_eq!(marker_span.style.fg, None);
    }

    #[test]
    fn collect_pending_spawn_grows_with_repo_root_bearing_groups() {
        let mut state = AppState::new("%0".into());
        let with_root = |root: &str, name: &str, pane_id: &str| RepoGroup {
            name: name.into(),
            has_focus: false,
            session: None,
            panes: vec![(
                make_pane(pane_id, PaneStatus::Running),
                PaneGitInfo {
                    repo_root: Some(root.into()),
                    branch: None,
                    is_worktree: false,
                    worktree_name: None,
                },
            )],
        };
        state.repo_groups = vec![
            with_root("/repo/a", "a", "%1"),
            with_root("/repo/b", "b", "%2"),
            with_root("/repo/c", "c", "%3"),
        ];
        let collected = collect(&state, 40);
        assert_eq!(collected.pending_spawn.len(), 3);
    }

    /// A sidebar-spawned worktree pane, which is the only shape that earns
    /// a trailing `×` remove marker.
    fn state_with_spawned_worktree() -> AppState {
        let mut state = AppState::new("%0".into());
        let mut pane = make_pane("%1", PaneStatus::Running);
        pane.sidebar_spawned = true;
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            session: None,
            panes: vec![(
                pane,
                PaneGitInfo {
                    repo_root: Some("/tmp/repo".into()),
                    branch: Some("feat/thing".into()),
                    is_worktree: true,
                    worktree_name: None,
                },
            )],
        }];
        state
    }

    #[test]
    fn collect_registers_remove_target_when_expanded() {
        let state = state_with_spawned_worktree();
        let collected = collect(&state, 40);
        assert_eq!(
            collected.pending_remove.len(),
            1,
            "expanded rows draw the × marker and must stay clickable"
        );
    }

    #[test]
    fn collect_skips_remove_target_when_compact() {
        let mut state = state_with_spawned_worktree();
        state.compact_rows = true;
        let collected = collect(&state, 40);
        assert!(
            collected.pending_remove.is_empty(),
            "compact rows draw no × marker, so no click target may be registered"
        );
    }

    /// Flatten collected lines to plain strings for structural assertions.
    /// These are `collect()` outputs, not a rendered frame, so no snapshot
    /// is required; `tests/ui_snapshot.rs` covers the rendered form.
    fn line_texts(collected: &CollectedRows) -> Vec<String> {
        collected
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn group_in_session(name: &str, session: Option<&str>, pane_id: &str) -> RepoGroup {
        RepoGroup {
            name: name.into(),
            session: session.map(|s| s.to_string()),
            has_focus: false,
            panes: vec![(
                make_pane(pane_id, PaneStatus::Running),
                PaneGitInfo::default(),
            )],
        }
    }

    #[test]
    fn collect_emits_one_session_header_per_session_block() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![
            group_in_session("repo-a", Some("personal"), "%1"),
            group_in_session("repo-a", Some("work"), "%2"),
            group_in_session("repo-b", Some("work"), "%3"),
        ];
        let texts = line_texts(&collect(&state, 40));

        assert_eq!(
            texts.iter().filter(|t| t.as_str() == "[personal]").count(),
            1
        );
        assert_eq!(texts.iter().filter(|t| t.as_str() == "[work]").count(), 1);
        let personal = texts.iter().position(|t| t == "[personal]").unwrap();
        let work = texts.iter().position(|t| t == "[work]").unwrap();
        assert!(personal < work, "session headers follow group order");
        assert_eq!(
            texts[work + 1],
            "repo-a",
            "the repo header follows its session header immediately"
        );
        assert_eq!(
            texts[work - 1],
            "",
            "the blank separator sits above the session header"
        );
    }

    /// Session grouping with `@sidebar_show_empty_sessions` already applied,
    /// i.e. `empty_sessions` populated the way `apply_session_snapshot` would.
    fn state_with_empty_sessions(groups: Vec<RepoGroup>, empty: &[&str]) -> AppState {
        let mut state = AppState::new("%0".into());
        state.repo_groups = groups;
        state.show_empty_sessions = true;
        state.empty_sessions = empty.iter().map(|s| s.to_string()).collect();
        state
    }

    #[test]
    fn collect_interleaves_empty_sessions_by_name() {
        let state = state_with_empty_sessions(
            vec![
                group_in_session("repo-a", Some("beta"), "%1"),
                group_in_session("repo-b", Some("delta"), "%2"),
            ],
            &["alpha", "charlie", "echo"],
        );
        let texts = line_texts(&collect(&state, 40));

        let headers: Vec<&String> = texts.iter().filter(|t| t.starts_with('[')).collect();
        assert_eq!(
            headers,
            vec!["[alpha]", "[beta]", "[charlie]", "[delta]", "[echo]"],
            "empty sessions sort into place, not into a trailing clump"
        );
    }

    #[test]
    fn collect_emits_nothing_under_an_empty_session_header() {
        let state = state_with_empty_sessions(
            vec![group_in_session("repo-a", Some("work"), "%1")],
            &["idle"],
        );
        let texts = line_texts(&collect(&state, 40));

        let idle = texts.iter().position(|t| t == "[idle]").unwrap();
        assert_eq!(
            texts[idle + 1],
            "",
            "the next line is the separator before the [work] block"
        );
        assert_eq!(texts[idle + 2], "[work]");
    }

    #[test]
    fn collect_registers_a_jump_target_per_empty_session_header() {
        let state = state_with_empty_sessions(
            vec![group_in_session("repo-a", Some("work"), "%1")],
            &["idle"],
        );
        let collected = collect(&state, 40);
        let texts = line_texts(&collected);

        let sessions: Vec<&str> = collected
            .pending_session_jump
            .iter()
            .map(|(_, name)| name.as_str())
            .collect();
        assert_eq!(sessions, vec!["idle"], "only agent-less headers jump");
        let (line_idx, _) = &collected.pending_session_jump[0];
        assert_eq!(texts[*line_idx], "[idle]");
    }

    #[test]
    fn collect_hides_empty_sessions_under_a_status_filter() {
        let mut state = state_with_empty_sessions(
            vec![group_in_session("repo-a", Some("work"), "%1")],
            &["idle"],
        );
        state.global.status_filter = StatusFilter::Running;
        let texts = line_texts(&collect(&state, 40));

        assert!(
            !texts.iter().any(|t| t == "[idle]"),
            "a header with nothing beneath it reads as a bug under a filter: {texts:?}"
        );
        assert!(collect(&state, 40).pending_session_jump.is_empty());
    }

    #[test]
    fn collect_hides_empty_sessions_under_a_repo_filter() {
        let mut state = state_with_empty_sessions(
            vec![group_in_session("repo-a", Some("work"), "%1")],
            &["idle"],
        );
        state.global.repo_filter = RepoFilter::Repo("repo-a".into());
        let texts = line_texts(&collect(&state, 40));

        assert!(
            !texts.iter().any(|t| t == "[idle]"),
            "repo-filtered lists answer a narrower question: {texts:?}"
        );
    }

    #[test]
    fn collect_emits_no_leading_separator_when_an_empty_session_is_first() {
        let state = state_with_empty_sessions(
            vec![group_in_session("repo-a", Some("work"), "%1")],
            &["alpha"],
        );
        let texts = line_texts(&collect(&state, 40));

        assert_eq!(
            texts[0], "[alpha]",
            "the list still starts immediately below the header: {texts:?}"
        );
    }

    /// A group whose panes resolve to a repo root, so the `+` spawn button
    /// and its `pending_spawn` entry are rendered.
    fn group_with_root(name: &str, session: Option<&str>, pane_id: &str) -> RepoGroup {
        let mut group = group_in_session(name, session, pane_id);
        group.panes[0].1.repo_root = Some(format!("/repos/{name}"));
        group
    }

    #[test]
    fn collect_hides_the_repo_title_for_a_single_repo_session() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![group_in_session("repo-a", Some("work"), "%1")];
        let texts = line_texts(&collect(&state, 40));

        assert_eq!(texts[0], "[work]");
        assert!(
            !texts.iter().any(|t| t == "repo-a"),
            "the session header already labels the block: {texts:?}"
        );
    }

    #[test]
    fn collect_keeps_repo_titles_when_a_session_holds_two_repos() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![
            group_in_session("repo-a", Some("work"), "%1"),
            group_in_session("repo-b", Some("work"), "%2"),
        ];
        let texts = line_texts(&collect(&state, 40));

        assert!(texts.iter().any(|t| t == "repo-a"));
        assert!(texts.iter().any(|t| t == "repo-b"));
    }

    #[test]
    fn collect_hides_the_repo_title_when_a_filter_leaves_one_repo() {
        // Two repos, but the status filter drops every pane of the second,
        // so the block renders as a single-repo session and the title goes
        // with it.
        let mut state = AppState::new("%0".into());
        let mut idle_group = group_in_session("repo-b", Some("work"), "%2");
        idle_group.panes[0].0.status = PaneStatus::Idle;
        state.repo_groups = vec![group_in_session("repo-a", Some("work"), "%1"), idle_group];
        state.global.status_filter = StatusFilter::Running;
        let texts = line_texts(&collect(&state, 40));

        assert!(
            !texts.iter().any(|t| t == "repo-a"),
            "only one repo still renders, so its title is redundant: {texts:?}"
        );
    }

    #[test]
    fn collect_keeps_the_repo_title_for_a_blank_session_name() {
        // A blank session emits no `[session]` header, so its repo title is
        // the only label the block has.
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![group_in_session("repo-a", Some(""), "%1")];
        let texts = line_texts(&collect(&state, 40));

        assert_eq!(texts[0], "repo-a");
    }

    #[test]
    fn collect_keeps_the_repo_title_in_repository_mode() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![group_in_session("repo-a", None, "%1")];
        let texts = line_texts(&collect(&state, 40));

        assert_eq!(texts[0], "repo-a");
    }

    #[test]
    fn collect_moves_the_spawn_button_onto_a_single_repo_session_header() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![group_with_root("repo-a", Some("work"), "%1")];
        let collected = collect(&state, 40);
        let texts = line_texts(&collected);

        assert_eq!(collected.pending_spawn.len(), 1);
        let (line_idx, name, root, session) = &collected.pending_spawn[0];
        assert_eq!(
            texts[*line_idx].trim_end(),
            "[work]                                 +"
        );
        assert_eq!(
            name, "repo-a",
            "the target still names the repo, not the session"
        );
        assert_eq!(root, "/repos/repo-a");
        assert_eq!(session.as_deref(), Some("work"));
    }

    #[test]
    fn collect_leaves_the_spawn_button_on_the_repo_title_for_a_multi_repo_session() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![
            group_with_root("repo-a", Some("work"), "%1"),
            group_with_root("repo-b", Some("work"), "%2"),
        ];
        let collected = collect(&state, 40);
        let texts = line_texts(&collected);

        let lines: Vec<&str> = collected
            .pending_spawn
            .iter()
            .map(|(idx, _, _, _)| texts[*idx].trim_end())
            .collect();
        assert_eq!(
            lines,
            vec![
                "repo-a                                 +",
                "repo-b                                 +"
            ]
        );
    }

    #[test]
    fn collect_emits_no_session_header_in_repository_mode() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![
            group_in_session("repo-a", None, "%1"),
            group_in_session("repo-b", None, "%2"),
        ];
        let texts = line_texts(&collect(&state, 40));

        assert!(
            !texts.iter().any(|t| t.starts_with('[')),
            "groups with no session must render exactly as they do today: {texts:?}"
        );
    }

    #[test]
    fn collect_emits_no_session_header_for_a_blank_session_name() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![group_in_session("repo-a", Some(""), "%1")];
        let texts = line_texts(&collect(&state, 40));

        assert!(
            !texts.iter().any(|t| t == "[]"),
            "a bare [] line is worse than no line: {texts:?}"
        );
    }

    #[test]
    fn collect_maps_session_header_lines_to_no_row() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![group_in_session("repo-a", Some("work"), "%1")];
        let collected = collect(&state, 40);
        let idx = line_texts(&collected)
            .iter()
            .position(|t| t == "[work]")
            .expect("session header rendered");

        assert_eq!(
            collected.line_to_row[idx], None,
            "j/k must skip the session header and it must not be selectable"
        );
    }
}
