# Focus Notification Target Design

## Goal

Extend the existing `focus` command with a third target, `notification`, that jumps tmux focus to the pane whose desktop notification fired most recently. This answers "something pinged me — take me to it" without the user cycling through every agent pane to find the one that spoke.

This builds on the design in `2026-07-27-agent-focus-command-design.md`; cycling behaviour there is unchanged.

## Command

```bash
tmux-agent-sidebar focus <next|prev|notification> [--scope <all|session>]
```

`--scope all` remains the default and applies to all three targets.

Examples:

```bash
tmux-agent-sidebar focus notification
tmux-agent-sidebar focus notification --scope session
```

`last` is deliberately **not** accepted as an alias. The token is `notification` only.

## Definition of "Last Notification"

The target is the pane whose most recent *delivered desktop notification* is newest. Delivered is the operative word: the source of truth is the stamp that `desktop_notification::notify_if_allowed` writes after a notification is successfully sent.

This means the command follows the user's notification settings. With `@sidebar_notifications off`, or with an event excluded from `@sidebar_notifications_events`, no stamp is written and that event is invisible to `focus notification`. That is intended — the command jumps to what actually interrupted the user, not to what would have.

Two alternatives were considered and rejected:

- Recording notification-worthy events regardless of delivery. Requires a new write in the hook path and diverges from what the user actually saw.
- Tracking the most recent attention state (waiting on permission or input). This ignores completion notifications, which are a large share of what a user wants to jump back to.

### Cooldown caveat

`notify_if_allowed` suppresses a repeat of the same fingerprint within `DESKTOP_NOTIFICATION_COOLDOWN_SECS` (120 s), and a suppressed notification does not refresh the stamp. So if pane A notifies twice about the same thing and pane B notifies once between them, `focus notification` resolves to B. This is consistent with the definition above — B's notification is the most recent one actually delivered — and needs no mitigation.

## Data Source

Three per-pane tmux options already carry the required timestamps, each encoded as `timestamp|fingerprint`:

- `@pane_os_notify_task_completed`
- `@pane_os_notify_task_failed`
- `@pane_os_notify_permission_required`

A pane's notification recency is the maximum timestamp across the three. All three kinds rank equally; there is no per-kind priority.

Because these live in tmux pane options, the command works with the sidebar TUI closed and survives a sidebar restart, consistent with the existing focus command's independence from live UI state.

### Reading the stamps

Read them with dedicated `tmux list-panes -a -F` calls issued **only** on the `notification` path — **one call per stamp key**, each using the two-field format `#{pane_id}|#{<key>}`.

One call per key rather than one call listing all three keys, because the stamp encoding already spends the `|` separator. A stamp value is `timestamp|fingerprint`, and `normalize_fingerprint` guarantees the fingerprint itself contains no `|`. So a two-field line splits unambiguously at its first `|`: everything before is the pane id (which never contains `|`), everything after is exactly one stamp value that `stamp_timestamp` can parse. Packing three stamp values into one line loses that property — after splitting on `|` no field carries a separator any more, so the values can't be recovered, and picking the numeric-looking fields would mistake an all-digit fingerprint for a timestamp.

The keys are deliberately not added to `pane_format()` in `src/tmux/query.rs`. That format has 28 fields kept in lock-step with hand-maintained index constants and a `MIN_FIELDS` guard, and the TUI has no use for notify stamps. Extending it would impose maintenance cost on every consumer to serve one CLI subcommand.

The cost of the chosen approach is three subprocess calls per `focus notification` invocation. That is not on any hot path — it runs once per keypress, and `tmux::select_pane` already makes three calls of its own.

Fields are deliberately *not* quoted with `#{q:...}`. That modifier escapes both `|` and `%`, which would corrupt both halves of the line: the escaped `|` breaks the first-`|` split above, and an escaped `%` in the pane id would never match an id from `query_sessions`. `src/tmux/query.rs` can use `#{q:...}` safely only because it unescapes the result afterwards via `split_tmux_fields`; this path does not.

## Selection

Candidate panes are exactly `eligible_pane_ids(sessions, active_session, scope)` — the same list cycling uses. This inherits the agent-pane filter, the sidebar-pane exclusion, the session scoping (including its refusal to degrade `--scope session` into `--scope all` when the session cannot be resolved), and the tmux enumeration ordering.

Stamps for panes outside the candidate list are ignored, which is how the scope filter and the agent-pane filter take effect.

Selection is a pure function:

