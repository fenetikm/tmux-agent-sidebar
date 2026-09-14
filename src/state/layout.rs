use super::{AppState, RepoFilter, StatusFilter};
use crate::ui::text::display_width;

#[derive(Debug, Clone)]
pub struct RowTarget {
    pub pane_id: String,
}

/// Click target for the `+` button rendered at the right edge of each
/// repo-group header in the agents panel. Clicking it opens the spawn
/// modal prefilled for that repo.
#[derive(Debug, Clone)]
pub struct RepoSpawnTarget {
    pub rect: ratatui::layout::Rect,
    pub repo_name: String,
    pub repo_root: String,
    /// Session the target's group belongs to, so the keyboard spawn flow can
    /// anchor under the right header when one repo appears under two sessions.
    /// `None` in repository grouping.
    pub session: Option<String>,
}

/// Click target for the red `×` rendered next to the branch of a
/// sidebar-spawned pane. Clicking it opens the close-pane confirmation
/// for that specific pane.
#[derive(Debug, Clone)]
pub struct SpawnRemoveTarget {
    pub rect: ratatui::layout::Rect,
    pub pane_id: String,
}

/// Click target for the header of a tmux session that holds no agents,
/// rendered when `@sidebar_show_empty_sessions` is on. Clicking it switches
/// the attached client to that session. Sessions that do have agents are
/// reached by clicking one of their panes instead.
#[derive(Debug, Clone)]
pub struct SessionJumpTarget {
    pub rect: ratatui::layout::Rect,
    pub session: String,
}

/// Screen-positioned hyperlink overlay for OSC 8 terminal hyperlinks.
#[derive(Debug, Clone)]
pub struct HyperlinkOverlay {
    pub x: u16,
    pub y: u16,
    pub text: String,
    pub url: String,
    /// Style the text was rendered with. Writing the OSC 8 overlay reprints
    /// these cells outside ratatui's buffer, so without carrying the style
    /// along the reprint lands in whatever SGR state the terminal was left
    /// in — every linked row ends up the same colour.
    pub style: ratatui::style::Style,
}

/// Click region for one bottom-panel tab title. Computed during render
/// because the panel tab's title is user-supplied, so the column ranges
/// cannot be known at compile time.
#[derive(Debug, Clone, PartialEq)]
pub struct BottomTabTarget {
    /// First column of the title text, absolute within the frame.
    pub start: u16,
    /// One past the last column of the title text.
    pub end: u16,
    pub tab: crate::state::BottomTab,
}

/// Ephemeral render output cached for click hit-testing.
///
/// Every field here is **rewritten on every frame** by the UI layer and
/// only read by event handlers (mouse/keyboard) before the next render.
/// Bundling them under `state.layout` makes the "frame-scoped vs
/// persistent state" boundary visible at a glance, since the rest of
/// `AppState` only holds data that survives across frames.
#[derive(Debug, Clone, Default)]
pub struct FrameLayout {
    /// Filtered pane list, in the order the UI rendered them. Index
    /// matches `GlobalState::selected_pane_row`.
    pub pane_row_targets: Vec<RowTarget>,
    /// Maps each rendered text line in the agents panel back to a row in
    /// `pane_row_targets`. `None` for header/blank lines that should not
    /// route clicks to a pane.
    pub line_to_row: Vec<Option<usize>>,
    /// X column of the repo filter button in the secondary header. `None`
    /// when the button is hidden. Used for click hit-testing.
    pub repo_button_col: Option<u16>,
    /// Click regions for the `[+]` spawn button rendered at the right
    /// edge of each repo-group header. One entry per visible repo group.
    pub repo_spawn_targets: Vec<RepoSpawnTarget>,
    /// Click regions for the red `×` remove marker rendered next to the
    /// branch of each sidebar-spawned pane. One entry per visible row.
    pub spawn_remove_targets: Vec<SpawnRemoveTarget>,
    /// Click regions for agent-less session headers. Empty unless
    /// `@sidebar_show_empty_sessions` is on and the list is grouped by
    /// session.
    pub session_jump_targets: Vec<SessionJumpTarget>,
    /// OSC 8 hyperlink overlays the main loop writes after each frame so
    /// terminals can recognise PR numbers as clickable links.
    pub hyperlink_overlays: Vec<HyperlinkOverlay>,
    /// Absolute frame Y of the agents panel top edge. Set every frame by
    /// `ui::draw` so mouse hit-testing can map absolute clicks to the
    /// panel-relative rows used by the filter bar and pane list.
    pub agents_area_y: u16,
    /// Click regions for the bottom panel's tab titles, rebuilt each frame
    /// by `ui::bottom::draw_bottom`.
    pub bottom_tab_targets: Vec<BottomTabTarget>,
}

