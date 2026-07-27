# Agent Focus Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `tmux-agent-sidebar focus <next|prev> [--scope <all|session>]` to jump tmux focus between running agent panes.

**Architecture:** Keep the command in a focused CLI module with pure selection logic that is unit-testable without tmux. Runtime code queries existing tmux session state, asks tmux for the current client pane, filters eligible running agents, and reuses `tmux::select_pane` for the jump.

**Tech Stack:** Rust 2024, existing tmux query helpers, Cargo unit tests.

---

## File Structure

- Create `src/cli/focus.rs`: command parsing, selection helpers, runtime execution, unit tests.
- Modify `src/cli/mod.rs`: declare the module and dispatch the `focus` subcommand.
- Modify `docs/superpowers/specs/2026-07-27-agent-focus-command-design.md`: no implementation changes required unless behavior changes during execution.

## Task 1: Add Focus Selection Tests

**Files:**
- Create: `src/cli/focus.rs`

- [ ] **Step 1: Write failing tests and pure helper signatures**

Create `src/cli/focus.rs` with this initial content:

```rust
use crate::tmux::{PaneInfo, PaneStatus, SessionInfo};

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
    let _ = (sessions, active_pane_id, direction, scope);
    None
}

pub fn cmd_focus(_args: &[String]) -> i32 {
    1
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
        let sessions = vec![session("one", vec![pane("%1", true, PaneStatus::Running, "one")])];

        assert_eq!(
            select_target_pane(&sessions, "%1", Direction::Next, Scope::All),
            None
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test cli::focus`

Expected: tests compile after module dispatch exists in Task 2, then fail because `select_target_pane` returns `None`.

## Task 2: Wire The CLI Module

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/focus.rs`

- [ ] **Step 1: Expose the module and dispatch command**

Change `src/cli/mod.rs` module declarations and command match:

```rust
pub mod capture;
mod focus;
mod hook;
mod label;
pub mod plugin_state;
pub(crate) mod session_filter;
pub(crate) mod setup;
pub(crate) mod shared_html;
mod spawn;
pub(crate) mod toggle;
```

Add this match arm before `--version`:

```rust
"focus" => focus::cmd_focus(rest),
```

- [ ] **Step 2: Run tests to verify focus tests fail for selection logic**

Run: `cargo test cli::focus`

Expected: the `cli::focus` tests run and selection tests fail because the helper still returns `None`.

## Task 3: Implement Pure Selection Logic

**Files:**
- Modify: `src/cli/focus.rs`

- [ ] **Step 1: Replace `select_target_pane` with working logic**

Use this implementation:

```rust
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

fn running_pane_ids(
    sessions: &[SessionInfo],
    active_pane_id: &str,
    scope: Scope,
) -> Vec<String> {
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

    let current = pane_ids.iter().position(|pane_id| pane_id == active_pane_id);
    let target_index = match (direction, current) {
        (Direction::Next, Some(index)) => (index + 1) % pane_ids.len(),
        (Direction::Prev, Some(0)) => pane_ids.len() - 1,
        (Direction::Prev, Some(index)) => index - 1,
        (Direction::Next, None) => 0,
        (Direction::Prev, None) => pane_ids.len() - 1,
    };

    pane_ids.get(target_index).cloned()
}
```

- [ ] **Step 2: Run tests to verify selection passes**

Run: `cargo test cli::focus`

Expected: all `cli::focus` tests pass except any parser/runtime tests not yet added.

## Task 4: Implement CLI Parsing And Runtime Execution

**Files:**
- Modify: `src/cli/focus.rs`

- [ ] **Step 1: Add parser helpers and command execution**

Replace `cmd_focus` with this implementation and add the helper functions above it:

```rust
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
    let direction = args.first().and_then(|value| parse_direction(value)).ok_or(())?;
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
```

- [ ] **Step 2: Add parser tests**

Add these tests to the existing test module:

```rust
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
```

- [ ] **Step 3: Run focus tests**

Run: `cargo test cli::focus`

Expected: all focus tests pass.

## Task 5: Verify And Format

**Files:**
- Modify: any files changed by `cargo fmt`

- [ ] **Step 1: Format**

Run: `cargo fmt`

Expected: command exits successfully.

- [ ] **Step 2: Run all tests**

Run: `cargo test`

Expected: all tests pass.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy`

Expected: no warnings or errors.

- [ ] **Step 4: Build release binary**

Run: `cargo build --release`

Expected: release build completes successfully.

- [ ] **Step 5: Manual command examples**

Use these tmux bindings or run-shell calls after building:

```tmux
bind-key C-n run-shell 'tmux-agent-sidebar focus next --scope all'
bind-key C-p run-shell 'tmux-agent-sidebar focus prev --scope all'
bind-key M-n run-shell 'tmux-agent-sidebar focus next --scope session'
bind-key M-p run-shell 'tmux-agent-sidebar focus prev --scope session'
```

Expected: key bindings jump between running agent panes, wrapping at list boundaries.

## Self-Review

- Spec coverage: command syntax, default scope, all/session scope, wrapping, no-op behavior, invalid usage, tmux query reuse, and tests are covered.
- Placeholder scan: no placeholders remain.
- Type consistency: helper signatures use existing `SessionInfo`, `PaneInfo`, and `PaneStatus` types consistently.