```rust
fn select_last_notified_pane(eligible: &[String], stamps: &[(String, u64)]) -> Option<String>
```

`stamps` is the concatenation of the three per-key query results, so it may hold **several entries for the same pane** — one per notification kind that pane has fired. A pane's recency is the maximum over its own entries; no separate merge step is needed.

Ties on identical timestamps resolve to whichever pane comes first in tmux enumeration order.

Unlike `next`/`prev`, the active pane is **not** filtered out of the result. Whether the newest notification came from the pane the user is already on is a meaningful distinction the command reports on, rather than something to hide.

## Behavior

| Result | Behaviour | Exit |
|---|---|---|
| No candidate pane carries a stamp | `show_message` with the no-notification message for the scope | 0 |
| Winner is the active pane | `show_message("agent-sidebar: already on the last notified pane")` | 0 |
| Winner is another pane | `tmux::select_pane(id)` | 0 |
| Not running inside tmux | existing stderr message | 1 |
| Invalid target or scope | usage to stderr | 1 |

Status-line messages, following the shape of the existing `no_target_message`:

- `--scope all`: `agent-sidebar: no recent agent notification to focus`
- `--scope session`: `agent-sidebar: no recent agent notification in this session`
- already on target (both scopes): `agent-sidebar: already on the last notified pane`

Every non-movement outcome reports itself. A silent no-op is indistinguishable from a broken binary.

Malformed or empty stamp values are skipped, so a corrupt option value on one pane cannot break the command for the rest. A pane with no usable stamp on any of the three keys is omitted from `parse_stamp_lines`' output entirely rather than reported with a zero timestamp — a pane that has never notified must never win the comparison.

## Components

- `src/cli/focus.rs`
  - Replace the parsed direction with a target enum:

    ```rust
    enum Target {
        Cycle(Direction),
        Notification,
    }
    ```

    `parse_args` returns `(Target, Scope)`. `Direction`, `select_target_pane`, and `eligible_pane_ids` are otherwise unchanged.
  - `cmd_focus` parses `--scope` once, then dispatches on `Target`. `Cycle` runs today's path verbatim.
  - Add the per-key stamp query, a `parse_stamp_lines(&str) -> Vec<(String, u64)>` helper, `select_last_notified_pane`, and the new message functions.
  - Update `usage()` to `focus <next|prev|notification> [--scope <all|session>]`.
- `src/desktop_notification.rs` — widen visibility so the stamp encoding stays owned by the module that writes it:
  - expose the three stamp option keys (via a public accessor over `DesktopNotificationKind`, not a duplicated literal list);
  - expose `stamp_timestamp(raw: &str) -> Option<u64>`, wrapping the existing private `parse_stamp`.

No changes to `src/tmux/query.rs`, the hook path, the adapters, or the TUI.

## Testing

Unit tests in `src/cli/focus.rs`'s existing `mod tests`, plus a small addition to the tests in `src/desktop_notification.rs`. No frames are rendered, so the project's inline-snapshot rule for UI tests does not apply.

- `parse_args` accepts `notification`; accepts `notification --scope session`; rejects `last`; rejects `notification` with an invalid scope.
- `select_last_notified_pane` picks the highest timestamp across all three notification kinds, including when one pane contributes several entries.
- `select_last_notified_pane` ignores stamps for panes absent from `eligible`.
- `select_last_notified_pane` honours session scope (a newer notification in another session loses to an older one in the active session).
- `select_last_notified_pane` returns `None` when no candidate carries a stamp.
- `select_last_notified_pane` resolves a timestamp tie in enumeration order.
- `select_last_notified_pane` returns the active pane when it is genuinely the most recent.
- `parse_stamp_lines` handles an unset option (empty value), a malformed timestamp, a fingerprint containing the `:` from the run-scoped prefix, and a line with no pane id.
- The new message functions return the expected scope-specific strings.
- `stamp_timestamp` handles a valid stamp, a malformed stamp, and an empty value.

## Documentation

- `README.md` — add `notification` to the focus command reference and its examples.
- `website/src/content/docs/reference/keybindings.md` — add a binding example.
- `website/src/content/docs/reference/scripting.md` — add the target to the command surface reference.

Keybinding examples use `@agent_sidebar_bin` rather than a bare binary name, consistent with commit 4d8cbce:

```tmux
bind-key M-l run-shell '"#{@agent_sidebar_bin}" focus notification'
bind-key M-L run-shell '"#{@agent_sidebar_bin}" focus notification --scope session'
```
