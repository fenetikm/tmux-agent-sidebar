# Session-name blocklist for automatic sidebar creation

**Date:** 2026-07-15
**Status:** Approved (design)

## Problem

`toggle-all` and the `after-new-window` hook create sidebar panes across every
session and window with no way to exclude specific ones. The motivating case is
tmux popups: the user names all popups with `_popup_` in the session name, yet a
sidebar still gets injected into them because:

1. **`toggle-all`** (`src/cli/toggle.rs:148-158`) enumerates panes with
   `list-panes -a`, which spans every session — including the transient session
   tmux creates for an open `display-popup` — and injects a sidebar into each
   window.
2. The global **`after-new-window` hook** (`agent-sidebar.conf:58-64`) fires
   `toggle --create-only` for popup-created windows too.

There is currently no popup detection and no session filtering anywhere in the
project.

## Goal

A general, user-configurable **blocklist of session-name glob patterns**.
Sessions whose name matches any pattern are excluded from *automatic* sidebar
creation. Popups are handled as an ordinary case (patterns like `*_popup_*`),
not as a special concept.

Non-goals (deliberately deferred, YAGNI):

- Allowlist ("only ever show on these sessions"). Blocklist alone covers the
  need; an allowlist can be added later if a real case appears.
- Window-name filtering. Session-name filtering is sufficient for the stated
  need.
- Regex or character-class globbing. `*` and `?` cover session names.

## Configuration

A new tmux option following the project's `@sidebar_*` convention, seeded in
`agent-sidebar.conf` alongside the other defaults:

```tmux
# space-separated glob patterns; sessions matching any pattern are excluded
# from automatic sidebar creation (toggle-all and the after-new-window hook).
set -g @sidebar_exclude_sessions ""
```

- **Default: empty** — no exclusions, zero behavior change for existing users.
- Patterns are whitespace-separated (the tmux idiom; session names do not
  contain spaces in practice).
- The README documents `*_popup_*` as the recommended value for a popup naming
  convention.

## Matching (pure, testable)

A small hand-rolled glob matcher supporting `*` and `?` only. No new dependency
(keeps the single distributed binary lean). Lives in a new module
`src/cli/session_filter.rs`:

- `fn glob_match(pattern: &str, text: &str) -> bool`
  - `*` matches any run of characters (including empty).
  - `?` matches exactly one character.
  - The match is anchored to the full string (whole session name).
- `fn session_excluded(session_name: &str, patterns: &[String]) -> bool`
  - true if `session_name` matches any pattern.
- `fn exclude_patterns() -> Vec<String>`
  - reads the option via `tmux show-options -gqv @sidebar_exclude_sessions`
    (using the existing tmux command runner), splits on whitespace.
  - unset/empty → empty vec → nothing excluded.

## Enforcement — two points, both delegating to `session_excluded`

### Path 1 — `toggle-all` (`src/cli/toggle.rs`, create branch)

The enumeration format gains `#{session_name}`:

```
list-panes -a -F "#{window_id}|#{session_name}|#{pane_current_path}"
```

Windows whose session matches the blocklist are filtered out *before* calling
`cmd_toggle`. No extra tmux calls — the session name is already in the listing.

The **toggle-off branch** (kill all sidebars) is **not** filtered: turning
everything off must kill every sidebar regardless of blocklist, including one
opened manually in a blocked session.

### Path 2 — `after-new-window` hook → `cmd_toggle`

Add a guard in `cmd_toggle` that fires **only when `--create-only` is set**. It
resolves the target window's session name
(`tmux display-message -t <window_id> -p '#{session_name}'`) and returns
silently if excluded.

### Why the `--create-only` gate implements the scope decision

The blocklist gates *automatic* creation only. This falls out of the
`--create-only` flag, which the automatic callers already pass and the manual
key does not:

| Trigger                 | Passes `--create-only`? | Gated? |
| ----------------------- | ----------------------- | ------ |
| `toggle-all` (`E`)       | yes                     | yes    |
| `after-new-window` hook | yes                     | yes    |
| manual `toggle` (`e`)    | no                      | no (escape hatch) |

## Testing

- Unit tests for `glob_match`: anchoring, `*`, `?`, e.g. `*_popup_*` matches
  `feat_popup_1` but not `mypopup`; `?` matches one char; `*` matches empty.
- Unit tests for `session_excluded`: multiple patterns, empty list, no match.
- tmux-interaction code (`exclude_patterns`, the `cmd_toggle` guard) stays thin
  and follows existing `toggle.rs` patterns; the pure functions carry the test
  coverage.

## Files touched

- **New:** `src/cli/session_filter.rs` — pure matcher plus option reader.
- `src/cli/toggle.rs` — filter in `cmd_toggle_all` create branch, guard in
  `cmd_toggle`, extended `list-panes` format.
- `src/cli/mod.rs` — register the new module.
- `agent-sidebar.conf` — seed the default option and a comment.
- README / docs — document `@sidebar_exclude_sessions`.
