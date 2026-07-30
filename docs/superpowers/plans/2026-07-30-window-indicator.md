# Window Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mark every agent pane that lives in the sidebar's own tmux window with a `┃` marker colored from a new `@sidebar_color_window` option, so you can see at a glance which agents are one pane switch away.

**Architecture:** `PaneInfo` gains a `window_id` carried through from the existing `list-panes -a` query. The sidebar's own window id comes from the `display-message` call that already runs every refresh, and is stored on `AppState`. The row renderer turns its existing boolean marker decision into a three-way choice: accent for the tmux-focused pane, the new window color for other panes in the sidebar's window, blank otherwise.

**Tech Stack:** Rust edition 2024, Ratatui + Crossterm, `insta` inline snapshots, tmux CLI.

Spec: `docs/superpowers/specs/2026-07-30-window-indicator-design.md`

## Global Constraints

- Rust edition 2024 (`Cargo.toml`). No new dependencies.
- Run `cargo fmt` **before every commit** — CI runs `cargo fmt --check`.
- Every commit must pass `cargo test` and `cargo clippy`.
- Any test that renders a frame MUST use `insta::assert_snapshot!(output, @"...")` inline snapshots. Never `assert!(output.contains(...))` for rendered frames.
- All docs under `docs/` and `website/` are written in English.
- No ampersands (`&`) in headings — write "and".
- Default color for `@sidebar_color_window` is exactly `Color::Indexed(103)`.
- The marker glyph is the existing `SELECTION_MARKER` (`┃`) from `src/ui/panes/row/ctx.rs`; do not introduce a new glyph.
- `AppState::sidebar_window_id` defaults to `None`, and `None` must mark nothing. This is what keeps every existing UI snapshot unchanged.

## File Structure

| File | Responsibility | Change |
|------|----------------|--------|
| `src/tmux/types.rs` | `PaneInfo` definition | Add `window_id: String` |
| `src/tmux/query.rs` | Parse `list-panes -a` into the session tree | Populate `window_id` per pane |
| `src/tmux/panes.rs` | Sidebar pane `display-message` query | New `SidebarPaneInfo` struct + pure parser carrying `window_id` |
| `src/tmux.rs` | Public re-exports | Export `SidebarPaneInfo`, `SIDEBAR_COLOR_WINDOW` |
| `src/tmux/options.rs` | tmux option name constants | Add `SIDEBAR_COLOR_WINDOW` |
| `src/state.rs` | `AppState` fields | Add `sidebar_window_id: Option<String>` |
| `src/state/refresh.rs` | Per-second refresh | Store the sidebar's window id |
| `src/ui/colors.rs` | Theme | Add `window_marker: Color` |
| `src/ui/panes/row.rs` | Pane row rendering | Three-way marker choice |
| `src/ui/panes/row_collector.rs` | Per-pane row assembly | Compute `is_same_window` |
| `tests/color_tests.rs` | Styled render snapshots | New marker-color snapshot |
| `website/src/content/docs/reference/tmux-options.md` | Option reference | Document the option |
| `docs/state-management.md` | State table | Document the new field |

Tasks 1–3 are independent of each other. Task 4 consumes all three. Task 5 documents the result.

---

### Task 1: Carry `window_id` on every `PaneInfo`

