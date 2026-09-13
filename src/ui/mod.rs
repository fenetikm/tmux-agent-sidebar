pub mod bottom;
pub mod colors;
pub mod icons;
pub mod notices;
pub mod panes;
pub mod pet;
pub mod text;

use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::{group::SortMode, state::AppState, tmux};

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

/// Read `@sidebar_show_worktree_marker` from tmux global options, defaulting
/// to `true` so the `+ ` worktree prefix keeps showing unless asked otherwise.
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn show_worktree_marker_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_SHOW_WORKTREE_MARKER)
        .map(|s| s.trim().to_ascii_lowercase())
        .map(|s| matches!(s.as_str(), "on" | "true" | "1" | "yes"))
        .unwrap_or(true)
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

/// Read `@sidebar_hide_filter_bar` from tmux global options, defaulting to
/// `false` so existing users keep the status filter bar visible.
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn hide_filter_bar_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_HIDE_FILTER_BAR)
        .map(|s| parse_tmux_truthy(s))
        .unwrap_or(false)
}

pub fn hide_filter_bar_from_tmux() -> bool {
    tmux::get_option(tmux::SIDEBAR_HIDE_FILTER_BAR)
        .map(|s| parse_tmux_truthy(&s))
        .unwrap_or(false)
}

/// Read `@sidebar_hide_repo_filter` from tmux global options, defaulting to
/// `false` so existing users keep the repo filter button visible.
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn hide_repo_filter_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_HIDE_REPO_FILTER)
        .map(|s| parse_tmux_truthy(s))
        .unwrap_or(false)
}

pub fn hide_repo_filter_from_tmux() -> bool {
    tmux::get_option(tmux::SIDEBAR_HIDE_REPO_FILTER)
        .map(|s| parse_tmux_truthy(&s))
        .unwrap_or(false)
}

/// Read `@sidebar_sorting` from tmux global options, defaulting to
/// `SortMode::Repository`. Unset, empty, `repository`, and any typo all fall
/// through to the default; only `session` selects session grouping.
pub fn sort_mode_from_options(opts: &HashMap<String, String>) -> SortMode {
    match opts
        .get(tmux::SIDEBAR_SORTING)
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("session") => SortMode::Session,
        _ => SortMode::Repository,
    }
}

pub fn sort_mode_from_tmux() -> SortMode {
    let opts = crate::tmux::get_all_global_options();
    sort_mode_from_options(&opts)
}

/// Read `@sidebar_show_empty_sessions` from tmux global options, defaulting
/// to `false` so the list keeps showing only sessions that hold agents.
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn show_empty_sessions_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_SHOW_EMPTY_SESSIONS)
        .map(|s| parse_tmux_truthy(s))
        .unwrap_or(false)
}

/// Read `@sidebar_link_click_command` from tmux global options. Unset or
/// blank yields `None`, which means the platform default opener.
pub fn link_click_command_from_options(opts: &HashMap<String, String>) -> Option<String> {
    opts.get(tmux::SIDEBAR_LINK_CLICK_COMMAND)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Read `@sidebar_show_activity_tab` from tmux global options, defaulting to
/// `true` so the tab keeps showing for users who never set it.
/// Accepts `on`/`off`, `true`/`false`, `1`/`0` (case-insensitive).
pub fn show_activity_tab_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_SHOW_ACTIVITY_TAB)
        .map(|s| parse_tmux_truthy(s))
        .unwrap_or(true)
}

/// Read `@sidebar_show_git_tab` from tmux global options, defaulting to
/// `true`, like [`show_activity_tab_from_options`].
pub fn show_git_tab_from_options(opts: &HashMap<String, String>) -> bool {
    opts.get(tmux::SIDEBAR_SHOW_GIT_TAB)
        .map(|s| parse_tmux_truthy(s))
        .unwrap_or(true)
}

fn parse_tmux_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "on" | "true" | "1" | "yes"
    )
}

/// Apply layout-related sidebar options from a single `show -g` snapshot.
/// Called at startup and whenever tmux globals are re-synced.
pub fn apply_sidebar_ui_options(state: &mut AppState, opts: &HashMap<String, String>) {
    state.bottom_panel_height = bottom_panel_height_from_options(opts);
    state.pet_enabled = pet_enabled_from_options(opts);
    state.show_session_names = show_session_names_from_options(opts);
    state.show_worktree_marker = show_worktree_marker_from_options(opts);
    state.compact_rows = compact_rows_from_options(opts);
    state.hide_filter_bar = hide_filter_bar_from_options(opts);
    state.hide_repo_filter = hide_repo_filter_from_options(opts);
    state.sort_mode = sort_mode_from_options(opts);
    state.show_empty_sessions = show_empty_sessions_from_options(opts);
    state.link_click_command = link_click_command_from_options(opts);
    state.show_activity_tab = show_activity_tab_from_options(opts);
    state.show_git_tab = show_git_tab_from_options(opts);
    if state.hide_filter_bar && state.focus_state.focus == crate::state::Focus::Filter {
        state.focus_state.focus = crate::state::Focus::Panes;
    }
}

