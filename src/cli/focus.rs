use crate::desktop_notification;
use crate::group;
use crate::state::{RepoFilter, StatusFilter};
use crate::tmux::SessionInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Next,
    Prev,
}

/// What the user asked `focus` to jump to. `Cycle` is the original
/// next/prev walk over the eligible pane list; `Notification` jumps
/// straight to the most recently notified pane.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Cycle(Direction),
    Notification,
    Index(u32),
    PaneId(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    Session,
}

/// Pick the pane to jump to, or `None` when the cursor is already on the only
/// candidate (or there are no candidates at all).
///
/// `active_session` is the session holding `active_pane_id`, resolved through
/// tmux rather than by searching `sessions` — the cursor is often on a pane
/// that isn't an agent pane at all, and those never appear in `sessions`.
fn select_target_pane(
    sessions: &[SessionInfo],
    active_pane_id: &str,
    active_session: Option<&str>,
    direction: Direction,
    scope: Scope,
) -> Option<String> {
    let pane_ids = eligible_pane_ids(sessions, active_session, scope);
    if pane_ids.is_empty() {
        return None;
    }

    let current = pane_ids
        .iter()
        .position(|pane_id| pane_id == active_pane_id);
    let target_index = match (direction, current) {
        (Direction::Next, Some(index)) => (index + 1) % pane_ids.len(),
        (Direction::Prev, Some(0)) => pane_ids.len() - 1,
        (Direction::Prev, Some(index)) => index - 1,
        // The cursor is on a pane outside the candidate list (a shell, the
        // sidebar, an editor). Enter the list from whichever end the
        // direction implies instead of treating it as "nowhere to go".
        (Direction::Next, None) => 0,
        (Direction::Prev, None) => pane_ids.len() - 1,
    };

    // With a single candidate every direction lands back on the cursor when
    // the cursor is already that pane — that is the genuine no-op the caller
    // reports on.
    pane_ids
        .get(target_index)
        .filter(|target| *target != active_pane_id)
        .cloned()
}

/// Every agent pane eligible for cycling, in the same repo-group order the
/// sidebar renders.
///
/// No status filter is applied: `query_sessions` already drops panes without
/// an `@pane_agent` marker (and the sidebar's own pane), so everything left
/// here is an agent pane. Cycling covers idle and waiting agents too — those
/// are precisely the ones a user wants to jump to.
///
/// A `Session` scope with no resolvable `active_session` yields nothing. The
/// alternative — falling back to every session — would silently turn
/// `--scope session` into `--scope all`.
fn eligible_pane_ids(
    sessions: &[SessionInfo],
    active_session: Option<&str>,
    scope: Scope,
) -> Vec<String> {
    let scoped_session = match scope {
        Scope::All => None,
        Scope::Session => match active_session {
            Some(name) => Some(name),
            None => return Vec::new(),
        },
    };

    let scoped_sessions: Vec<SessionInfo> = sessions
        .iter()
        .filter(|session| scoped_session.is_none_or(|name| session.session_name == name))
        .cloned()
        .collect();

    group::group_panes(&scoped_sessions, group::SortMode::Repository)
        .iter()
        .flat_map(|group| group.panes.iter())
        .map(|(pane, _)| pane.pane_id.clone())
        .collect()
}

/// Parse one stamp query's output into `(pane_id, timestamp)` pairs.
///
/// Each line is `pane_id|<stamp value>` for a single stamp key, and a stamp
/// value is itself `timestamp|fingerprint`. Splitting at the *first* `|`
/// is therefore exact: a pane id never contains `|`, and
/// `normalize_fingerprint` strips `|` from fingerprints, so the remainder
/// is one whole stamp value with its own separator intact.
///
/// Lines without a pane id, and panes whose option is unset or corrupt,
/// are omitted rather than reported with a zero timestamp: a pane that has
/// never notified must never win the comparison in
/// [`select_last_notified_pane`].
fn parse_stamp_lines(raw: &str) -> Vec<(String, u64)> {
    raw.lines()
        .filter_map(|line| {
            let (pane_id, stamp) = line.split_once('|')?;
            if pane_id.is_empty() {
                return None;
            }
            Some((
                pane_id.to_string(),
                desktop_notification::stamp_timestamp(stamp)?,
            ))
        })
        .collect()
}

