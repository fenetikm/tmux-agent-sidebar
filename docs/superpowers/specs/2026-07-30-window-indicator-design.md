# Window Indicator Design

Date: 2026-07-30

## Problem

The sidebar lists every agent pane across every tmux session, grouped by
repository. Nothing in that list tells you which of those agents are sitting in
the *same tmux window as the sidebar you are looking at* — the ones a pane
switch away, rather than a window switch away. The only positional cue today is
the `┃` marker on the tmux-focused pane, which marks exactly one row.

## Solution

Every agent pane that lives in the sidebar's own tmux window gets a `┃` marker
in the existing left marker column, colored from a new `@sidebar_color_window`
option (default `Indexed(103)` — a dimmer shade of the accent hue).

## Behavior

- **Scope is the sidebar's own window**, not the tmux-active window. A sidebar
  rendering in a background window still marks its neighbors. Several sidebars
  across windows each mark a different set, which is the intent: the marker
  answers "which of these agents is next to me", independent of focus.
- **Focus wins.** The tmux-focused pane keeps its `accent` `┃`. Since the
  focused pane is usually in the sidebar's own window, the two states would
  otherwise collide; the more specific state keeps the stronger accent.
- **Rows: status row and branch/ports row only**, matching the existing focus
  marker. Task-progress, subagent, wait-reason, background-hint and prompt rows
  keep a blank marker column. This preserves the single rule for that column
  and avoids a multi-row bar that would compete with repo grouping.
- **No on/off option.** The color option is the only knob; setting
  `@sidebar_color_window` to the terminal background hides the marker. No
  existing marker has an on/off switch.

## Data plumbing

Two pieces of information are missing today.

### Per-pane window

Add `pub window_id: String` to `PaneInfo` (`src/tmux/types.rs`). The value is
already parsed in `build_session_hierarchy` (`src/tmux/query.rs`) as the
session-level `window_id` field, but is discarded when `group.rs` flattens the
session → window → pane hierarchy into repo groups. Assign it onto the pane
immediately after `parse_pane_fields_with_processes` returns, before
`window.panes.push(pane)`.

Cost: `PaneInfo` has no `Default` impl, so roughly 30 struct literals across 17
files — mostly one test fixture builder per file — need one extra field. The
change is mechanical and makes `PaneInfo` self-describing about its window.

### The sidebar's own window

Extend the format string in `get_sidebar_pane_info` (`src/tmux/panes.rs`) from
`#{pane_active} #{window_active} #{pane_width} #{pane_height}` to also carry
`#{window_id}`. That `display-message` call already runs on every refresh, so
this costs zero extra tmux forks and re-resolves each second — correct even if
the sidebar pane is moved between windows with `join-pane`.

The return type becomes a small named struct instead of growing to a 5-tuple.
`AppState::refresh` (`src/state/refresh.rs`) stores the value as
`sidebar_window_id: Option<String>` on `AppState`, using `None` when the field
comes back empty so an unresolvable window marks nothing rather than everything.

## Rendering

`row_collector::collect` (`src/ui/panes/row_collector.rs`) already computes
`is_active` per pane. Add alongside it:

```rust
let is_same_window = state
    .sidebar_window_id
    .as_deref()
    .is_some_and(|w| w == pane.window_id);
```

and pass it to `render_pane_lines_with_ports`.

In `src/ui/panes/row.rs` the marker decision becomes a three-way choice instead
of a boolean:

```rust
let marker_fg = if active {
    Some(theme.accent)
} else if same_window {
    Some(theme.window_marker)
} else {
    None
};
```

`marker_char` is `SELECTION_MARKER` when `marker_fg.is_some()`, otherwise a
space. `marker_style` applies the foreground plus the selection background
exactly as it does now, so a marked row under the sidebar cursor still picks up
`selection_bg`.

`RowCtx.active` keeps its current meaning — it feeds styling beyond the marker —
so the new flag drives the marker column only. No change to widths, padding, or
click targets: the marker column already exists and is always occupied by either
the glyph or a space.

## Config and theme

- `SIDEBAR_COLOR_WINDOW: &str = "@sidebar_color_window"` in
  `src/tmux/options.rs`, re-exported through `src/tmux.rs`.
- `window_marker: Color` on `ColorTheme` (`src/ui/colors.rs`), default
  `Color::Indexed(103)`, with a `read(...)` line in `from_options`.

The field is named `window_marker` rather than `window` because it describes the
marker, not a window background.

## Testing

- `src/tmux/query.rs` — pane parsing populates `window_id` from the session
  line, including through the grouped-session dedup path.
- `src/tmux/panes.rs` — parsing the extended `display-message` reply, plus a
  short or malformed reply falling back to no window id.
- `src/ui/panes/row.rs` — marker cases: same window and not focused (window
  color), focused and in the same window (accent wins), different window
  (blank).
- `src/ui/colors.rs` — `@sidebar_color_window` override is read into
  `window_marker`.
- `tests/ui_snapshot.rs` — one inline `insta` snapshot with three agents: one
  focused, one in the sidebar's window, one in another window. Per the project
  UI test rule, this is a snapshot assertion, not a substring check.

## Documentation

Add `@sidebar_color_window` to the "Core colors" table in
`website/src/content/docs/reference/tmux-options.md`, next to
`@sidebar_color_accent`, described as "Marker on agents in the sidebar's own
window". Add a `sidebar_window_id` row to the scope / update-frequency table in
`docs/state-management.md` (updated every 1s from the existing sidebar
`display-message` call). `README.md` carries no color table and needs no change.
