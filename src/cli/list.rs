use std::collections::HashMap;

use crate::group::{self, PaneGitInfo, RepoGroup};
use crate::state::{RepoFilter, StatusFilter};
use crate::tmux::{PaneInfo, PaneStatus, SessionInfo};
use crate::ui::text::truncate_to_width;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Plain,
    Json,
}

#[derive(Debug, Clone)]
struct ListOptions {
    format: OutputFormat,
    scope: Scope,
    ignore_filters: bool,
}

#[derive(Debug, Clone)]
struct PaneEntry {
    index: u32,
    pane: PaneInfo,
    git: PaneGitInfo,
    repo: String,
}

pub fn cmd_list(args: &[String]) -> i32 {
    let options = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(()) => {
            usage();
            return 1;
        }
    };

    let sessions = crate::tmux::query_sessions();
    let active_session = active_pane_id()
        .as_deref()
        .and_then(crate::tmux::pane_session_name)
        .map(|name| name.to_string());

    let entries = collect_entries(
        &sessions,
        options.scope,
        active_session.as_deref(),
        options.ignore_filters,
    );

    match options.format {
        OutputFormat::Json => print_json(&entries),
        OutputFormat::Plain => print_plain(&entries),
    }
    0
}

fn usage() {
    eprintln!("usage: tmux-agent-sidebar list [--json] [--scope <all|session>] [--all-panes]");
    eprintln!("  --json         emit structured JSON for scripting (default: tab-separated)");
    eprintln!("  --scope        limit to current tmux session (default: all sessions)");
    eprintln!("  --all-panes    ignore sidebar status/repo filters (default: match sidebar list)");
}

fn parse_args(args: &[String]) -> Result<ListOptions, ()> {
    let mut format = OutputFormat::Plain;
    let mut scope = Scope::All;
    let mut ignore_filters = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--json" => format = OutputFormat::Json,
            "--all-panes" => ignore_filters = true,
            "--scope" => {
                let value = args.get(index + 1).ok_or(())?;
                scope = match value.as_str() {
                    "all" => Scope::All,
                    "session" => Scope::Session,
                    _ => return Err(()),
                };
                index += 1;
            }
            _ => return Err(()),
        }
        index += 1;
    }

    if matches!(scope, Scope::Session) && active_pane_id().is_none() {
        return Err(());
    }

    Ok(ListOptions {
        format,
        scope,
        ignore_filters,
    })
}

fn active_pane_id() -> Option<String> {
    crate::tmux::run_tmux(&["display-message", "-p", "#{pane_id}"])
        .map(|pane_id| pane_id.trim().to_string())
        .filter(|pane_id| !pane_id.is_empty())
}

fn scoped_sessions<'a>(
    sessions: &'a [SessionInfo],
    scope: Scope,
    active_session: Option<&str>,
) -> Vec<SessionInfo> {
    match scope {
        Scope::All => sessions.to_vec(),
        Scope::Session => {
            let Some(name) = active_session else {
                return Vec::new();
            };
            sessions
                .iter()
                .filter(|session| session.session_name == name)
                .cloned()
                .collect()
        }
    }
}

fn collect_entries(
    sessions: &[SessionInfo],
    scope: Scope,
    active_session: Option<&str>,
    ignore_filters: bool,
) -> Vec<PaneEntry> {
    let scoped = scoped_sessions(sessions, scope, active_session);
    let groups = group::group_panes(&scoped, crate::ui::sort_mode_from_tmux());
    let (status_filter, repo_filter) = if ignore_filters {
        (StatusFilter::All, RepoFilter::All)
    } else {
        super::focus::load_sidebar_filters()
    };
    let visible = group::visible_pane_ids(&groups, status_filter, &repo_filter);
    let lookup = pane_lookup(&groups);

    visible
        .into_iter()
        .enumerate()
        .filter_map(|(offset, pane_id)| {
            let (pane, git, repo) = lookup.get(&pane_id)?;
            Some(PaneEntry {
                index: (offset + 1) as u32,
                pane: (*pane).clone(),
                git: git.clone(),
                repo: repo.clone(),
            })
        })
        .collect()
}

fn pane_lookup(groups: &[RepoGroup]) -> HashMap<String, (&PaneInfo, PaneGitInfo, String)> {
    let mut lookup = HashMap::new();
    for group in groups {
        for (pane, git) in &group.panes {
            lookup.insert(
                pane.pane_id.clone(),
                (pane, git.clone(), group.name.clone()),
            );
        }
    }
    lookup
}