/// The candidate pane whose newest notification stamp is the most recent,
/// or `None` when no candidate has ever notified.
///
/// `stamps` is the concatenation of one query per stamp key, so a pane may
/// appear several times — once per notification kind it has fired. A
/// pane's recency is the maximum over its own entries.
///
/// Unlike [`select_target_pane`], the active pane is deliberately *not*
/// filtered out. Whether the newest notification came from the pane the
/// user is already on is a meaningful distinction the caller reports on
/// rather than something to hide.
///
/// Ties resolve to the earlier pane in `eligible`. The strict `>` is what
/// enforces that: `max_by_key` would keep the *last* equal maximum instead.
fn select_last_notified_pane(eligible: &[String], stamps: &[(String, u64)]) -> Option<String> {
    let mut best: Option<(&String, u64)> = None;
    for pane_id in eligible {
        let Some(timestamp) = stamps
            .iter()
            .filter(|(id, _)| id == pane_id)
            .map(|(_, timestamp)| *timestamp)
            .max()
        else {
            continue;
        };
        if best.is_none_or(|(_, best_timestamp)| timestamp > best_timestamp) {
            best = Some((pane_id, timestamp));
        }
    }
    best.map(|(pane_id, _)| pane_id.clone())
}

/// The `list-panes -F` format for one stamp key: the pane id paired with
/// that key's value.
///
/// One key per query, because a stamp value is itself
/// `timestamp|fingerprint` — see [`parse_stamp_lines`] for why a line
/// carrying several stamps cannot be split back apart.
///
/// These keys are deliberately absent from `tmux::query_sessions`' shared
/// `pane_format()`: its 28 fields are kept in lock-step with
/// hand-maintained index constants and the TUI has no use for notify
/// stamps, so `focus notification` pays for its own queries instead of
/// imposing maintenance cost on every consumer.
///
/// Deliberately unquoted — no `#{q:...}`. That modifier shell-escapes both
/// `|` and `%`, which would turn a real line into
/// `\%34|1785210882\|1785210318192:stop`: the escaped `|` breaks
/// `parse_stamp_lines`' first-`|` split (the timestamp fails to parse) and
/// the escaped `%` in the pane id would never match an id from
/// `query_sessions` anyway. `src/tmux/query.rs` can use `#{q:...}` safely
/// only because it unescapes the result afterwards via `split_tmux_fields`;
/// this path does not, so omitting the quoting here is required, not an
/// oversight. It stays safe without quoting because there are exactly two
/// fields split at the *first* `|`, a pane id never contains `|`, and
/// `normalize_fingerprint` (in `desktop_notification.rs`) already replaces
/// `|`, `\n`, and `\r` in fingerprints with spaces.
fn stamp_format(key: &str) -> String {
    format!("#{{pane_id}}|#{{{key}}}")
}

/// Every pane's notification stamps, one query per stamp key. A pane that
/// has fired several kinds of notification contributes one entry per kind;
/// [`select_last_notified_pane`] reduces those to the newest. A failed
/// query yields no entries, which reads the same as "nothing notified".
fn notification_stamps() -> Vec<(String, u64)> {
    desktop_notification::stamp_option_keys()
        .iter()
        .flat_map(|key| {
            let format = stamp_format(key);
            let raw =
                crate::tmux::run_tmux(&["list-panes", "-a", "-F", &format]).unwrap_or_default();
            parse_stamp_lines(&raw)
        })
        .collect()
}

