use crate::tmux::{PaneStatus, SessionInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Next,
    Prev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    Session,
}

fn select_target_pane(
    sessions: &[SessionInfo],
    active_pane_id: &str,
    direction: Direction,
    scope: Scope,
) -> Option<String> {
    let pane_ids = running_pane_ids(sessions, active_pane_id, scope);
    if pane_ids.len() <= 1 {
        return None;
    }

    let current = pane_ids
        .iter()
        .position(|pane_id| pane_id == active_pane_id);
    let target_index = match (direction, current) {
        (Direction::Next, Some(index)) => (index + 1) % pane_ids.len(),
        (Direction::Prev, Some(0)) => pane_ids.len() - 1,
        (Direction::Prev, Some(index)) => index - 1,
        (Direction::Next, None) => 0,
        (Direction::Prev, None) => pane_ids.len() - 1,
    };

    pane_ids.get(target_index).cloned()
}

fn active_session_name<'a>(sessions: &'a [SessionInfo], active_pane_id: &str) -> Option<&'a str> {
    for session in sessions {
        for window in &session.windows {
            for pane in &window.panes {
                if pane.pane_id == active_pane_id {
                    return Some(session.session_name.as_str());
                }
            }
        }
    }
    None
}

fn running_pane_ids(sessions: &[SessionInfo], active_pane_id: &str, scope: Scope) -> Vec<String> {
    let scoped_session = match scope {
        Scope::All => None,
        Scope::Session => active_session_name(sessions, active_pane_id),
    };

    sessions
        .iter()
        .filter(|session| scoped_session.is_none_or(|name| session.session_name == name))
        .flat_map(|session| session.windows.iter())
        .flat_map(|window| window.panes.iter())
        .filter(|pane| pane.status == PaneStatus::Running)
        .map(|pane| pane.pane_id.clone())
        .collect()
}

fn usage() {
    eprintln!("usage: tmux-agent-sidebar focus <next|prev> [--scope <all|session>]");
}

fn parse_direction(value: &str) -> Option<Direction> {
    match value {
        "next" => Some(Direction::Next),
        "prev" | "previous" => Some(Direction::Prev),
        _ => None,
    }
}

fn parse_args(args: &[String]) -> Result<(Direction, Scope), ()> {
    let direction = args
        .first()
        .and_then(|value| parse_direction(value))
        .ok_or(())?;
    let mut scope = Scope::All;
    let mut index = 1;

    while index < args.len() {
        match args[index].as_str() {
            "--scope" => {
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

    Ok((direction, scope))
}

fn active_pane_id() -> Option<String> {
    crate::tmux::run_tmux(&["display-message", "-p", "#{pane_id}"])
        .map(|pane_id| pane_id.trim().to_string())
        .filter(|pane_id| !pane_id.is_empty())
}

pub fn cmd_focus(args: &[String]) -> i32 {
    let (direction, scope) = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(()) => {
            usage();
            return 1;
        }
    };

    let sessions = crate::tmux::query_sessions();
    let Some(active_pane_id) = active_pane_id() else {
        return 0;
    };
    let Some(target_pane_id) = select_target_pane(&sessions, &active_pane_id, direction, scope)
    else {
        return 0;
    };

    crate::tmux::select_pane(&target_pane_id);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{AgentType, PaneInfo, PermissionMode, WindowInfo, WorktreeMetadata};

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

    #[test]
    fn next_all_selects_next_running_pane() {
        let sessions = vec![session(
            "one",
            vec![
                pane("%1", true, PaneStatus::Running, "one"),
                pane("%2", false, PaneStatus::Idle, "one"),
                pane("%3", false, PaneStatus::Running, "one"),
            ],
        )];

        assert_eq!(
            select_target_pane(&sessions, "%1", Direction::Next, Scope::All),
            Some("%3".into())
        );
    }

    #[test]
    fn prev_all_wraps_to_last_running_pane() {
        let sessions = vec![
            session("one", vec![pane("%1", true, PaneStatus::Running, "one")]),
            session("two", vec![pane("%2", false, PaneStatus::Running, "two")]),
        ];

        assert_eq!(
            select_target_pane(&sessions, "%1", Direction::Prev, Scope::All),
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
            select_target_pane(&sessions, "%2", Direction::Next, Scope::Session),
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
            select_target_pane(&sessions, "%1", Direction::Next, Scope::Session),
            Some("%2".into())
        );
    }

    #[test]
    fn single_eligible_running_pane_returns_none() {
        let sessions = vec![session(
            "one",
            vec![pane("%1", true, PaneStatus::Running, "one")],
        )];

        assert_eq!(
            select_target_pane(&sessions, "%1", Direction::Next, Scope::All),
            None
        );
    }

    #[test]
    fn parse_args_defaults_to_all_scope() {
        assert_eq!(
            parse_args(&["next".into()]),
            Ok((Direction::Next, Scope::All))
        );
    }

    #[test]
    fn parse_args_accepts_session_scope_and_previous_alias() {
        assert_eq!(
            parse_args(&["previous".into(), "--scope".into(), "session".into()]),
            Ok((Direction::Prev, Scope::Session))
        );
    }

    #[test]
    fn parse_args_rejects_invalid_scope() {
        assert_eq!(
            parse_args(&["next".into(), "--scope".into(), "window".into()]),
            Err(())
        );
    }
}