fn status_label(status: &PaneStatus) -> &'static str {
    match status {
        PaneStatus::Running => "running",
        PaneStatus::Background => "background",
        PaneStatus::Waiting => "waiting",
        PaneStatus::Idle => "idle",
        PaneStatus::Error => "error",
        PaneStatus::Unknown => "unknown",
    }
}

fn build_label(entry: &PaneEntry) -> String {
    let mut parts = vec![
        entry.pane.agent.label().to_string(),
        status_label(&entry.pane.status).to_string(),
        entry.repo.clone(),
    ];
    if !entry.pane.session_name.is_empty() {
        parts.push(entry.pane.session_name.clone());
    } else if !entry.pane.tmux_session.is_empty() {
        parts.push(entry.pane.tmux_session.clone());
    }
    if !entry.pane.prompt.is_empty() {
        parts.push(truncate_to_width(&entry.pane.prompt, 60));
    }
    parts.join(" · ")
}

fn print_plain(entries: &[PaneEntry]) {
    for entry in entries {
        let prompt = truncate_to_width(&entry.pane.prompt, 40);
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            entry.index,
            entry.pane.agent.label(),
            status_label(&entry.pane.status),
            entry.repo,
            prompt,
            entry.pane.pane_id,
        );
    }
}

fn pane_json(entry: &PaneEntry) -> serde_json::Value {
    serde_json::json!({
        "index": entry.index,
        "pane_id": entry.pane.pane_id,
        "agent": entry.pane.agent.label(),
        "status": status_label(&entry.pane.status),
        "attention": entry.pane.needs_user_attention(),
        "wait_reason": entry.pane.wait_reason,
        "session_name": entry.pane.session_name,
        "tmux_session": entry.pane.tmux_session,
        "window_id": entry.pane.window_id,
        "repo": entry.repo,
        "branch": entry.git.branch.clone().unwrap_or_default(),
        "path": entry.pane.path,
        "prompt": entry.pane.prompt,
        "worktree": entry.pane.worktree.name,
        "active": entry.pane.pane_active,
        "label": build_label(entry),
    })
}

fn json_payload(entries: &[PaneEntry]) -> serde_json::Value {
    let panes: Vec<serde_json::Value> = entries.iter().map(pane_json).collect();
    serde_json::json!({ "panes": panes })
}