/// Jump to the pane whose notification fired most recently, reporting on
/// the tmux status line when there is nowhere to go.
fn focus_last_notification(
    sessions: &[SessionInfo],
    active_pane_id: &str,
    active_session: Option<&str>,
    scope: Scope,
) -> i32 {
    let eligible = eligible_pane_ids(sessions, active_session, scope);
    let stamps = notification_stamps();

    match select_last_notified_pane(&eligible, &stamps) {
        None => crate::tmux::show_message(no_notification_message(scope)),
        Some(pane_id) if pane_id == active_pane_id => {
            crate::tmux::show_message(ALREADY_ON_NOTIFIED_PANE)
        }
        Some(pane_id) => crate::tmux::select_pane(&pane_id),
    }
    0
}

/// Status-line text shown when there is nowhere to jump. Without it the
/// command is a silent no-op, indistinguishable from a broken binary.
fn no_target_message(scope: Scope) -> &'static str {
    match scope {
        Scope::All => "agent-sidebar: no other agent pane to focus",
        Scope::Session => "agent-sidebar: no other agent pane in this session",
    }
}

/// Status-line text when no candidate pane has ever fired a notification.
/// Mirrors [`no_target_message`]: a silent no-op is indistinguishable from
/// a broken binary.
fn no_notification_message(scope: Scope) -> &'static str {
    match scope {
        Scope::All => "agent-sidebar: no recent agent notification to focus",
        Scope::Session => "agent-sidebar: no recent agent notification in this session",
    }
}

/// Status-line text when the most recent notification came from the pane
/// the cursor is already on. Distinct from [`no_notification_message`] so
/// the user can tell "nothing has notified" from "you are already there".
const ALREADY_ON_NOTIFIED_PANE: &str = "agent-sidebar: already on the last notified pane";

fn select_pane_by_index(visible: &[String], index: u32) -> Option<String> {
    visible.get(index as usize - 1).cloned()
}

fn parse_index(value: &str) -> Option<u32> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u32 = value.parse().ok()?;
    (n >= 1).then_some(n)
}

pub(crate) fn load_sidebar_filters() -> (StatusFilter, RepoFilter) {
    let opts = crate::tmux::get_all_global_options();
    let status = if crate::ui::hide_filter_bar_from_options(&opts) {
        StatusFilter::All
    } else {
        opts.get(crate::tmux::SIDEBAR_FILTER)
            .map(|s| StatusFilter::from_label(s))
            .unwrap_or(StatusFilter::All)
    };
    let repo = if crate::ui::hide_repo_filter_from_options(&opts) {
        RepoFilter::All
    } else {
        opts.get(crate::tmux::SIDEBAR_REPO_FILTER)
            .map(|s| RepoFilter::from_label(s))
            .unwrap_or(RepoFilter::All)
    };
    (status, repo)
}

fn no_visible_message() -> &'static str {
    "agent-sidebar: no visible agent panes"
}

fn no_index_message(index: u32) -> String {
    format!("agent-sidebar: no agent at position {index}")
}

fn focus_by_index(sessions: &[SessionInfo], index: u32) -> i32 {
    let groups = group::group_panes(sessions, group::SortMode::Repository);
    let (status_filter, repo_filter) = load_sidebar_filters();
    let visible = group::visible_pane_ids(&groups, status_filter, &repo_filter);

    match select_pane_by_index(&visible, index) {
        None if visible.is_empty() => crate::tmux::show_message(no_visible_message()),
        None => crate::tmux::show_message(&no_index_message(index)),
        Some(pane_id) => {
            crate::tmux::select_pane(&pane_id);
        }
    }
    0
}

fn usage() {
    eprintln!(
        "usage: tmux-agent-sidebar focus <next|prev|notification|<N>|%pane_id> [--scope <all|session>]"
    );
}

fn parse_target(value: &str) -> Option<Target> {
    if value.starts_with('%') && value.len() > 1 {
        return Some(Target::PaneId(value.to_string()));
    }
    if let Some(index) = parse_index(value) {
        return Some(Target::Index(index));
    }
    match value {
        "next" => Some(Target::Cycle(Direction::Next)),
        "prev" | "previous" => Some(Target::Cycle(Direction::Prev)),
        "notification" => Some(Target::Notification),
        _ => None,
    }
}