pub(super) fn point_in_rect(row: u16, col: u16, rect: ratatui::layout::Rect) -> bool {
    rect.contains(ratatui::layout::Position { x: col, y: row })
}

impl AppState {
    pub fn rebuild_row_targets(&mut self) {
        // Reset stale repo filter if the repo no longer exists, and
        // persist the reset back to tmux so fresh sidebar instances do
        // not reload the dead repo name on startup.
        if let RepoFilter::Repo(ref name) = self.global.repo_filter
            && !self.repo_groups.iter().any(|g| g.name == *name)
        {
            self.global.repo_filter = RepoFilter::All;
            self.global.save_repo_filter();
        }

        self.layout.pane_row_targets.clear();
        for pane_id in crate::group::visible_pane_ids(
            &self.repo_groups,
            self.effective_status_filter(),
            &self.effective_repo_filter(),
        ) {
            self.layout.pane_row_targets.push(RowTarget { pane_id });
        }
        if self.global.selected_pane_row >= self.layout.pane_row_targets.len()
            && !self.layout.pane_row_targets.is_empty()
        {
            self.global.selected_pane_row = self.layout.pane_row_targets.len() - 1;
        }
    }

    /// Flip compact rows and persist the choice to `@sidebar_compact`.
    ///
    /// Persistence is write-on-toggle and read-at-startup only — there is
    /// no sync-back from tmux, so a failed write costs the setting on the
    /// next launch and nothing more. That is why this needs none of the
    /// `last_saved_*` bookkeeping `GlobalState::save_filter` carries.
    ///
    /// Row heights change, so the click hit-test targets must be rebuilt.
    pub fn toggle_compact_rows(&mut self) {
        self.compact_rows = !self.compact_rows;
        let value = if self.compact_rows { "on" } else { "off" };
        crate::tmux::run_tmux(&["set", "-g", crate::tmux::SIDEBAR_COMPACT, value]);
        self.rebuild_row_targets();
    }

    /// Hide or show the status filter bar and persist to
    /// `@sidebar_hide_filter_bar`. While hidden the list always uses the
    /// All filter and status-filter keybindings are ignored.
    pub fn toggle_hide_filter_bar(&mut self) {
        self.hide_filter_bar = !self.hide_filter_bar;
        let value = if self.hide_filter_bar { "on" } else { "off" };
        crate::tmux::run_tmux(&["set", "-g", crate::tmux::SIDEBAR_HIDE_FILTER_BAR, value]);
        if self.hide_filter_bar && self.focus_state.focus == crate::state::Focus::Filter {
            self.focus_state.focus = crate::state::Focus::Panes;
        }
        self.rebuild_row_targets();
    }

    /// Handle mouse scroll event, routing to agents or bottom panel by Y position.
    pub fn handle_mouse_scroll(
        &mut self,
        row: u16,
        term_height: u16,
        bottom_panel_height: u16,
        delta: isize,
    ) {
        let bottom_start = term_height.saturating_sub(bottom_panel_height);
        if row >= bottom_start {
            self.scroll_bottom(delta);
        } else {
            self.scrolls.panes.scroll(delta);
        }
    }

    /// Handle mouse click on the filter bar (row 0).
    /// Determines which filter was clicked based on x coordinate.
    /// Debounces rapid clicks to ignore phantom mouse events from tmux
    /// pane resize/layout changes.
    pub fn handle_filter_click(&mut self, col: u16) {
        if self.hide_filter_bar {
            return;
        }
        const DEBOUNCE_MS: u128 = 150;
        let now = std::time::Instant::now();
        if now
            .duration_since(self.timers.last_filter_click)
            .as_millis()
            < DEBOUNCE_MS
        {
            return;
        }
        self.timers.last_filter_click = now;

        let (all, running, background, waiting, idle, error) = self.status_counts();
        // Layout: " ∑N  ●N  ◎N  ◐N  ○N  ✕N"
        // Each filter item renders as `icon(1) + count`, so the clickable
        // width is `1 + digits(count)`.
        let mut x = 1usize; // leading space
        let items: Vec<(StatusFilter, usize)> = vec![
            (StatusFilter::All, 1 + format!("{all}").len()),
            (StatusFilter::Running, 1 + format!("{running}").len()),
            (StatusFilter::Background, 1 + format!("{background}").len()),
            (StatusFilter::Waiting, 1 + format!("{waiting}").len()),
            (StatusFilter::Idle, 1 + format!("{idle}").len()),
            (StatusFilter::Error, 1 + format!("{error}").len()),
        ];
        let col = col as usize;
        for (i, (filter, width)) in items.iter().enumerate() {
            if i > 0 {
                x += 2; // "  " separator
            }
            if col >= x && col < x + width {
                self.global.status_filter = *filter;
                self.global.save_filter();
                self.rebuild_row_targets();
                return;
            }
            x += width;
        }
    }

