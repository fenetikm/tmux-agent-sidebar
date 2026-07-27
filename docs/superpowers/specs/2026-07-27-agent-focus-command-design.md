# Agent Focus Command Design

## Goal

Add a CLI command that jumps tmux focus to the next or previous running agent pane. The command supports global navigation across all tmux sessions and scoped navigation within the session containing the currently active pane.

## Command

```bash
tmux-agent-sidebar focus <next|prev> [--scope <all|session>]
```

`--scope all` is the default.

Examples:

```bash
tmux-agent-sidebar focus next
tmux-agent-sidebar focus prev --scope all
tmux-agent-sidebar focus next --scope session
tmux-agent-sidebar focus prev --scope session
```

## Behavior

- `focus next --scope all` jumps to the next running agent across all tmux sessions.
- `focus prev --scope all` jumps to the previous running agent across all tmux sessions.
- `focus next --scope session` jumps to the next running agent in the session containing the currently active pane.
- `focus prev --scope session` does the same in reverse.
- Navigation wraps at the start and end of the eligible pane list.
- If there are no eligible running agents, the command exits quietly.
- If the only eligible running agent is the current pane, the command exits quietly.
- Invalid directions or scopes should return a non-zero exit code and print a concise usage message.

## Eligible Panes

Eligible panes are panes discovered by the existing tmux query path whose `PaneStatus` is `Running`.

The command should not depend on the live TUI process or sidebar UI state. This keeps tmux key bindings reliable even when the sidebar pane is closed.

## Ordering

Use the existing `tmux::query_sessions()` traversal order:

```text
sessions -> windows -> panes
```

This is deterministic and close to the sidebar's natural global ordering without introducing persisted UI state.

## Active Pane Resolution

Resolve the active pane with tmux at command runtime. For `--scope session`, use the session containing that active pane as the scope boundary.

## Components

- Add `src/cli/focus.rs` for parsing and command execution.
- Dispatch `focus` from `src/cli/mod.rs`.
- Reuse `tmux::query_sessions()` for pane discovery.
- Reuse `tmux::select_pane(pane_id)` to switch session, window, and pane.
- Add focused unit tests for next, previous, wraparound, session scoping, and no-op cases.

## Error Handling

- Missing or invalid direction returns non-zero with usage.
- Unknown `--scope` values return non-zero with usage.
- Tmux query failures are treated as no eligible panes and exit quietly, matching the existing tmux helper style.

## Testing

Unit tests should cover the pure selection logic without requiring a live tmux server. CLI parsing can be tested directly through the focus module. No UI snapshot tests are needed because this feature does not render frames.

## Tmux Binding Examples

```tmux
bind-key C-n run-shell 'tmux-agent-sidebar focus next --scope all'
bind-key C-p run-shell 'tmux-agent-sidebar focus prev --scope all'
bind-key M-n run-shell 'tmux-agent-sidebar focus next --scope session'
bind-key M-p run-shell 'tmux-agent-sidebar focus prev --scope session'
```