fn parse_args(args: &[String]) -> Result<(Target, Scope), ()> {
    let target = args
        .first()
        .and_then(|value| parse_target(value))
        .ok_or(())?;
    let mut scope = Scope::All;
    let mut index = 1;

    while index < args.len() {
        match args[index].as_str() {
            "--scope" => {
                if matches!(target, Target::Index(_) | Target::PaneId(_)) {
                    return Err(());
                }
                let value = args.get(index + 1).ok_or(())?;
                scope = match value.as_str() {
                    "all" => Scope::All,
                    "session" => Scope::Session,
                    _ => return Err(()),
                };
                index += 2;
            }
            _ => return Err(()),
        }
    }

    Ok((target, scope))
}

fn active_pane_id() -> Option<String> {
    crate::tmux::run_tmux(&["display-message", "-p", "#{pane_id}"])
        .map(|pane_id| pane_id.trim().to_string())
        .filter(|pane_id| !pane_id.is_empty())
}

pub fn cmd_focus(args: &[String]) -> i32 {
    let (target, scope) = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(()) => {
            usage();
            return 1;
        }
    };

    let sessions = crate::tmux::query_sessions();
    let Some(active_pane_id) = active_pane_id() else {
        eprintln!("tmux-agent-sidebar focus: not running inside tmux");
        return 1;
    };
    let active_session = crate::tmux::pane_session_name(&active_pane_id);

    match target {
        Target::Notification => {
            focus_last_notification(&sessions, &active_pane_id, active_session.as_deref(), scope)
        }
        Target::Index(index) => focus_by_index(&sessions, index),
        Target::PaneId(pane_id) => {
            crate::tmux::select_pane(&pane_id);
            0
        }
        Target::Cycle(direction) => {
            let Some(target_pane_id) = select_target_pane(
                &sessions,
                &active_pane_id,
                active_session.as_deref(),
                direction,
                scope,
            ) else {
                crate::tmux::show_message(no_target_message(scope));
                return 0;
            };

            crate::tmux::select_pane(&target_pane_id);
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{
        AgentType, PaneInfo, PaneStatus, PermissionMode, WindowInfo, WorktreeMetadata,
    };

    fn pane(id: &str, active: bool, status: PaneStatus, session_name: &str) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: active,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/repo".into(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: Vec::new(),
            pane_pid: None,
            worktree: WorktreeMetadata::default(),
            session_id: None,
            session_name: session_name.into(),
            tmux_session: String::new(),
            window_id: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }

    fn session(name: &str, panes: Vec<PaneInfo>) -> SessionInfo {
        SessionInfo {
            session_name: name.into(),
            windows: vec![WindowInfo {
                window_id: format!("@{name}"),
                window_name: name.into(),
                window_active: true,
                auto_rename: false,
                panes,
            }],
        }
    }

    fn pane_at_path(id: &str, path: &str, session_name: &str) -> PaneInfo {
        let mut pane = pane(id, false, PaneStatus::Running, session_name);
        pane.path = path.into();
        pane
    }

    #[test]
    fn next_follows_sidebar_repo_order_not_tmux_enumeration_order() {
        let sessions = vec![session(
            "one",
            vec![
                pane_at_path("%1", "/tmp/m-repo", "one"),
                pane_at_path("%2", "/tmp/a-repo", "one"),
                pane_at_path("%3", "/tmp/z-repo", "one"),
            ],
        )];

        assert_eq!(
            select_target_pane(&sessions, "%1", Some("one"), Direction::Next, Scope::All),
            Some("%3".into())
        );
    }

    #[test]
    fn next_all_selects_next_agent_pane_regardless_of_status() {
        let sessions = vec![session(
            "one",
            vec![
                pane("%1", true, PaneStatus::Running, "one"),
                pane("%2", false, PaneStatus::Idle, "one"),
                pane("%3", false, PaneStatus::Running, "one"),
            ],
        )];

        assert_eq!(
            select_target_pane(&sessions, "%1", Some("one"), Direction::Next, Scope::All),
            Some("%2".into())
        );
    }

    #[test]
    fn prev_all_wraps_to_last_running_pane() {
        let sessions = vec![
            session("one", vec![pane("%1", true, PaneStatus::Running, "one")]),
            session("two", vec![pane("%2", false, PaneStatus::Running, "two")]),
        ];

        assert_eq!(
            select_target_pane(&sessions, "%1", Some("one"), Direction::Prev, Scope::All),
            Some("%2".into())
        );
    }

    #[test]
    fn next_session_stays_in_active_pane_session() {
        let sessions = vec![
            session("one", vec![pane("%1", true, PaneStatus::Running, "one")]),
            session(
                "two",
                vec![
                    pane("%2", false, PaneStatus::Running, "two"),
                    pane("%3", false, PaneStatus::Running, "two"),
                ],
            ),
        ];

        assert_eq!(
            select_target_pane(
                &sessions,
                "%2",
                Some("two"),
                Direction::Next,
                Scope::Session
            ),
            Some("%3".into())
        );
    }

    #[test]
    fn session_scope_uses_session_containing_active_pane_even_when_active_is_not_running() {
        let sessions = vec![session(
            "one",
            vec![
                pane("%1", true, PaneStatus::Idle, "one"),
                pane("%2", false, PaneStatus::Running, "one"),
                pane("%3", false, PaneStatus::Running, "one"),
            ],
        )];

        assert_eq!(
            select_target_pane(
                &sessions,
                "%1",
                Some("one"),
                Direction::Next,
                Scope::Session
            ),
            Some("%2".into())
        );
    }

    #[test]
    fn lone_eligible_pane_is_focused_from_a_non_agent_pane() {
        let sessions = vec![session(
            "one",
            vec![pane("%1", false, PaneStatus::Idle, "one")],
        )];

        for direction in [Direction::Next, Direction::Prev] {
            assert_eq!(
                select_target_pane(&sessions, "%9", Some("one"), direction, Scope::All),
                Some("%1".into()),
                "{direction:?} should jump to the only agent pane"
            );
        }
    }

    #[test]
    fn session_scope_from_a_non_agent_pane_stays_in_that_session() {
        let sessions = vec![
            session("one", vec![pane("%1", false, PaneStatus::Idle, "one")]),
            session("two", vec![pane("%2", false, PaneStatus::Idle, "two")]),
        ];

        assert_eq!(
            select_target_pane(
                &sessions,
                "%9",
                Some("two"),
                Direction::Next,
                Scope::Session
            ),
            Some("%2".into())
        );
    }

    #[test]
    fn session_scope_without_a_resolvable_session_finds_nothing() {
        let sessions = vec![session(
            "one",
            vec![pane("%1", false, PaneStatus::Idle, "one")],
        )];

        assert_eq!(
            select_target_pane(&sessions, "%9", None, Direction::Next, Scope::Session),
            None
        );
    }

    #[test]
    fn idle_agent_panes_are_eligible() {
        let sessions = vec![session(
            "one",
            vec![
                pane("%1", true, PaneStatus::Running, "one"),
                pane("%2", false, PaneStatus::Idle, "one"),
            ],
        )];

        assert_eq!(
            select_target_pane(&sessions, "%1", Some("one"), Direction::Next, Scope::All),
            Some("%2".into())
        );
    }

    #[test]
    fn every_agent_pane_status_is_eligible() {
        let sessions = vec![session(
            "one",
            vec![
                pane("%1", true, PaneStatus::Running, "one"),
                pane("%2", false, PaneStatus::Waiting, "one"),
                pane("%3", false, PaneStatus::Background, "one"),
                pane("%4", false, PaneStatus::Error, "one"),
                pane("%5", false, PaneStatus::Unknown, "one"),
            ],
        )];

        assert_eq!(
            eligible_pane_ids(&sessions, Some("one"), Scope::All),
            vec!["%1", "%2", "%3", "%4", "%5"]
        );
    }

    #[test]
    fn no_target_message_names_the_scope() {
        assert_eq!(
            no_target_message(Scope::All),
            "agent-sidebar: no other agent pane to focus"
        );
        assert_eq!(
            no_target_message(Scope::Session),
            "agent-sidebar: no other agent pane in this session"
        );
    }

    #[test]
    fn single_eligible_agent_pane_returns_none() {
        let sessions = vec![session(
            "one",
            vec![pane("%1", true, PaneStatus::Running, "one")],
        )];

        assert_eq!(
            select_target_pane(&sessions, "%1", Some("one"), Direction::Next, Scope::All),
            None
        );
    }

    #[test]
    fn parse_args_defaults_to_all_scope() {
        assert_eq!(
            parse_args(&["next".into()]),
            Ok((Target::Cycle(Direction::Next), Scope::All))
        );
    }

    #[test]
    fn parse_args_accepts_session_scope_and_previous_alias() {
        assert_eq!(
            parse_args(&["previous".into(), "--scope".into(), "session".into()]),
            Ok((Target::Cycle(Direction::Prev), Scope::Session))
        );
    }

    #[test]
    fn select_pane_by_index_is_one_based() {
        let visible = ids(&["%1", "%2", "%3"]);
        assert_eq!(select_pane_by_index(&visible, 1), Some("%1".into()));
        assert_eq!(select_pane_by_index(&visible, 3), Some("%3".into()));
        assert_eq!(select_pane_by_index(&visible, 4), None);
    }

    #[test]
    fn parse_args_accepts_numeric_index() {
        assert_eq!(
            parse_args(&["3".into()]),
            Ok((Target::Index(3), Scope::All))
        );
    }

    #[test]
    fn parse_args_rejects_zero_index() {
        assert_eq!(parse_args(&["0".into()]), Err(()));
    }

    #[test]
    fn parse_args_rejects_scope_with_numeric_index() {
        assert_eq!(
            parse_args(&["2".into(), "--scope".into(), "session".into()]),
            Err(())
        );
    }

    #[test]
    fn parse_args_accepts_pane_id_target() {
        assert_eq!(
            parse_args(&["%34".into()]),
            Ok((Target::PaneId("%34".into()), Scope::All))
        );
    }

    #[test]
    fn parse_args_rejects_scope_with_pane_id() {
        assert_eq!(
            parse_args(&["%34".into(), "--scope".into(), "session".into()]),
            Err(())
        );
    }

    #[test]
    fn parse_target_distinguishes_index_from_pane_id() {
        assert_eq!(parse_target("3"), Some(Target::Index(3)));
        assert_eq!(parse_target("%3"), Some(Target::PaneId("%3".into())));
    }

    #[test]
    fn focus_by_index_respects_visible_order_and_filters() {
        let mut p1 = pane_at_path("%1", "/tmp/a-repo", "one");
        p1.status = PaneStatus::Running;
        let mut p2 = pane_at_path("%2", "/tmp/a-repo", "one");
        p2.status = PaneStatus::Idle;
        let sessions = vec![session("one", vec![p1, p2])];
        let groups = group::group_panes(&sessions, group::SortMode::Repository);
        let visible = group::visible_pane_ids(&groups, StatusFilter::Running, &RepoFilter::All);
        assert_eq!(select_pane_by_index(&visible, 1), Some("%1".into()));
        assert_eq!(select_pane_by_index(&visible, 2), None);
    }

    #[test]
    fn no_visible_message_is_exact() {
        assert_eq!(
            no_visible_message(),
            "agent-sidebar: no visible agent panes"
        );
    }

    #[test]
    fn no_index_message_substitutes_position() {
        assert_eq!(no_index_message(3), "agent-sidebar: no agent at position 3");
    }

    #[test]
    fn parse_args_accepts_the_notification_target() {
        assert_eq!(
            parse_args(&["notification".into()]),
            Ok((Target::Notification, Scope::All))
        );
        assert_eq!(
            parse_args(&["notification".into(), "--scope".into(), "session".into()]),
            Ok((Target::Notification, Scope::Session))
        );
    }

    #[test]
    fn parse_args_rejects_last_as_an_alias_for_notification() {
        assert_eq!(parse_args(&["last".into()]), Err(()));
    }

    #[test]
    fn parse_args_rejects_an_invalid_scope_for_the_notification_target() {
        assert_eq!(
            parse_args(&["notification".into(), "--scope".into(), "window".into()]),
            Err(())
        );
    }

    #[test]
    fn parse_stamp_lines_reads_a_pane_id_and_its_stamp() {
        let raw = "%1|1700000300|1700000000:notification\n%2|1700000100|1700000000:stop\n";
        assert_eq!(
            parse_stamp_lines(raw),
            vec![
                ("%1".to_string(), 1_700_000_300),
                ("%2".to_string(), 1_700_000_100)
            ]
        );
    }

    #[test]
    fn parse_stamp_lines_skips_panes_whose_option_is_unset() {
        // tmux emits an empty field for an option that was never set.
        let raw = "%1|\n%2|1700000900|1700000000:stop\n";
        assert_eq!(
            parse_stamp_lines(raw),
            vec![("%2".to_string(), 1_700_000_900)]
        );
    }

    #[test]
    fn parse_stamp_lines_skips_malformed_stamps() {
        let raw = "%1|nonsense\n%2|notanumber|fingerprint\n%3|1700000700|fp\n";
        assert_eq!(
            parse_stamp_lines(raw),
            vec![("%3".to_string(), 1_700_000_700)]
        );
    }

    #[test]
    fn parse_stamp_lines_ignores_blank_and_id_less_lines() {
        let raw = "\n|1700000500|fp\n%4|1700000700|fp\n";
        assert_eq!(
            parse_stamp_lines(raw),
            vec![("%4".to_string(), 1_700_000_700)]
        );
    }

    #[test]
    fn parse_stamp_lines_keeps_a_fingerprint_containing_a_colon() {
        // Fingerprints are run-scoped ("<run_id>:<suffix>") and free text
        // beyond that; only `|` is normalised away.
        let raw = "%1|1700000800|1700000000:Permission required: write\n";
        assert_eq!(
            parse_stamp_lines(raw),
            vec![("%1".to_string(), 1_700_000_800)]
        );
    }

    #[test]
    fn parse_stamp_lines_parses_a_real_unquoted_tmux_line() {
        // Captured from a live tmux 3.6a server via stamp_format's
        // unquoted #{pane_id}|#{<key>} shape.
        let raw = "%34|1785210882|1785210318192:stop\n";
        assert_eq!(
            parse_stamp_lines(raw),
            vec![("%34".to_string(), 1_785_210_882)]
        );
    }

    #[test]
    fn parse_stamp_lines_rejects_the_q_quoted_shape_that_caused_the_original_bug() {
        // This is what #{q:pane_id}|#{q:<key>} actually produces: q:
        // backslash-escapes both `|` and `%`, so the first-`|` split hands
        // "1785210882\|1785210318192:stop" to stamp_timestamp, which then
        // splits at the *escaped* `|` and fails to parse "1785210882\\" as
        // a u64. This must NOT yield a timestamp for pane %34 — if it ever
        // does, something re-quoted the format.
        let raw = "\\%34|1785210882\\|1785210318192:stop\n";
        assert_eq!(parse_stamp_lines(raw), Vec::new());
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn last_notified_picks_the_highest_timestamp() {
        let stamps = vec![
            ("%1".to_string(), 100),
            ("%2".to_string(), 300),
            ("%3".to_string(), 200),
        ];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2", "%3"]), &stamps),
            Some("%2".into())
        );
    }

    #[test]
    fn last_notified_takes_the_newest_of_several_entries_for_one_pane() {
        // The three per-key queries are concatenated, so a pane that has
        // fired more than one kind of notification appears more than once.
        let stamps = vec![
            ("%1".to_string(), 100),
            ("%2".to_string(), 200),
            ("%1".to_string(), 400),
            ("%2".to_string(), 300),
        ];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn last_notified_ignores_stamps_for_panes_outside_the_candidate_list() {
        // %9 is newer but is not an agent pane (or is out of scope).
        let stamps = vec![("%1".to_string(), 100), ("%9".to_string(), 999)];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn last_notified_returns_none_when_no_candidate_has_a_stamp() {
        let stamps = vec![("%9".to_string(), 999)];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2"]), &stamps),
            None
        );
    }

    #[test]
    fn last_notified_returns_none_for_an_empty_candidate_list() {
        let stamps = vec![("%1".to_string(), 100)];
        assert_eq!(select_last_notified_pane(&[], &stamps), None);
    }

    #[test]
    fn last_notified_breaks_ties_in_candidate_order() {
        let stamps = vec![("%2".to_string(), 500), ("%1".to_string(), 500)];
        // Candidate order, not stamp order, decides the winner.
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn last_notified_can_resolve_to_the_active_pane() {
        // The active pane is not filtered out — cmd_focus reports that case.
        let stamps = vec![("%1".to_string(), 900), ("%2".to_string(), 100)];
        assert_eq!(
            select_last_notified_pane(&ids(&["%1", "%2"]), &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn session_scope_prefers_an_older_notification_inside_the_active_session() {
        let sessions = vec![
            session("one", vec![pane("%1", true, PaneStatus::Idle, "one")]),
            session("two", vec![pane("%2", false, PaneStatus::Idle, "two")]),
        ];
        // %2 in session "two" is newer, but the cursor is in session "one".
        let stamps = vec![("%1".to_string(), 100), ("%2".to_string(), 999)];
        let eligible = eligible_pane_ids(&sessions, Some("one"), Scope::Session);

        assert_eq!(
            select_last_notified_pane(&eligible, &stamps),
            Some("%1".into())
        );
    }

    #[test]
    fn no_notification_message_names_the_scope() {
        assert_eq!(
            no_notification_message(Scope::All),
            "agent-sidebar: no recent agent notification to focus"
        );
        assert_eq!(
            no_notification_message(Scope::Session),
            "agent-sidebar: no recent agent notification in this session"
        );
    }

    #[test]
    fn already_on_notified_pane_message_is_distinct() {
        assert_eq!(
            ALREADY_ON_NOTIFIED_PANE,
            "agent-sidebar: already on the last notified pane"
        );
        assert_ne!(
            ALREADY_ON_NOTIFIED_PANE,
            no_notification_message(Scope::All)
        );
    }

    #[test]
    fn stamp_format_pairs_the_pane_id_with_an_unquoted_key() {
        let format = stamp_format("@pane_os_notify_task_completed");
        assert_eq!(format, "#{pane_id}|#{@pane_os_notify_task_completed}");
        // #{q:...} escapes both `|` and `%`, which breaks the first-`|`
        // split in parse_stamp_lines and the pane-id match in
        // select_last_notified_pane. If this ever contains "q:" again,
        // the feature is broken — see stamp_format's doc comment.
        assert!(!format.contains("q:"));
    }

    #[test]
    fn stamp_format_is_built_for_every_exposed_stamp_key() {
        let formats: Vec<String> = desktop_notification::stamp_option_keys()
            .iter()
            .map(|key| stamp_format(key))
            .collect();
        assert_eq!(formats.len(), 3);
        for format in &formats {
            assert!(format.starts_with("#{pane_id}|#{@pane_os_notify_"));
            assert!(!format.contains("q:"));
        }
    }
}
