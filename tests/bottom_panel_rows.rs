#[allow(dead_code, unused_imports)]
mod test_helpers;

use test_helpers::*;
use tmux_agent_sidebar::panel::{PanelColor, PanelData, PanelRow};
use tmux_agent_sidebar::state::{AppState, BottomTab};

fn row(text: &str) -> PanelRow {
    PanelRow {
        text: text.into(),
        text_color: PanelColor::Default,
        icon: None,
        icon_color: PanelColor::Default,
        url: None,
        heading: false,
    }
}

fn state_with(rows: Vec<PanelRow>, error: Option<&str>) -> AppState {
    let mut state = AppState::new("%99".into());
    state.panel_config = Some(tmux_agent_sidebar::panel::PanelConfig {
        command: "echo hi".into(),
        name: "PRs".into(),
        interval: std::time::Duration::from_secs(120),
        timeout: std::time::Duration::from_secs(10),
    });
    state.bottom_tab = BottomTab::Panel;
    state.panel = Some(PanelData {
        rows,
        error: error.map(str::to_string),
        fetched_at: std::time::Instant::now(),
    });
    state
}

fn render(state: &mut AppState) -> String {
    // 28x24 matches every bottom-panel test in tests/bottom_tests.rs, so
    // these snapshots stay comparable with the existing ones.
    render_to_string(state, 28, 24)
}

#[test]
fn renders_plain_rows() {
    let mut state = state_with(vec![row("#412 fix flaky test"), row("#98 bump deps")], None);
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │#412 fix flaky test       │
    │#98 bump deps             │
    ╰──────────────────────────╯
    ");
}

#[test]
fn renders_icons_headings_and_colors() {
    let mut rows = vec![PanelRow {
        text: "Needs review".into(),
        text_color: PanelColor::Muted,
        icon: None,
        icon_color: PanelColor::Muted,
        url: None,
        heading: true,
    }];
    rows.push(PanelRow {
        text: "#412 approved".into(),
        text_color: PanelColor::Default,
        icon: Some("+".into()),
        icon_color: PanelColor::Success,
        url: Some("https://example.com/pr/412".into()),
        heading: false,
    });
    rows.push(PanelRow {
        text: "#77 stale".into(),
        text_color: PanelColor::Danger,
        icon: Some("!".into()),
        icon_color: PanelColor::Danger,
        url: None,
        heading: false,
    });
    let mut state = state_with(rows, None);
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │Needs review              │
    │+ #412 approved           │
    │! #77 stale               │
    ╰──────────────────────────╯
    ");
}

#[test]
fn truncates_long_rows_to_the_panel_width() {
    let mut state = state_with(
        vec![row(
            "#412 a very long pull request title that will not fit in the panel",
        )],
        None,
    );
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │#412 a very long pull req…│
    ╰──────────────────────────╯
    ");
}

#[test]
fn renders_the_error_footer_over_stale_rows() {
    let mut state = state_with(
        vec![row("#412 stale but useful")],
        Some("exit 1: gh: not found"),
    );
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │#412 stale but useful     │
    │exit 1: gh: not found     │
    ╰──────────────────────────╯
    ");
}

#[test]
fn renders_the_empty_state() {
    let mut state = state_with(vec![], None);
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │          No rows         │
    ╰──────────────────────────╯
    ");
}

#[test]
fn renders_the_loading_state_before_the_first_result() {
    let mut state = state_with(vec![], None);
    state.panel = None;
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │         Loading…         │
    ╰──────────────────────────╯
    ");
}

#[test]
fn rows_with_urls_register_hyperlink_overlays() {
    let mut rows = vec![row("#412 linked")];
    rows[0].url = Some("https://example.com/pr/412".into());
    rows.push(row("#98 not linked"));
    let mut state = state_with(rows, None);
    let _ = render(&mut state);

    assert_eq!(state.layout.hyperlink_overlays.len(), 1);
    let overlay = &state.layout.hyperlink_overlays[0];
    assert_eq!(overlay.url, "https://example.com/pr/412");
    assert_eq!(overlay.text, "#412 linked");
}

#[test]
fn scrolled_rows_start_from_the_offset() {
    let rows: Vec<PanelRow> = (1..=20).map(|n| row(&format!("row {n}"))).collect();
    let mut state = state_with(rows, None);
    state.scrolls.panel.offset = 5;
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ╭ Activity │ Git │ PRs ────╮
    │row 6                     │
    │row 7                     │
    │row 8                     │
    │row 9                     │
    │row 10                    │
    │row 11                    │
    │row 12                    │
    │row 13                    │
    │row 14                    │
    │row 15                    │
    │row 16                    │
    │row 17                    │
    │row 18                    │
    │row 19                    │
    │row 20                    │
    ╰──────────────────────────╯
    ");
}