**Files:**
- Modify: `src/tmux/types.rs` (the `PaneInfo` struct, around line 6-33)
- Modify: `src/tmux/query.rs` (the `PaneInfo` literal in `parse_pane_fields_with_processes`, around line 320; the pane push in `build_session_hierarchy`, around line 185)
- Modify: every other file containing a `PaneInfo { ... }` literal — the compiler lists them
- Test: `src/tmux/query.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: `PaneInfo.window_id: String` — the tmux window id (e.g. `"@3"`) the pane lives in, or `""` when unknown. Task 4 compares it against `AppState::sidebar_window_id`.

- [ ] **Step 1: Write the failing test**

In `src/tmux/query.rs`, find the existing test helper `make_full_pane_line` (in the "build_session_hierarchy dedup" test section). Add a window-parameterized variant next to it and make the existing helper delegate, so both spellings stay in sync:

```rust
    /// Construct a minimal valid pane line for `build_session_hierarchy`
    /// with the given session name and pane_pid. All other fields are
    /// empty/defaults — enough to survive parsing as an opencode pane.
    fn make_full_pane_line(session_name: &str, pane_pid: u32) -> String {
        make_full_pane_line_in_window(session_name, pane_pid, "@0")
    }

    /// Same as `make_full_pane_line`, but places the pane in an explicit
    /// tmux window so window-scoped behaviour can be exercised.
    fn make_full_pane_line_in_window(
        session_name: &str,
        pane_pid: u32,
        window_id: &str,
    ) -> String {
```

Keep the existing field-layout comment and body of `make_full_pane_line` inside `make_full_pane_line_in_window`, changing only the `window_id` assignment:

```rust
        fields[1] = window_id; // window_id
```

Then add the test:

```rust
    #[test]
    fn build_session_hierarchy_assigns_window_id_to_each_pane() {
        let line_a = make_full_pane_line_in_window("primary", 41, "@7");
        let line_b = make_full_pane_line_in_window("primary", 42, "@9");

        let input = format!("{line_a}\n{line_b}");
        let (sessions_map, _) = build_session_hierarchy(&input, None);
        let sessions = finalize_sessions(sessions_map);

        let mut seen: Vec<(String, String)> = sessions[0]
            .windows
            .iter()
            .flat_map(|w| {
                w.panes
                    .iter()
                    .map(|p| (w.window_id.clone(), p.window_id.clone()))
            })
            .collect();
        seen.sort();

        // Each pane carries the id of the window it was parsed under.
        assert_eq!(
            seen,
            vec![
                ("@7".to_string(), "@7".to_string()),
                ("@9".to_string(), "@9".to_string()),
            ]
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib build_session_hierarchy_assigns_window_id`
Expected: FAIL to compile with `no field 'window_id' on type '&PaneInfo'`.

- [ ] **Step 3: Add the field to `PaneInfo`**

In `src/tmux/types.rs`, add after `pub session_name: String,`:

```rust
    /// tmux window this pane lives in (e.g. `@3`). Populated from the
    /// session-level `window_id` field of the `list-panes -a` query, which
    /// would otherwise be lost when `group.rs` flattens the
    /// session → window → pane tree into repo groups. Empty when unknown.
    pub window_id: String,
```

- [ ] **Step 4: Populate it in the query layer**

In `src/tmux/query.rs`, in the `PaneInfo { ... }` literal returned by `parse_pane_fields_with_processes`, add alongside `session_name: String::new(),`:

```rust
        window_id: String::new(),
```

The window id is not part of the pane fields, so it is filled in by the caller. In `build_session_hierarchy`, change the push block to:

```rust
        if let Some(mut pane) = parse_pane_fields_with_processes(pane_fields, process_snapshot) {
            pane.window_id = window_id.to_string();
            if pane.agent == AgentType::Codex
                && let Some(pid) = pane.pane_pid
            {
                codex_pids.push((window_id.to_string(), window.panes.len(), pid));
            }
            window.panes.push(pane);
        }
```

- [ ] **Step 5: Fix every remaining `PaneInfo` literal**

Run: `cargo build 2>&1 | grep -A2 "missing field"`

For each reported literal (test fixtures in `src/ui/panes/*.rs`, `src/cli/focus.rs`, `src/state/*.rs`, `src/state.rs`, `src/group.rs`, `src/tmux/types.rs`, `tests/state_tests.rs`, `tests/ui_snapshot.rs`), add:

```rust
            window_id: String::new(),
```

Two exceptions, where a real id is more useful than an empty one:

- `tests/test_helpers.rs`, in `make_pane`, use `window_id: "@1".into(),`
- `src/tmux/types.rs`, if it has a fixture literal, use `window_id: String::new(),`

Repeat `cargo build` until it is clean. Since `AppState::sidebar_window_id` does not exist yet, no rendering changes — existing snapshots stay green.

- [ ] **Step 6: Run the full suite**

Run: `cargo test`
Expected: PASS, including the new `build_session_hierarchy_assigns_window_id_to_each_pane`.

- [ ] **Step 7: Lint, format, commit**

```bash
cargo clippy --all-targets
cargo fmt
git add -A
git commit -m "feat(tmux): carry window_id on PaneInfo"
```

---

### Task 2: Resolve the sidebar's own window id

**Files:**
- Modify: `src/tmux/panes.rs:3-19` (`get_sidebar_pane_info`)
- Modify: `src/tmux.rs:35` (re-export list)
- Modify: `src/state.rs` (`AppState` struct and `AppState::new`)
- Modify: `src/state/refresh.rs:149` (the only `get_sidebar_pane_info` call site)
- Test: `src/tmux/panes.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `tmux::SidebarPaneInfo { pane_active: bool, window_active: bool, width: u16, height: u16, window_id: Option<String> }`
  - `tmux::get_sidebar_pane_info(tmux_pane: &str) -> SidebarPaneInfo` (was a `(bool, bool, u16, u16)` tuple)
  - `AppState::sidebar_window_id: Option<String>` — Task 4 reads this.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block at the bottom of `src/tmux/panes.rs`:

```rust
    #[test]
    fn parse_sidebar_pane_info_reads_all_fields() {
        let info = parse_sidebar_pane_info("1 1 28 40 @7");
        assert_eq!(
            info,
            SidebarPaneInfo {
                pane_active: true,
                window_active: true,
                width: 28,
                height: 40,
                window_id: Some("@7".into()),
            }
        );
    }

    #[test]
    fn parse_sidebar_pane_info_handles_inactive_flags() {
        let info = parse_sidebar_pane_info("0 0 30 12 @1");
        assert!(!info.pane_active);
        assert!(!info.window_active);
        assert_eq!(info.width, 30);
        assert_eq!(info.height, 12);
    }

    #[test]
    fn parse_sidebar_pane_info_falls_back_on_short_reply() {
        // A truncated reply (e.g. tmux returned nothing useful) must not
        // panic and must leave the window unresolved, so the caller marks
        // no panes rather than every pane.
        let info = parse_sidebar_pane_info("1 1");
        assert!(info.pane_active);
        assert!(info.window_active);
        assert_eq!(info.width, DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(info.height, DEFAULT_SIDEBAR_HEIGHT);
        assert_eq!(info.window_id, None);
    }

    #[test]
    fn parse_sidebar_pane_info_treats_empty_reply_as_unresolved() {
        let info = parse_sidebar_pane_info("");
        assert!(!info.pane_active);
        assert!(!info.window_active);
        assert_eq!(info.width, DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(info.height, DEFAULT_SIDEBAR_HEIGHT);
        assert_eq!(info.window_id, None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib parse_sidebar_pane_info`
Expected: FAIL to compile — `cannot find function 'parse_sidebar_pane_info'`.

- [ ] **Step 3: Replace the tuple with a struct**

In `src/tmux/panes.rs`, replace the whole existing `get_sidebar_pane_info` function with:

```rust
/// Fallback sidebar width when tmux gives no usable answer.
pub(crate) const DEFAULT_SIDEBAR_WIDTH: u16 = 28;
/// Fallback sidebar height when tmux gives no usable answer.
pub(crate) const DEFAULT_SIDEBAR_HEIGHT: u16 = 24;

/// Placement and geometry of the sidebar's own pane, read in a single
/// `display-message` call per refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarPaneInfo {
    /// Whether the sidebar pane itself holds tmux focus.
    pub pane_active: bool,
    /// Whether the window the sidebar lives in is the active one.
    pub window_active: bool,
    pub width: u16,
    pub height: u16,
    /// tmux window the sidebar pane lives in (e.g. `@3`). `None` when tmux
    /// returned no value — callers must treat that as "no window known"
    /// rather than matching every pane.
    pub window_id: Option<String>,
}

pub fn get_sidebar_pane_info(tmux_pane: &str) -> SidebarPaneInfo {
    let out = display_message(
        tmux_pane,
        "#{pane_active} #{window_active} #{pane_width} #{pane_height} #{window_id}",
    );
    parse_sidebar_pane_info(&out)
}

/// Pure parser for the `display-message` reply. Each field falls back
/// independently so a truncated reply still yields usable geometry.
pub(crate) fn parse_sidebar_pane_info(out: &str) -> SidebarPaneInfo {
    let parts: Vec<&str> = out.split_whitespace().collect();
    let field = |i: usize| parts.get(i).copied().unwrap_or_default();
    SidebarPaneInfo {
        pane_active: field(0) == "1",
        window_active: field(1) == "1",
        width: field(2).parse().unwrap_or(DEFAULT_SIDEBAR_WIDTH),
        height: field(3).parse().unwrap_or(DEFAULT_SIDEBAR_HEIGHT),
        window_id: Some(field(4).to_string()).filter(|s| !s.is_empty()),
    }
}
```

- [ ] **Step 4: Export the struct**

In `src/tmux.rs`, add `SidebarPaneInfo` to the `pub use` list that already carries `get_sidebar_pane_info` (line 35), keeping alphabetical order within that group.

- [ ] **Step 5: Add the `AppState` field**

In `src/state.rs`, add to the `AppState` struct after `pub tmux_pane: String,`:

```rust
    /// tmux window the sidebar pane itself lives in, refreshed every 1s.
    /// `None` when tmux did not report one — the window marker then applies
    /// to no pane at all.
    pub sidebar_window_id: Option<String>,
```

and in `AppState::new`, after `tmux_pane,`:

```rust
            sidebar_window_id: None,
```

- [ ] **Step 6: Update the refresh call site**

In `src/state/refresh.rs`, replace line 149 and thread the struct through the rest of `refresh`:

```rust
        let sidebar = tmux::get_sidebar_pane_info(&self.tmux_pane);
        self.sidebar_window_id = sidebar.window_id.clone();
        let focused = sidebar.pane_active;
```

Leave the rest of the function body untouched (it already uses `focused`), and change the final `window_active` return expression to `sidebar.window_active`.

- [ ] **Step 7: Run the tests**

Run: `cargo test`
Expected: PASS. The four new `parse_sidebar_pane_info` tests pass; nothing else changes behavior because no renderer reads `sidebar_window_id` yet.

- [ ] **Step 8: Lint, format, commit**

```bash
cargo clippy --all-targets
cargo fmt
git add -A
git commit -m "feat(state): track the sidebar pane's own tmux window"
```

---

### Task 3: Add the `@sidebar_color_window` theme color

**Files:**
- Modify: `src/tmux/options.rs` (near `SIDEBAR_COLOR_SELECTION`, line 125)
- Modify: `src/tmux.rs:17-26` (option re-export list)
- Modify: `src/ui/colors.rs` (struct field, `Default`, `from_options`)
- Test: `src/ui/colors.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: `ColorTheme::window_marker: Color`, default `Color::Indexed(103)`, overridable via the `@sidebar_color_window` tmux option. Task 4 reads it.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/ui/colors.rs`:

```rust
    #[test]
    fn window_marker_defaults_to_dim_accent_shade() {
        assert_eq!(ColorTheme::default().window_marker, Color::Indexed(103));
    }

    #[test]
    fn from_options_reads_window_marker_override() {
        let mut options = std::collections::HashMap::new();
        options.insert(tmux::SIDEBAR_COLOR_WINDOW.to_string(), "60".to_string());

        let theme = ColorTheme::from_options(&options);

        assert_eq!(theme.window_marker, Color::Indexed(60));
    }

    #[test]
    fn from_options_window_marker_falls_back_when_invalid() {
        let mut options = std::collections::HashMap::new();
        options.insert(
            tmux::SIDEBAR_COLOR_WINDOW.to_string(),
            "nope".to_string(),
        );

        let theme = ColorTheme::from_options(&options);

        assert_eq!(theme.window_marker, ColorTheme::default().window_marker);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib window_marker`
Expected: FAIL to compile — no field `window_marker`, no constant `SIDEBAR_COLOR_WINDOW`.

- [ ] **Step 3: Add the option constant**

In `src/tmux/options.rs`, next to `SIDEBAR_COLOR_SELECTION`:

```rust
pub const SIDEBAR_COLOR_WINDOW: &str = "@sidebar_color_window";
```

In `src/tmux.rs`, add `SIDEBAR_COLOR_WINDOW` to the same `pub use` group as the other `SIDEBAR_COLOR_*` constants, in alphabetical order.

- [ ] **Step 4: Add the theme field**

In `src/ui/colors.rs`, add to the `ColorTheme` struct after `pub selection_bg: Color,`:

```rust
    /// Marker on agent panes that share the sidebar's own tmux window but
    /// do not hold focus. A dimmer companion to `accent`; the two are
    /// configured independently, so retheming `accent` does not shift it.
    pub window_marker: Color,
```

In `impl Default`, after `selection_bg: Color::Indexed(239),`:

```rust
            window_marker: Color::Indexed(103),
```

In `from_options`, after the `selection_bg` line:

```rust
        theme.window_marker = read(tmux::SIDEBAR_COLOR_WINDOW, theme.window_marker);
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib window_marker`
Expected: PASS (3 tests).

- [ ] **Step 6: Lint, format, commit**

```bash
cargo clippy --all-targets
cargo fmt
git add -A
git commit -m "feat(ui): add the @sidebar_color_window theme color"
```

---

### Task 4: Render the window marker

**Files:**
- Modify: `src/ui/panes/row.rs:24-72` (`render_pane_lines_with_ports` signature and marker decision)
- Modify: `src/ui/panes/row_collector.rs:99-118` (compute and pass `is_same_window`)
- Test: `src/ui/panes/row.rs` (inline `mod tests`)
- Test: `tests/color_tests.rs` (new styled snapshot)

**Interfaces:**
- Consumes: `PaneInfo.window_id` (Task 1), `AppState::sidebar_window_id` (Task 2), `ColorTheme::window_marker` (Task 3).
- Produces: `render_pane_lines_with_ports(pane, git_info, ports, task_progress, selected, active, same_window, width, icons, theme, spinner_frame, now)` — the new `same_window: bool` sits immediately after `active: bool`.

- [ ] **Step 1: Write the failing unit tests**

Add to `mod tests` in `src/ui/panes/row.rs`, next to the existing `render_pane_lines_active_shows_left_marker_on_status_row`:

```rust
    #[test]
    fn render_pane_lines_same_window_uses_window_marker_color() {
        let theme = ColorTheme::default();
        let pane = pane(PermissionMode::Default, PaneStatus::Running, "");
        let lines = render_pane_lines_with_ports(
            &pane,
            &PaneGitInfo::default(),
            None,
            None,
            false,
            false, // active
            true,  // same_window
            40,
            &StatusIcons::default(),
            &theme,
            0,
            0,
        );

        let marker_span = &lines[0].spans[0];
        assert_eq!(marker_span.content, SELECTION_MARKER);
        assert_eq!(marker_span.style.fg, Some(theme.window_marker));
    }

    #[test]
    fn render_pane_lines_active_wins_over_same_window() {
        let theme = ColorTheme::default();
        let pane = pane(PermissionMode::Default, PaneStatus::Running, "");
        let lines = render_pane_lines_with_ports(
            &pane,
            &PaneGitInfo::default(),
            None,
            None,
            false,
            true, // active
            true, // same_window
            40,
            &StatusIcons::default(),
            &theme,
            0,
            0,
        );

        let marker_span = &lines[0].spans[0];
        assert_eq!(marker_span.content, SELECTION_MARKER);
        assert_eq!(marker_span.style.fg, Some(theme.accent));
    }

    #[test]
    fn render_pane_lines_other_window_has_blank_marker() {
        let theme = ColorTheme::default();
        let pane = pane(PermissionMode::Default, PaneStatus::Running, "");
        let lines = render_pane_lines_with_ports(
            &pane,
            &PaneGitInfo::default(),
            None,
            None,
            false,
            false, // active
            false, // same_window
            40,
            &StatusIcons::default(),
            &theme,
            0,
            0,
        );

        assert_eq!(lines[0].spans[0].content, " ");
        assert_eq!(lines[0].spans[0].style.fg, None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib render_pane_lines`
Expected: FAIL to compile — `this function takes 11 arguments but 12 arguments were supplied`.

- [ ] **Step 3: Implement the three-way marker choice**

In `src/ui/panes/row.rs`, add the parameter after `active: bool,`:

```rust
    same_window: bool,
```

Replace the marker comment and `marker_ctx` construction with:

```rust
    // The left marker `┃` answers "where am I": accent for the pane that
    // currently holds tmux focus, the dimmer window color for other agents
    // sharing the sidebar's own window, blank for agents in other windows.
    // To keep the accent compact it only appears on the status row and the
    // branch/ports row (when present) — never on deeper details like task
    // progress or prompt wrapping. The sidebar cursor position (`selected`)
    // still paints the full pane with the selection background.
    let marker_fg = if active {
        Some(theme.accent)
    } else if same_window {
        Some(theme.window_marker)
    } else {
        None
    };
    let marker_ctx = RowCtx {
        marker_char: if marker_fg.is_some() {
            SELECTION_MARKER
        } else {
            " "
        },
        marker_style: match marker_fg {
            Some(fg) => apply_bg(Style::default().fg(fg)),
            None => apply_bg(Style::default()),
        },
        inner_width: width.saturating_sub(2),
        theme,
        bg,
        active,
    };
```

Leave `plain_ctx` and everything below unchanged — `RowCtx.active` keeps its existing meaning and still drives non-marker styling.

- [ ] **Step 4: Update the remaining call sites**

Run: `cargo build --all-targets 2>&1 | grep -B2 "arguments"`

Pass `false` for `same_window` in every existing `render_pane_lines_with_ports` test call in `src/ui/panes/row.rs` (the three new tests already pass an explicit value). Then update the one production call site in `src/ui/panes/row_collector.rs`: add, right below the existing `let is_active = ...` line,

```rust
            let is_same_window = state
                .sidebar_window_id
                .as_deref()
                .is_some_and(|w| w == pane.window_id);
```

and pass `is_same_window` after `is_active` in the `render_pane_lines_with_ports(...)` call.

- [ ] **Step 5: Run the unit tests**

Run: `cargo test --lib render_pane_lines`
Expected: PASS (all four marker tests, including the pre-existing active one).

- [ ] **Step 6: Confirm existing snapshots still pass**

Run: `cargo test`
Expected: PASS with no snapshot diffs. `AppState::sidebar_window_id` is `None` in every existing test, so `is_same_window` is always `false` and no rendered output moves. If a snapshot does fail here, stop — it means `sidebar_window_id` is being populated somewhere it should not be.

- [ ] **Step 7: Write the styled snapshot test**

Add to the end of `tests/color_tests.rs`:

```rust
// Three agents in one repo group: `%1` holds tmux focus, `%2` shares the
// sidebar's window (`@1`), `%3` lives in another window (`@2`). Verifies the
// marker column paints accent 153, window 103, and blank respectively.
#[test]
fn window_marker_colors_distinguish_focus_window_and_elsewhere() {
    let mut focused = make_pane(AgentType::Claude, PaneStatus::Idle);
    focused.pane_id = "%1".into();
    focused.window_id = "@1".into();

    let mut neighbor = make_pane(AgentType::Codex, PaneStatus::Idle);
    neighbor.pane_id = "%2".into();
    neighbor.window_id = "@1".into();

    let mut elsewhere = make_pane(AgentType::Claude, PaneStatus::Idle);
    elsewhere.pane_id = "%3".into();
    elsewhere.window_id = "@2".into();

    let mut state = make_state(vec![]);
    // Keep the sidebar cursor out of the picture so the snapshot shows the
    // marker colors alone, with no selection background.
    state.focus_state.sidebar_focused = false;
    state.focus_state.focused_pane_id = Some("%1".into());
    state.sidebar_window_id = Some("@1".into());
    state.repo_groups = vec![make_repo_group(
        "project",
        vec![focused, neighbor, elsewhere],
    )];
    state.rebuild_row_targets();

    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"");
}
```

Check the imports at the top of `tests/color_tests.rs` and add anything missing (`AgentType`, `PaneStatus`, and the `test_helpers` items are already used by that file).

- [ ] **Step 8: Generate and inspect the snapshot**

```bash
cargo test --test color_tests window_marker_colors -- --nocapture
cargo insta accept
```

Then read the accepted inline snapshot and confirm, before committing:
- the `claude` row for `%1` starts with `┃[fg:153]`
- the `codex` row for `%2` starts with `┃[fg:103]`
- the third row starts with a space, not `┃`

If any of those is wrong, the implementation is wrong — fix it rather than accepting the snapshot.

- [ ] **Step 9: Full verification**

```bash
cargo test
cargo clippy --all-targets
```
Expected: PASS, no warnings.

- [ ] **Step 10: Format and commit**

```bash
cargo fmt
git add -A
git commit -m "feat(ui): mark agents in the sidebar's own window"
```

---

### Task 5: Document the option and the state field

**Files:**
- Modify: `website/src/content/docs/reference/tmux-options.md:44-51` ("Structural colors" table)
- Modify: `docs/state-management.md:73-99` ("Local State" table)

**Interfaces:**
- Consumes: the option name and default from Task 3, the field name from Task 2.
- Produces: nothing consumed by code.

- [ ] **Step 1: Document the tmux option**

In the "Structural colors" table of `website/src/content/docs/reference/tmux-options.md`, add a row directly below `@sidebar_color_selection`:

```markdown
| `@sidebar_color_window`    | `103`&nbsp;(dim slate blue) | Marker on agents sharing the sidebar's own window (the focused pane keeps `@sidebar_color_accent`) |
```

Keep the column pipes aligned with the surrounding rows.

- [ ] **Step 2: Document the state field**

In the "Local State (single sidebar process only)" table of `docs/state-management.md`, add a row after the `focus_state.prev_focused_pane_id` row:

```markdown
| `sidebar_window_id` | Every 1s | tmux window the sidebar pane itself lives in; drives the `@sidebar_color_window` marker on agents in that window. `None` marks nothing |
```

- [ ] **Step 3: Verify the docs build and the tables render**

Run: `git diff --stat`
Expected: exactly the two files above, one line added to each. Skim both tables to confirm no column got misaligned.

- [ ] **Step 4: Commit**

```bash
git add website/src/content/docs/reference/tmux-options.md docs/state-management.md
git commit -m "docs: document the current-window agent marker"
```

---

### Task 6: Verify against a live tmux session

**Files:** none.

**Interfaces:** consumes the finished feature.

- [ ] **Step 1: Build the release binary**

Run: `cargo build --release`

The plugin directory `~/.tmux/plugins/tmux-agent-sidebar` is usually a symlink to this repo, so the binary is picked up directly. If this work happens in a git worktree, copy and re-sign instead:

```bash
cp "$PWD/target/release/tmux-agent-sidebar" ~/.tmux/plugins/tmux-agent-sidebar/target/release/tmux-agent-sidebar
codesign --force --sign - ~/.tmux/plugins/tmux-agent-sidebar/target/release/tmux-agent-sidebar
```

Skipping `codesign` after a worktree copy causes tmux to report `terminated by signal 9`.

- [ ] **Step 2: Restart the sidebar and check the marker**

Toggle the sidebar off and on via the tmux keybinding, then confirm by eye:
- the focused agent keeps its pale blue `┃`
- other agents in the same window show a dimmer `┃`
- agents in other windows and sessions show none
- `tmux set -g @sidebar_color_window 203` plus a sidebar restart changes the dim marker's color

- [ ] **Step 3: Report the result**

State what was observed for each of the four checks above. Do not claim the feature works without having run it.

---

## Self-Review Notes

Spec coverage: own-window scope (Tasks 1, 2, 4), focus precedence (Task 4 Step 3), status and branch rows only (Task 4 Step 3 leaves `plain_ctx` untouched), no on/off option (nothing added), `Indexed(103)` default and independent configurability (Task 3), `PaneInfo.window_id` plumbing (Task 1), `display-message` extension with per-field fallback (Task 2), all five test groups from the spec (Tasks 1–4), both doc updates (Task 5).

One deviation from the spec, deliberate: the rendered-frame snapshot lives in `tests/color_tests.rs` rather than `tests/ui_snapshot.rs`, because marker colors need `render_to_styled_string` and every styled snapshot in this repo lives in `color_tests.rs`.
