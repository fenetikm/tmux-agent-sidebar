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

Use `focus` from tmux bindings or scripts to jump between agent panes:

```bash
tmux-agent-sidebar focus <next|prev|notification|<N>> [--waiting] [--scope <all|session>]
```

Examples:

```tmux
bind-key C-n run-shell '"#{@agent_sidebar_bin}" focus next --scope all'
bind-key C-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope all'
bind-key M-n run-shell '"#{@agent_sidebar_bin}" focus next --scope session'
bind-key M-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope session'
bind-key M-l run-shell '"#{@agent_sidebar_bin}" focus notification'
bind-key M-L run-shell '"#{@agent_sidebar_bin}" focus notification --scope session'
bind-key C-w run-shell '"#{@agent_sidebar_bin}" focus next --waiting'
bind-key C-W run-shell '"#{@agent_sidebar_bin}" focus prev --waiting'
```

The plugin sets `@agent_sidebar_bin` to the absolute path of the binary it loaded, so bindings resolve it at press time and do not depend on the binary being on your `PATH`. Define these after the plugin is loaded in your `tmux.conf`.

`--scope all` is the default and walks agent panes across all tmux sessions. `--scope session` walks only the session containing the currently active pane. Every agent pane is eligible regardless of status, so idle and waiting agents are included alongside running ones.

The command wraps at list boundaries. From a pane that isn't an agent pane — a shell, an editor, the sidebar — `next` enters the list at the first agent pane and `prev` at the last, so a single agent pane is still reachable in one press.

When the jump would land on the pane you are already in, the command writes a short note to the tmux status line and exits `0`; when it is not running inside tmux at all it prints an error to stderr and exits non-zero.

### Cycling only the waiting agents

`--waiting` narrows `next` and `prev` to the agents blocked on you, so one key walks the panes that actually need an answer instead of stepping through agents that are still working.

```tmux
bind-key C-w run-shell '"#{@agent_sidebar_bin}" focus next --waiting'
bind-key C-W run-shell '"#{@agent_sidebar_bin}" focus prev --waiting --scope session'
```

"Waiting" is the same condition `list --json` reports as `attention` and the sidebar renders as *waiting for input*: the `@pane_attention` flag is raised, or the status is `waiting`, or the pane is idle with a `@pane_wait_reason` of `idle_prompt`. That last case matters — an idle prompt records its wait reason without raising the attention flag, so a plain status check would skip it.

`--waiting` combines with `--scope`, and everything else about cycling is unchanged: repo-group order, wrap-around, and entering the list from the appropriate end when you press the key from a non-agent pane. It is only valid with `next` and `prev` — `notification` jumps by recency and `<N>` / `%pane_id` name their pane outright, so the flag is rejected there.

When no agent is waiting, the command writes `no agent waiting for input` to the tmux status line and exits `0`.

### Focus by index

`focus <N>` jumps directly to the *N*th agent row visible in the sidebar list. Numbers are **1-based** (`1` = top row). Repo header lines do not count — only agent pane rows.

The list respects the current `@sidebar_filter` and `@sidebar_repo_filter` tmux options, so numbers track what you see in the sidebar and shift when filters change. `--scope` does not apply to numeric targets.

Example bindings:

```tmux
bind 1 run-shell '"#{@agent_sidebar_bin}" focus 1'
bind 2 run-shell '"#{@agent_sidebar_bin}" focus 2'
bind 3 run-shell '"#{@agent_sidebar_bin}" focus 3'
```

When no agents are visible under the current filters, or when `N` is out of range, the command writes a note to the tmux status line and exits `0`.

### The notification target

`focus notification` jumps to the agent pane whose desktop notification fired most recently, rather than walking the list. It reads the same `@pane_os_notify_task_completed`, `@pane_os_notify_task_failed`, and `@pane_os_notify_permission_required` pane options that the notification pipeline writes, so it works with the sidebar closed and survives a sidebar restart.

Because those options are only written when a desktop notification is actually delivered, the command follows your notification settings — an event suppressed by `@sidebar_notifications` or excluded from `@sidebar_notifications_events` leaves no trace for it to find. Repeat notifications with the same fingerprint inside the 120-second cooldown do not refresh the timestamp either.

When no eligible pane has ever notified, or when the most recent notification came from the pane you are already in, the command writes a note to the tmux status line and exits `0`.

### Focus by pane id

`focus %pane_id` jumps directly to a specific tmux pane (for example `%34`). This is the companion to `list --json`: pipe the list through `fzf`, extract the chosen `pane_id`, and call `focus` on it. Numeric targets without a leading `%` remain sidebar index jumps — `focus 3` and `focus %3` are different operations.

### Fuzzy picker with fzf

`list` prints every agent pane the sidebar would show, in the same order as the pane list. Use `--json` when you want structured output for `jq`; the default is tab-separated columns (`index`, `agent`, `status`, `repo`, `prompt`, `pane_id`).

```bash
# Plain text — no jq required
pane=$(tmux-agent-sidebar list | fzf | awk '{print $NF}')
[ -n "$pane" ] && tmux-agent-sidebar focus "$pane"

# JSON — richer display via the pre-built label field
pane=$(
  tmux-agent-sidebar list --json \
    | jq -r '.panes[] | "\(.label)\t\(.pane_id)"' \
    | fzf --delimiter=$'\t' --with-nth=1 \
    | cut -f2
)
[ -n "$pane" ] && tmux-agent-sidebar focus "$pane"
```

Flags:

| Flag | Effect |
| ---- | ------ |
| `--json` | Emit `{ "panes": [ … ] }` on stdout |
| `--scope session` | Limit to the current tmux session |
| `--all-panes` | Ignore `@sidebar_filter` / `@sidebar_repo_filter` |

Example tmux binding:

```tmux
bind-key C-f run-shell 'pane=$(\"#{@agent_sidebar_bin}\" list | fzf | awk \"{print \\$NF}\") && [ -n \"$pane\" ] && \"#{@agent_sidebar_bin}\" focus \"$pane\"'
```

## Example status line snippet

```bash
# only show the indicator when a status is set
set -g status-right '#(tmux show -t #{pane_id} -pv @pane_status) | %H:%M'
```

If you pair this with a custom notifier, mirror the filter set supported by `@sidebar_notifications_events` — see [Notifications](/tmux-agent-sidebar/features/notifications/).
