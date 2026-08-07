pub mod bottom;
pub mod colors;
pub mod icons;
pub mod notices;
pub mod panes;
pub mod pet;
pub mod sessions;
pub mod text;

use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::{state::AppState, tmux};

pub const BOTTOM_PANEL_HEIGHT: u16 = 20;

/// Rows reserved between the pane list and the bottom panel when the pet is
/// enabled. The pet and its desk/chair all render inside this band so they
/// never overdraw the pane list above or the bottom panel's border below.
pub const PET_SCENE_HEIGHT: u16 = 5;

/// Read `@sidebar_bottom_height` from tmux global options, falling back to the default.
/// A value of 0 hides the bottom panel entirely.
pub fn bottom_panel_height_from_options(opts: &HashMap<String, String>) -> u16 {
    opts.get(tmux::SIDEBAR_BOTTOM_HEIGHT)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(BOTTOM_PANEL_HEIGHT)
}

pub fn bottom_panel_height_from_tmux() -> u16 {
    let opts = tmux::get_all_global_options();
    bottom_panel_height_from_options(&opts)
}

/// Read `@sidebar_sessions_height` from tmux global options.
pub fn sessions_panel_height_from_options(
    opts: &HashMap<String, String>,
) -> crate::state::SessionsPanelHeight {
    crate::state::sessions_panel_height_from_options(opts)
}

/// Read `@sidebar_pet` from tmux global options, defaulting to `false` (off).
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn pet_enabled_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_PET)
        .map(|s| s.trim().to_ascii_lowercase())
        .map(|s| matches!(s.as_str(), "on" | "true" | "1" | "yes"))
        .unwrap_or(false)
}

pub fn pet_enabled_from_tmux() -> bool {
    let opts = crate::tmux::get_all_global_options();
    pet_enabled_from_options(&opts)
}

/// Read `@sidebar_show_session_names` from tmux global options, defaulting to
/// `true` to preserve the existing `/rename` label behavior.
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn show_session_names_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_SHOW_SESSION_NAMES)
        .map(|s| s.trim().to_ascii_lowercase())
        .map(|s| matches!(s.as_str(), "on" | "true" | "1" | "yes"))
        .unwrap_or(true)
}

pub fn show_session_names_from_tmux() -> bool {
    let opts = crate::tmux::get_all_global_options();
    show_session_names_from_options(&opts)
}

/// Read `@sidebar_compact` from tmux global options, defaulting to `false`
/// so existing users keep the variable-height rows.
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn compact_rows_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_COMPACT)
        .map(|s| s.trim().to_ascii_lowercase())
        .map(|s| matches!(s.as_str(), "on" | "true" | "1" | "yes"))
        .unwrap_or(false)
}

pub fn compact_rows_from_tmux() -> bool {
    let opts = crate::tmux::get_all_global_options();
    compact_rows_from_options(&opts)
}

// ── public entry point ──────────────────────────────────────────────