fn print_json(entries: &[PaneEntry]) {
    println!(
        "{}",
        serde_json::to_string_pretty(&json_payload(entries)).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{
        AgentType, PaneInfo, PaneStatus, PermissionMode, WindowInfo, WorktreeMetadata,
    };

    fn pane(id: &str, status: PaneStatus, session_name: &str) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: false,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/repo/a".into(),
            current_command: String::new(),
            prompt: "fix tests".into(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: Vec::new(),
            pane_pid: None,
            worktree: WorktreeMetadata::default(),
            session_id: None,
            session_name: session_name.into(),
            tmux_session: "main".into(),
            window_id: "@1".into(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }

    fn session(panes: Vec<PaneInfo>) -> SessionInfo {
        SessionInfo {
            session_name: "main".into(),
            windows: vec![WindowInfo {
                window_id: "@1".into(),
                window_name: "editor".into(),
                window_active: true,
                auto_rename: false,
                panes,
            }],
        }
    }

    #[test]
    fn parse_args_defaults() {
        let parsed = parse_args(&[]).unwrap();
        assert_eq!(parsed.format, OutputFormat::Plain);
        assert_eq!(parsed.scope, Scope::All);
        assert!(!parsed.ignore_filters);
    }

    #[test]
    fn parse_args_json_and_all_panes() {
        let parsed = parse_args(&["--json".into(), "--all-panes".into()]).unwrap();
        assert_eq!(parsed.format, OutputFormat::Json);
        assert!(parsed.ignore_filters);
    }

    #[test]
    fn collect_entries_respects_visible_order() {
        let sessions = vec![session(vec![
            pane("%1", PaneStatus::Running, "alpha"),
            pane("%2", PaneStatus::Idle, "beta"),
        ])];
        let entries = collect_entries(&sessions, Scope::All, None, true);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].pane.pane_id, "%1");
        assert_eq!(entries[0].index, 1);
        assert_eq!(entries[1].pane.pane_id, "%2");
        assert_eq!(entries[1].index, 2);
    }

    #[test]
    fn collect_entries_scopes_to_active_session() {
        let sessions = vec![
            SessionInfo {
                session_name: "work".into(),
                windows: vec![WindowInfo {
                    window_id: "@1".into(),
                    window_name: "a".into(),
                    window_active: true,
                    auto_rename: false,
                    panes: vec![pane("%1", PaneStatus::Running, "alpha")],
                }],
            },
            SessionInfo {
                session_name: "other".into(),
                windows: vec![WindowInfo {
                    window_id: "@2".into(),
                    window_name: "b".into(),
                    window_active: false,
                    auto_rename: false,
                    panes: vec![pane("%2", PaneStatus::Running, "beta")],
                }],
            },
        ];
        let entries = collect_entries(&sessions, Scope::Session, Some("work"), true);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pane.pane_id, "%1");
    }

    #[test]
    fn build_label_includes_core_fields() {
        let entry = PaneEntry {
            index: 1,
            pane: pane("%1", PaneStatus::Waiting, "my-task"),
            git: PaneGitInfo {
                branch: Some("main".into()),
                ..PaneGitInfo::default()
            },
            repo: "my-repo".into(),
        };
        let label = build_label(&entry);
        assert!(label.contains("claude"));
        assert!(label.contains("waiting"));
        assert!(label.contains("my-repo"));
        assert!(label.contains("my-task"));
        assert!(label.contains("fix tests"));
    }

    #[test]
    fn json_marks_idle_prompt_pane_as_needing_attention() {
        // `idle_prompt` notifications are meta-only: they record the wait
        // reason without raising `@pane_attention`, so the raw flag alone
        // hides a pane the sidebar renders as "waiting for input".
        let mut waiting = pane("%7", PaneStatus::Idle, "task");
        waiting.wait_reason = "idle_prompt".into();
        let entry = PaneEntry {
            index: 1,
            pane: waiting,
            git: PaneGitInfo::default(),
            repo: "repo".into(),
        };
        let value = json_payload(&[entry]);
        assert_eq!(value["panes"][0]["attention"], true);
        assert_eq!(value["panes"][0]["wait_reason"], "idle_prompt");
    }

    #[test]
    fn json_ignores_stale_idle_prompt_on_running_pane() {
        // `@pane_wait_reason` is not cleared when a pane resumes work, so a
        // running pane can still carry `idle_prompt` from its last wait.
        let mut running = pane("%10", PaneStatus::Running, "task");
        running.wait_reason = "idle_prompt".into();
        let entry = PaneEntry {
            index: 1,
            pane: running,
            git: PaneGitInfo::default(),
            repo: "repo".into(),
        };
        let value = json_payload(&[entry]);
        assert_eq!(value["panes"][0]["attention"], false);
    }

    #[test]
    fn json_leaves_plain_idle_pane_without_attention() {
        let entry = PaneEntry {
            index: 1,
            pane: pane("%8", PaneStatus::Idle, "task"),
            git: PaneGitInfo::default(),
            repo: "repo".into(),
        };
        let value = json_payload(&[entry]);
        assert_eq!(value["panes"][0]["attention"], false);
        assert_eq!(value["panes"][0]["wait_reason"], "");
    }

    #[test]
    fn json_marks_waiting_pane_as_needing_attention() {
        let entry = PaneEntry {
            index: 1,
            pane: pane("%9", PaneStatus::Waiting, "task"),
            git: PaneGitInfo::default(),
            repo: "repo".into(),
        };
        let value = json_payload(&[entry]);
        assert_eq!(value["panes"][0]["attention"], true);
    }

    #[test]
    fn print_json_is_valid_and_includes_pane_id() {
        let entry = PaneEntry {
            index: 1,
            pane: pane("%42", PaneStatus::Running, "task"),
            git: PaneGitInfo::default(),
            repo: "repo".into(),
        };
        let mut buf = Vec::new();
        {
            use std::io::Write;
            let entries = [entry];
            let panes: Vec<serde_json::Value> = entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "index": entry.index,
                        "pane_id": entry.pane.pane_id,
                        "label": build_label(entry),
                    })
                })
                .collect();
            let json = serde_json::json!({ "panes": panes });
            write!(buf, "{json}").unwrap();
        }
        let value: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(value["panes"][0]["pane_id"], "%42");
    }
}
