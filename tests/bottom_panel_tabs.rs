#[allow(dead_code, unused_imports)]
mod test_helpers;

use test_helpers::*;
use tmux_agent_sidebar::state::{AppState, BottomTab};

fn panel_config(name: &str) -> tmux_agent_sidebar::panel::PanelConfig {
    tmux_agent_sidebar::panel::PanelConfig {
        command: "echo hi".into(),
        name: name.into(),
        interval: std::time::Duration::from_secs(120),
        timeout: std::time::Duration::from_secs(10),
    }
}

#[test]
fn click_targets_cover_two_tabs_when_unconfigured() {
    let state = AppState::new("%99".into());
    let (_, targets) = tmux_agent_sidebar::ui::bottom::build_tab_title(&state);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].tab, BottomTab::Activity);
    assert_eq!(targets[1].tab, BottomTab::GitStatus);
}

#[test]
fn click_targets_include_the_named_panel_tab() {
    let mut state = AppState::new("%99".into());
    state.panel_config = Some(panel_config("PRs"));
    let (_, targets) = tmux_agent_sidebar::ui::bottom::build_tab_title(&state);
    assert_eq!(targets.len(), 3);
    assert_eq!(targets[2].tab, BottomTab::Panel);
    // "╭ Activity │ Git │ PRs" — Activity starts at column 2.
    assert_eq!(targets[0].start, 2);
}

#[test]
fn clicking_a_computed_range_selects_that_tab() {
    let mut state = AppState::new("%99".into());
    state.panel_config = Some(panel_config("PRs"));
    let (_, targets) = tmux_agent_sidebar::ui::bottom::build_tab_title(&state);
    state.layout.bottom_tab_targets = targets.clone();

    state.handle_bottom_tab_click(targets[2].start);
    assert_eq!(state.bottom_tab, BottomTab::Panel);

    state.handle_bottom_tab_click(targets[0].start);
    assert_eq!(state.bottom_tab, BottomTab::Activity);
}

#[test]
fn clicking_outside_every_range_changes_nothing() {
    let mut state = AppState::new("%99".into());
    let (_, targets) = tmux_agent_sidebar::ui::bottom::build_tab_title(&state);
    state.layout.bottom_tab_targets = targets;
    state.bottom_tab = BottomTab::GitStatus;
    state.handle_bottom_tab_click(200);
    assert_eq!(state.bottom_tab, BottomTab::GitStatus);
}

#[test]
fn tab_bar_renders_the_custom_panel_name() {
    let mut state = AppState::new("%99".into());
    state.panel_config = Some(panel_config("PRs"));
    state.bottom_tab = BottomTab::Panel;
    insta::assert_snapshot!(render_to_string(&mut state, 28, 24), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │       No panel data      │
    ╰──────────────────────────╯
    ");
}