    /// Handle mouse click on the secondary header row (row 1).
    /// The repo filter button lives on the far right of this row.
    pub fn handle_secondary_header_click(&mut self, col: u16) {
        if self
            .notices
            .button_col
            .is_some_and(|notices_col| col == notices_col)
        {
            self.toggle_notices_popup();
            return;
        }
        if self.hide_repo_filter {
            return;
        }
        if self
            .layout
            .repo_button_col
            .is_some_and(|repo_button_col| col >= repo_button_col)
        {
            self.toggle_repo_popup();
        }
    }

    /// Handle mouse click in agents panel. `row`/`col` are absolute frame
    /// coordinates. Filter bar and pane-list routing use panel-relative
    /// rows derived from [`FrameLayout::agents_area_y`]; popup and spawn/
    /// remove targets use the absolute coordinates stored at render time.
    pub fn handle_mouse_click(&mut self, row: u16, col: u16) {
        let rel_row = row.saturating_sub(self.layout.agents_area_y);

        if self.is_notices_popup_open() {
            if let Some(area) = self.notices_popup_area()
                && point_in_rect(row, col, area)
            {
                if let Some(agent) = self.notices_copy_target_at(row, col).map(str::to_string) {
                    self.copy_notices_prompt(&agent);
                }
                return;
            }
            self.close_notices_popup();
            return;
        }
        if self.is_repo_popup_open() {
            if let Some(area) = self.repo_popup_area()
                && point_in_rect(row, col, area)
            {
                // Skip clicks on the popup chrome (top border / title row).
                // Without this guard `saturating_sub(1)` collapses a click on
                // the title row into `item_index == 0`, switching the filter
                // to the first repo the moment the user reaches for the
                // popup.
                if row > area.y {
                    let item_index = (row - area.y - 1) as usize;
                    if item_index < self.repo_names().len() {
                        self.set_repo_popup_selected(item_index);
                        self.confirm_repo_popup();
                    }
                }
                return;
            }
            self.close_repo_popup();
            return;
        }
        if self.is_spawn_input_open() {
            if let Some(area) = self.spawn_input_popup_area()
                && point_in_rect(row, col, area)
            {
                return;
            }
            self.close_spawn_input();
            return;
        }
        if self.is_remove_confirm_open() {
            if let Some(area) = self.remove_confirm_popup_area()
                && point_in_rect(row, col, area)
            {
                return;
            }
            self.close_remove_confirm();
            return;
        }

        if rel_row == 0 && !self.hide_filter_bar {
            self.handle_filter_click(col);
            return;
        }
        if let Some(row) = self.secondary_header_row()
            && rel_row == row
        {
            self.handle_secondary_header_click(col);
            return;
        }

        // Check the `+` spawn buttons before the pane-row fallback so a
        // click on the button doesn't also shift the pane selection.
        if let Some((repo_name, repo_root, anchor_y)) = self
            .layout
            .repo_spawn_targets
            .iter()
            .find(|t| point_in_rect(row, col, t.rect))
            .map(|t| (t.repo_name.clone(), t.repo_root.clone(), t.rect.y))
        {
            self.open_spawn_input_for_repo(repo_name, repo_root, Some(anchor_y));
            return;
        }

        // Check the red `×` remove markers next to spawn-created branches.
        if let Some(pane_id) = self
            .layout
            .spawn_remove_targets
            .iter()
            .find(|t| point_in_rect(row, col, t.rect))
            .map(|t| t.pane_id.clone())
        {
            self.open_remove_confirm_for_pane(pane_id);
            return;
        }

        // Agent-less session headers: no pane to select, so the click is a
        // plain jump to that session.
        if let Some(session) = self
            .layout
            .session_jump_targets
            .iter()
            .find(|t| point_in_rect(row, col, t.rect))
            .map(|t| t.session.clone())
        {
            crate::tmux::switch_session(&session);
            return;
        }

        let line_index =
            (rel_row as usize - self.list_start_row() as usize) + self.scrolls.panes.offset;
        if let Some(Some(agent_row)) = self.layout.line_to_row.get(line_index) {
            self.global.selected_pane_row = *agent_row;
            self.global.queue_cursor_save();
            self.activate_selected_pane();
        }
    }

