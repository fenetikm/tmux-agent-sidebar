use crate::cli::plugin_state;
use crate::session;
use crate::state::AppState;
use crate::tmux;
use crate::ui;

/// Construct and prime the initial [`AppState`] before the event loop starts.
///
/// Equivalent to the original `run_app` prelude in `src/main.rs`: installs the
/// color theme/icons from tmux options, loads global filter state, resolves
/// the Claude plugin install version once at startup, seeds session names
/// synchronously so `/rename` labels render on the first frame, and performs
/// the first refresh pass.
pub(super) fn init_state(tmux_pane: String) -> AppState {
    let mut state = AppState::new(tmux_pane);
    let opts = tmux::get_all_global_options();
    state.theme = ui::colors::ColorTheme::from_options(&opts);
    state.icons = ui::icons::StatusIcons::from_options(&opts);
    ui::apply_sidebar_ui_options(&mut state, &opts);
    state.hide_filter_bar = ui::hide_filter_bar_from_tmux();
    state.hide_repo_filter = ui::hide_repo_filter_from_tmux();
    state.global.apply_all(&opts);
    state.panel_config = crate::panel::PanelConfig::from_options(&opts);
    state.refresh();

    super::render::refresh_git_for_focused_pane(&mut state);

    // Resolve the installed Claude Code plugin status once at startup,
    // matching the version_notice pattern. Restart the sidebar after a
    // /plugin install, /plugin uninstall, or /plugin update to pick up
    // the new state.
    state.notices.claude_plugin_status = plugin_state::installed_plugin_status();
    // Likewise resolve whether the user still has legacy
    // tmux-agent-sidebar/hook.sh entries in ~/.claude/settings.json so
    // the notices popup can warn about duplicate hook execution.
    state.notices.claude_settings_has_residual_hooks =
        plugin_state::claude_settings_has_residual_hooks();
    // Notice inputs are static after the two lines above, so compute
    // them once here instead of from the per-tick refresh loop. This
    // also decouples the ⓘ badge from `focused_pane_id`, so killing
    // the last agent pane no longer drops outstanding setup warnings.
    state.refresh_notices();
    // Populate session names synchronously before the first draw so
    // `/rename`-assigned labels show up without waiting for the first
    // background scan tick.
    state.session_names.names = session::scan_session_names();
    state.session_names.dirty = true;
    state.refresh();

    state
}