// ── public entry point ──────────────────────────────────────────────

pub fn draw(frame: &mut Frame, state: &mut AppState) {
    state.layout.hyperlink_overlays.clear();
    let area = frame.area();

    // Every tab switched off means there is nothing to put in the bottom
    // panel, so it collapses exactly as `@sidebar_bottom_height 0` does.
    let bot_h = if state.enabled_bottom_tabs().is_empty() {
        0
    } else {
        state.bottom_panel_height
    };
    let pet_band_h = if bot_h > 0 {
        if state.pet_enabled {
            PET_SCENE_HEIGHT
        } else {
            1
        }
    } else {
        0
    };

    let mut constraints = vec![Constraint::Min(1)];
    if bot_h > 0 {
        constraints.push(Constraint::Length(pet_band_h));
        constraints.push(Constraint::Length(bot_h));
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    state.layout.agents_area_y = chunks[0].y;
    panes::draw_agents(frame, state, chunks[0]);

    if bot_h > 0 && chunks.len() > 2 {
        if state.pet_enabled {
            let running_count = state.running_count();
            pet::draw_pet(frame, state, chunks[1], running_count);
        }
        bottom::draw_bottom(frame, state, chunks[2]);
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
    fn empty_sessions_default_off_when_option_missing() {
        let opts = HashMap::new();
        assert!(!show_empty_sessions_from_options(&opts));
    }

    #[test]
    fn empty_sessions_enabled_when_on() {
        for value in ["on", "ON", "true", "1", "yes"] {
            let opts = opts_with(tmux::SIDEBAR_SHOW_EMPTY_SESSIONS, value);
            assert!(
                show_empty_sessions_from_options(&opts),
                "expected {value} to show empty sessions"
            );
        }
    }

    #[test]
    fn empty_sessions_disabled_when_off_or_unrecognised() {
        for value in ["off", "OFF", "false", "0", "no", "", "maybe"] {
            let opts = opts_with(tmux::SIDEBAR_SHOW_EMPTY_SESSIONS, value);
            assert!(
                !show_empty_sessions_from_options(&opts),
                "expected {value} to hide empty sessions"
            );
        }
    }

    #[test]
    fn worktree_marker_default_on_when_option_missing() {
        let opts = HashMap::new();
        assert!(show_worktree_marker_from_options(&opts));
    }

    #[test]
    fn worktree_marker_disabled_when_off() {
        for value in ["off", "OFF", "false", "0", "no", ""] {
            let opts = opts_with(tmux::SIDEBAR_SHOW_WORKTREE_MARKER, value);
            assert!(
                !show_worktree_marker_from_options(&opts),
                "expected {value} to hide the worktree marker"
            );
        }
    }

    #[test]
    fn worktree_marker_enabled_when_on() {
        for value in ["on", "ON", "true", "1", "yes"] {
            let opts = opts_with(tmux::SIDEBAR_SHOW_WORKTREE_MARKER, value);
            assert!(
                show_worktree_marker_from_options(&opts),
                "expected {value} to show the worktree marker"
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

    #[test]
    fn hide_filter_bar_defaults_to_off() {
        let opts = HashMap::new();
        assert!(!hide_filter_bar_from_options(&opts));
    }

    #[test]
    fn hide_filter_bar_accepts_truthy_spellings() {
        for value in ["on", "true", "1", "yes", "ON", " On ", "TRUE"] {
            let mut opts = HashMap::new();
            opts.insert(tmux::SIDEBAR_HIDE_FILTER_BAR.into(), value.into());
            assert!(
                hide_filter_bar_from_options(&opts),
                "{value:?} should hide the filter bar"
            );
        }
    }

    #[test]
    fn hide_filter_bar_rejects_other_values() {
        for value in ["off", "false", "0", "no", "", "maybe"] {
            let mut opts = HashMap::new();
            opts.insert(tmux::SIDEBAR_HIDE_FILTER_BAR.into(), value.into());
            assert!(
                !hide_filter_bar_from_options(&opts),
                "{value:?} should leave the filter bar visible"
            );
        }
    }

    #[test]
    fn hide_repo_filter_defaults_to_off() {
        let opts = HashMap::new();
        assert!(!hide_repo_filter_from_options(&opts));
    }

    #[test]
    fn hide_repo_filter_accepts_truthy_spellings() {
        for value in ["on", "true", "1", "yes", "ON", " On ", "TRUE"] {
            let mut opts = HashMap::new();
            opts.insert(tmux::SIDEBAR_HIDE_REPO_FILTER.into(), value.into());
            assert!(
                hide_repo_filter_from_options(&opts),
                "{value:?} should hide the repo filter"
            );
        }
    }

    #[test]
    fn hide_repo_filter_rejects_other_values() {
        for value in ["off", "false", "0", "no", "", "maybe"] {
            let mut opts = HashMap::new();
            opts.insert(tmux::SIDEBAR_HIDE_REPO_FILTER.into(), value.into());
            assert!(
                !hide_repo_filter_from_options(&opts),
                "{value:?} should leave the repo filter visible"
            );
        }
    }

    #[test]
    fn apply_sidebar_ui_options_hides_status_filter_bar() {
        let mut state = AppState::new("%0".into());
        let opts = opts_with(tmux::SIDEBAR_HIDE_FILTER_BAR, "on");
        apply_sidebar_ui_options(&mut state, &opts);
        assert!(state.hide_filter_bar);
        assert!(!state.show_filter_bar());
    }

    #[test]
    fn sort_mode_defaults_to_repository_when_option_missing() {
        let opts = HashMap::new();
        assert_eq!(
            sort_mode_from_options(&opts),
            crate::group::SortMode::Repository
        );
    }

    #[test]
    fn sort_mode_accepts_session_spellings() {
        for value in ["session", "Session", "SESSION", " session "] {
            let opts = opts_with(tmux::SIDEBAR_SORTING, value);
            assert_eq!(
                sort_mode_from_options(&opts),
                crate::group::SortMode::Session,
                "{value:?} should select session grouping"
            );
        }
    }

    #[test]
    fn sort_mode_falls_back_to_repository_for_everything_else() {
        // A tmux option cannot be validated when it is set, so a typo
        // yielding the default is the contract every other option offers.
        for value in ["repository", "Repository", "", "  ", "sessions", "garbage"] {
            let opts = opts_with(tmux::SIDEBAR_SORTING, value);
            assert_eq!(
                sort_mode_from_options(&opts),
                crate::group::SortMode::Repository,
                "{value:?} should fall back to repository grouping"
            );
        }
    }

    #[test]
    fn apply_sidebar_ui_options_assigns_sort_mode() {
        let mut state = AppState::new("%0".into());
        assert_eq!(state.sort_mode, crate::group::SortMode::Repository);
        let opts = opts_with(tmux::SIDEBAR_SORTING, "session");
        apply_sidebar_ui_options(&mut state, &opts);
        assert_eq!(state.sort_mode, crate::group::SortMode::Session);
    }

    // ─── bottom tab toggles ──────────────────────────────────────

    #[test]
    fn show_activity_tab_defaults_to_true_when_option_missing() {
        assert!(show_activity_tab_from_options(&HashMap::new()));
    }

    #[test]
    fn show_activity_tab_off_hides_the_tab() {
        let opts = opts_with(tmux::SIDEBAR_SHOW_ACTIVITY_TAB, "off");
        assert!(!show_activity_tab_from_options(&opts));
    }

    #[test]
    fn show_git_tab_defaults_to_true_when_option_missing() {
        assert!(show_git_tab_from_options(&HashMap::new()));
    }

    #[test]
    fn show_git_tab_off_hides_the_tab() {
        let opts = opts_with(tmux::SIDEBAR_SHOW_GIT_TAB, "off");
        assert!(!show_git_tab_from_options(&opts));
    }

    #[test]
    fn apply_sidebar_ui_options_reads_the_tab_toggles() {
        let mut state = AppState::new("%0".into());
        let opts = opts_with(tmux::SIDEBAR_SHOW_GIT_TAB, "off");
        apply_sidebar_ui_options(&mut state, &opts);
        assert!(!state.show_git_tab);
        assert!(
            state.show_activity_tab,
            "untouched option keeps its default"
        );
    }

    #[test]
    fn link_click_command_defaults_to_unset() {
        assert_eq!(link_click_command_from_options(&HashMap::new()), None);
    }

    #[test]
    fn link_click_command_reads_the_option() {
        let opts = opts_with(tmux::SIDEBAR_LINK_CLICK_COMMAND, "firefox");
        assert_eq!(
            link_click_command_from_options(&opts).as_deref(),
            Some("firefox")
        );
    }

    #[test]
    fn apply_sidebar_ui_options_reads_the_link_click_command() {
        let mut state = AppState::new("%0".into());
        let opts = opts_with(tmux::SIDEBAR_LINK_CLICK_COMMAND, "firefox");
        apply_sidebar_ui_options(&mut state, &opts);
        assert_eq!(state.link_click_command.as_deref(), Some("firefox"));
    }
}
