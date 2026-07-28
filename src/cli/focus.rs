use crate::desktop_notification;
use crate::tmux::SessionInfo;

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

/// Every agent pane eligible for cycling, in tmux enumeration order.
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

    sessions
        .iter()
        .filter(|session| scoped_session.is_none_or(|name| session.session_name == name))
        .flat_map(|session| session.windows.iter())
        .flat_map(|window| window.panes.iter())
        .map(|pane| pane.pane_id.clone())
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
            let pane_id = pane_id.trim();
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
/// Ties resolve to the earlier pane in `eligible` (tmux enumeration
/// order). The strict `>` is what enforces that: `max_by_key` would keep
/// the *last* equal maximum instead.
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
        eprintln!("tmux-agent-sidebar focus: not running inside tmux");
        return 1;
    };
    let active_session = crate::tmux::pane_session_name(&active_pane_id);
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
    fn last_notified_breaks_ties_in_enumeration_order() {
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
}
