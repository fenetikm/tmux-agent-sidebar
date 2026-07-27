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

- `focus next --scope all` jumps to the next agent pane across all tmux sessions.
- `focus prev --scope all` jumps to the previous agent pane across all tmux sessions.
- `focus next --scope session` jumps to the next agent pane in the session containing the currently active pane.
- `focus prev --scope session` does the same in reverse.
- Navigation wraps at the start and end of the eligible pane list.
- If the only eligible agent pane is the current pane (or there are none), the command writes a short explanation to the tmux status line and exits `0`. A silent no-op is indistinguishable from a broken binary, so the command always reports why nothing moved.
- If the command cannot resolve an active pane (i.e. it is not running inside tmux), it prints an error to stderr and returns a non-zero exit code.
- Invalid directions or scopes should return a non-zero exit code and print a concise usage message.

## Eligible Panes

Eligible panes are every pane discovered by the existing tmux query path, regardless of `PaneStatus`. That path already drops panes with no `@pane_agent` marker and the sidebar's own pane, so what remains is exactly the set of agent panes.

Status is deliberately *not* a filter. Restricting to `Running` made the command a no-op in the common case of one working agent plus several idle ones — and idle or waiting agents are precisely the ones a user wants to jump to.

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