pub fn draw(frame: &mut Frame, state: &mut AppState) {
    state.layout.hyperlink_overlays.clear();
    state.layout.session_row_targets.clear();
    let area = frame.area();

    let bot_h = state.bottom_panel_height;
    let pet_band_h = if bot_h > 0 {
        if state.pet_enabled {
            PET_SCENE_HEIGHT
        } else {
            1
        }
    } else {
        0
    };

    let band_h = state
        .sessions
        .total_band_height(area.height, bot_h, pet_band_h);
    let sessions_content_h = band_h.saturating_sub(1);

    let mut constraints = Vec::new();
    if band_h > 0 {
        constraints.push(Constraint::Length(sessions_content_h));
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(1));
    if bot_h > 0 {
        constraints.push(Constraint::Length(pet_band_h));
        constraints.push(Constraint::Length(bot_h));
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut idx = 0usize;
    if band_h > 0 {
        sessions::draw_sessions_panel(frame, state, chunks[idx]);
        idx += 1;
        sessions::draw_sessions_divider(frame, state, chunks[idx]);
        idx += 1;
    }

    state.layout.agents_area_y = chunks[idx].y;
    panes::draw_agents(frame, state, chunks[idx]);
    idx += 1;

    if bot_h > 0 && chunks.len() > idx {
        if state.pet_enabled {
            let running_count = state.running_count();
            pet::draw_pet(frame, state, chunks[idx], running_count);
        }
        bottom::draw_bottom(frame, state, chunks[idx + 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts_with(key: &str, value: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(key.into(), value.into());
        m
    }

    #[test]
    fn bottom_height_defaults_when_option_missing() {
        let opts = HashMap::new();
        assert_eq!(bottom_panel_height_from_options(&opts), BOTTOM_PANEL_HEIGHT);
    }

    #[test]
    fn bottom_height_parses_valid_value() {
        let opts = opts_with(tmux::SIDEBAR_BOTTOM_HEIGHT, "12");
        assert_eq!(bottom_panel_height_from_options(&opts), 12);
    }

    #[test]
    fn bottom_height_trims_whitespace() {
        let opts = opts_with(tmux::SIDEBAR_BOTTOM_HEIGHT, "  8  ");
        assert_eq!(bottom_panel_height_from_options(&opts), 8);
    }

    #[test]
    fn bottom_height_zero_hides_panel() {
        let opts = opts_with(tmux::SIDEBAR_BOTTOM_HEIGHT, "0");
        assert_eq!(bottom_panel_height_from_options(&opts), 0);
    }

    #[test]
    fn bottom_height_falls_back_on_invalid_value() {
        let opts = opts_with(tmux::SIDEBAR_BOTTOM_HEIGHT, "abc");
        assert_eq!(bottom_panel_height_from_options(&opts), BOTTOM_PANEL_HEIGHT);
    }

    #[test]
    fn bottom_height_falls_back_on_empty_value() {
        let opts = opts_with(tmux::SIDEBAR_BOTTOM_HEIGHT, "");
        assert_eq!(bottom_panel_height_from_options(&opts), BOTTOM_PANEL_HEIGHT);
    }

    #[test]
    fn pet_defaults_off_when_option_missing() {
        let opts = HashMap::new();
        assert!(!pet_enabled_from_options(&opts));
    }

    #[test]
    fn pet_enabled_when_on() {
        for value in ["on", "ON", "true", "1", "yes"] {
            let opts = opts_with(tmux::SIDEBAR_PET, value);
            assert!(
                pet_enabled_from_options(&opts),
                "expected {value} to enable"
            );
        }
    }

    #[test]
    fn pet_disabled_when_off() {
        for value in ["off", "false", "0", "no", ""] {
            let opts = opts_with(tmux::SIDEBAR_PET, value);
            assert!(
                !pet_enabled_from_options(&opts),
                "expected {value} to disable"
            );
        }
    }

    #[test]
    fn session_names_default_on_when_option_missing() {
        let opts = HashMap::new();
        assert!(show_session_names_from_options(&opts));
    }

    #[test]
    fn session_names_disabled_when_off() {
        for value in ["off", "OFF", "false", "0", "no", ""] {
            let opts = opts_with(tmux::SIDEBAR_SHOW_SESSION_NAMES, value);
            assert!(
                !show_session_names_from_options(&opts),
                "expected {value} to disable session names"
            );
        }
    }

    #[test]
    fn session_names_enabled_when_on() {
        for value in ["on", "ON", "true", "1", "yes"] {
            let opts = opts_with(tmux::SIDEBAR_SHOW_SESSION_NAMES, value);
            assert!(
                show_session_names_from_options(&opts),
                "expected {value} to enable session names"
            );
        }
    }

    #[test]
    fn compact_rows_defaults_to_off() {
        let opts = HashMap::new();
        assert!(!compact_rows_from_options(&opts));
    }

    #[test]
    fn compact_rows_accepts_truthy_spellings() {
        for value in ["on", "true", "1", "yes", "ON", " On ", "TRUE"] {
            let mut opts = HashMap::new();
            opts.insert(tmux::SIDEBAR_COMPACT.into(), value.into());
            assert!(
                compact_rows_from_options(&opts),
                "{value:?} should enable compact rows"
            );
        }
    }

    #[test]
    fn compact_rows_rejects_other_values() {
        for value in ["off", "false", "0", "no", "", "maybe"] {
            let mut opts = HashMap::new();
            opts.insert(tmux::SIDEBAR_COMPACT.into(), value.into());
            assert!(
                !compact_rows_from_options(&opts),
                "{value:?} should leave compact rows off"
            );
        }
    }
}