    /// Open the link under an absolute frame cell, if there is one.
    /// Returns whether a link was found, so the caller can fall through to
    /// other click handling when there was not.
    pub fn open_link_at(&self, col: u16, row: u16) -> bool {
        let Some(url) = self.link_at(col, row) else {
            return false;
        };
        // A failed launch is not worth interrupting the frame for; the row
        // stays on screen and the user can try again.
        let _ = crate::link::open(&url, self.link_click_command.as_deref());
        true
    }

    /// URL of the hyperlink covering an absolute frame cell, if any.
    ///
    /// Reads the same overlays the renderer emits OSC 8 escapes from, so a
    /// click can only land on text the terminal also considers a link —
    /// there is no second notion of where links are.
    pub fn link_at(&self, col: u16, row: u16) -> Option<String> {
        self.layout
            .hyperlink_overlays
            .iter()
            .find(|overlay| {
                overlay.y == row
                    && col >= overlay.x
                    && col < overlay.x + display_width(&overlay.text) as u16
            })
            .map(|overlay| overlay.url.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn mouse_click_inside_repo_popup_with_nonzero_agents_area_y_does_not_close() {
        use crate::state::PopupState;

        let mut state = AppState::new("%99".into());
        state.layout.agents_area_y = 3;
        state.popup = PopupState::Repo {
            selected: 0,
            area: Some(Rect::new(30, 5, 10, 6)),
        };

        // Title-row click (row == area.y) stays inside without confirming.
        state.handle_mouse_click(5, 35);

        assert!(
            state.is_repo_popup_open(),
            "click inside the repo popup must not close it when agents_area_y > 0"
        );
    }

    // ─── link_at ─────────────────────────────────────────────────

    fn state_with_overlay(x: u16, y: u16, text: &str) -> AppState {
        let mut state = AppState::new("%99".into());
        state.layout.hyperlink_overlays = vec![crate::state::HyperlinkOverlay {
            x,
            y,
            text: text.into(),
            url: "https://example.com/pull/1".into(),
            style: ratatui::style::Style::default(),
        }];
        state
    }

    #[test]
    fn link_at_returns_the_url_under_the_click() {
        let state = state_with_overlay(4, 7, "#412 fix it");
        assert_eq!(
            state.link_at(4, 7).as_deref(),
            Some("https://example.com/pull/1")
        );
        assert_eq!(
            state.link_at(14, 7).as_deref(),
            Some("https://example.com/pull/1"),
            "last cell of the text is still the link"
        );
    }

    #[test]
    fn link_at_returns_none_outside_the_text_span() {
        let state = state_with_overlay(4, 7, "#412 fix it");
        assert!(state.link_at(3, 7).is_none(), "left of the span");
        assert!(state.link_at(15, 7).is_none(), "one past the span");
    }

    #[test]
    fn link_at_returns_none_on_another_row() {
        let state = state_with_overlay(4, 7, "#412 fix it");
        assert!(state.link_at(5, 6).is_none());
        assert!(state.link_at(5, 8).is_none());
    }

    #[test]
    fn link_at_measures_display_width_not_byte_length() {
        // "✓ 日本" is 4 bytes shorter than it is wide; a byte-length span
        // would stop short of the last cell.
        let state = state_with_overlay(0, 0, "✓ 日本");
        assert_eq!(display_width("✓ 日本"), 6);
        assert!(state.link_at(5, 0).is_some(), "last cell of a wide glyph");
        assert!(state.link_at(6, 0).is_none());
    }

    #[test]
    fn open_link_at_reports_whether_a_link_was_under_the_click() {
        let state = state_with_overlay(4, 7, "#412 fix it");
        assert!(state.open_link_at(5, 7), "hit opens the link");
        assert!(!state.open_link_at(50, 7), "miss opens nothing");
    }
}
