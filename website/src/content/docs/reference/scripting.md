---
title: Scripting
description: Read agent status from your own shell scripts or status bar.
---

The sidebar writes agent state into tmux pane options on every hook event, so you can pick them up from any script with `tmux show -t <pane> -pv <key>`.

## Reading pane options

```bash
# Get a specific pane's agent status
tmux show -t "$pane_id" -pv @pane_status
# → running / background / waiting / idle / error / (empty)

# Get agent type
tmux show -t "$pane_id" -pv @pane_agent
# → claude / codex / opencode / (empty)
```

## Available pane options

| Key                        | Value                                                              |
| -------------------------- | ------------------------------------------------------------------ |
| `@pane_status`             | `running` / `background` / `waiting` / `idle` / `error` / empty    |
| `@pane_attention`          | `1` while the pane is flagged for attention, otherwise empty        |
| `@pane_agent`              | `claude` / `codex` / `opencode` / empty                             |
| `@pane_name`               | Friendly agent/session name (from `/rename` on Claude)              |
| `@pane_role`               | `sidebar` for the sidebar pane itself; empty for agent panes        |
| `@pane_prompt`             | Latest user prompt text or response preview                         |
| `@pane_prompt_source`      | `user` when the prompt field holds the user's prompt, `response` when it holds the agent's last reply |
| `@pane_started_at`         | Epoch seconds of the last `UserPromptSubmit`                        |
| `@pane_wait_reason`        | Wait-reason text (populated only when waiting)                      |
| `@pane_bg_cmd`             | Latest sanitized background Bash command (Claude `run_in_background`); empty when no bg shell is tracked. Cleared automatically by a `ps` liveness sweep when the process exits. |
| `@pane_subagents`          | Comma-separated subagent labels (Claude only)                       |
| `@pane_cwd`                | Working directory reported by the agent (preferred over `pane_current_path`) |
| `@pane_permission_mode`    | Permission-mode string for the badge (`plan` / `edit` / `auto` / `!` / …) |
| `@pane_worktree_name`      | Worktree label when the pane was spawned from the sidebar           |
| `@pane_worktree_branch`    | Branch that was auto-created for the worktree                       |
| `@pane_session_id`         | Agent session ID (opaque; useful for correlating logs)              |

## Use cases

- **Status bar integration** — surface `@pane_status` in your tmux status line to light up when an agent needs attention.
- **Custom notifications** — if you don't like the built-in desktop notifications, build your own pipeline off the same pane options.
- **Shell aliases** — gate side-effectful commands on agent state.
- **Agent navigation** — bind `tmux-agent-sidebar focus` commands to jump between agent panes.

## Agent focus command

Use `focus` from tmux bindings or scripts to jump to the next or previous agent pane:

```bash
tmux-agent-sidebar focus <next|prev> [--scope <all|session>]
```

Examples:

```tmux
bind-key C-n run-shell '"#{@agent_sidebar_bin}" focus next --scope all'
bind-key C-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope all'
bind-key M-n run-shell '"#{@agent_sidebar_bin}" focus next --scope session'
bind-key M-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope session'
```

The plugin sets `@agent_sidebar_bin` to the absolute path of the binary it loaded, so bindings resolve it at press time and do not depend on the binary being on your `PATH`. Define these after the plugin is loaded in your `tmux.conf`.

`--scope all` is the default and walks agent panes across all tmux sessions. `--scope session` walks only the session containing the currently active pane. Every agent pane is eligible regardless of status, so idle and waiting agents are included alongside running ones.

The command wraps at list boundaries. From a pane that isn't an agent pane — a shell, an editor, the sidebar — `next` enters the list at the first agent pane and `prev` at the last, so a single agent pane is still reachable in one press.

When the jump would land on the pane you are already in, the command writes a short note to the tmux status line and exits `0`; when it is not running inside tmux at all it prints an error to stderr and exits non-zero.

## Example status line snippet

```bash
# only show the indicator when a status is set
set -g status-right '#(tmux show -t #{pane_id} -pv @pane_status) | %H:%M'
```

If you pair this with a custom notifier, mirror the filter set supported by `@sidebar_notifications_events` — see [Notifications](/tmux-agent-sidebar/features/notifications/).
