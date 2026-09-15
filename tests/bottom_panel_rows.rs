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
        centered: false,
    }
}

fn centered_row(text: &str) -> PanelRow {
    PanelRow {
        centered: true,
        text_color: PanelColor::Muted,
        ..row(text)
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
    ─ Activity │ Git │ PRs ─────
    #412 fix flaky test
    #98 bump deps
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
        centered: false,
    }];
    rows.push(PanelRow {
        text: "#412 approved".into(),
        text_color: PanelColor::Default,
        icon: Some("+".into()),
        icon_color: PanelColor::Success,
        url: Some("https://example.com/pr/412".into()),
        heading: false,
        centered: false,
    });
    rows.push(PanelRow {
        text: "#77 stale".into(),
        text_color: PanelColor::Danger,
        icon: Some("!".into()),
        icon_color: PanelColor::Danger,
        url: None,
        heading: false,
        centered: false,
    });
    let mut state = state_with(rows, None);
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
    Needs review
    + #412 approved
    ! #77 stale
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
    ─ Activity │ Git │ PRs ─────
    #412 a very long pull reque…
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
    ─ Activity │ Git │ PRs ─────
    #412 stale but useful
    exit 1: gh: not found
    ");
}

#[test]
fn renders_the_empty_state() {
    let mut state = state_with(vec![], None);
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
               No rows
    ");
}

#[test]
fn renders_the_loading_state_before_the_first_result() {
    let mut state = state_with(vec![], None);
    state.panel = None;
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
              Loading…
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
fn error_footer_does_not_overwrite_the_bottom_border_when_the_panel_has_no_room() {
    // `@sidebar_bottom_height` of 2 leaves a Block::inner height of 0 (both
    // rows are consumed by the top/bottom border lines drawn by
    // draw_bottom). Regression test for the error footer landing on the
    // row already used for the box's `╰───╯` bottom border.
    let mut state = state_with(
        vec![row("#412 stale but useful")],
        Some("exit 1: gh: not found"),
    );
    state.bottom_panel_height = 2;
    insta::assert_snapshot!(render_to_string(&mut state, 28, 24), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
    exit 1: gh: not found
    ");
}

#[test]
fn scroll_offset_reclamps_when_rows_shrink_between_fetches() {
    // Regression test: a stale large offset (from when there were many
    // rows) must not survive a fetch that shrinks the row count, or the
    // panel shows one row plus blank space instead of all rows.
    let rows = vec![row("row 1"), row("row 2"), row("row 3")];
    let mut state = state_with(rows, None);
    state.scrolls.panel.offset = 10;
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
    row 1
    row 2
    row 3
    ");
}

#[test]
fn scrolled_rows_start_from_the_offset() {
    let rows: Vec<PanelRow> = (1..=20).map(|n| row(&format!("row {n}"))).collect();
    let mut state = state_with(rows, None);
    state.scrolls.panel.offset = 5;
    insta::assert_snapshot!(render(&mut state), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
    row 2
    row 3
    row 4
    row 5
    row 6
    row 7
    row 8
    row 9
    row 10
    row 11
    row 12
    row 13
    row 14
    row 15
    row 16
    row 17
    row 18
    row 19
    row 20
    ");
}

#[test]
fn colors_map_rows_and_headings_onto_the_expected_theme_fields() {
    // render_to_string drops style info, so it cannot catch a regression in
    // `color_for`'s mapping. This asserts the styled buffer instead, one row
    // per `PanelColor` variant plus a heading, so a future edit to the
    // mapping (Default->text_active, Muted->text_muted, Accent->accent,
    // Success->status_running, Warning->status_waiting, Danger->status_error,
    // headings->section_title) would fail here.
    let rows = vec![
        PanelRow {
            text: "Heading".into(),
            text_color: PanelColor::Default,
            icon: None,
            icon_color: PanelColor::Default,
            url: None,
            heading: true,
            centered: false,
        },
        PanelRow {
            text: "default".into(),
            text_color: PanelColor::Default,
            icon: Some("*".into()),
            icon_color: PanelColor::Default,
            url: None,
            heading: false,
            centered: false,
        },
        PanelRow {
            text: "muted".into(),
            text_color: PanelColor::Muted,
            icon: Some("*".into()),
            icon_color: PanelColor::Muted,
            url: None,
            heading: false,
            centered: false,
        },
        PanelRow {
            text: "accent".into(),
            text_color: PanelColor::Accent,
            icon: Some("*".into()),
            icon_color: PanelColor::Accent,
            url: None,
            heading: false,
            centered: false,
        },
        PanelRow {
            text: "success".into(),
            text_color: PanelColor::Success,
            icon: Some("*".into()),
            icon_color: PanelColor::Success,
            url: None,
            heading: false,
            centered: false,
        },
        PanelRow {
            text: "warning".into(),
            text_color: PanelColor::Warning,
            icon: Some("*".into()),
            icon_color: PanelColor::Warning,
            url: None,
            heading: false,
            centered: false,
        },
        PanelRow {
            text: "danger".into(),
            text_color: PanelColor::Danger,
            icon: Some("*".into()),
            icon_color: PanelColor::Danger,
            url: None,
            heading: false,
            centered: false,
        },
    ];
    // Use a short bottom panel (9 rows -> inner height 7, one per row here)
    // so the styled snapshot has no trailing blank bordered rows to pad it
    // out — buffer_to_styled_string keeps every cell's style, unlike the
    // plain-text helper used by the other tests in this file, which drops
    // blank bordered rows.
    let mut state = state_with(rows, None);
    state.bottom_panel_height = 9;
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 13), @"
     ≡[fg:111]0[fg:245]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]0[fg:245]  ✕[fg:245]0[fg:245]
                             —[fg:252] ▾[fg:252]


    ─[fg:240] [fg:240]A[fg:252]c[fg:252]t[fg:252]i[fg:252]v[fg:252]i[fg:252]t[fg:252]y[fg:252] [fg:240]│[fg:240] [fg:240]G[fg:252]i[fg:252]t[fg:252] [fg:240]│[fg:240] [fg:240]P[fg:153]R[fg:153]s[fg:153] [fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]─[fg:240]
    H[fg:109]e[fg:109]a[fg:109]d[fg:109]i[fg:109]n[fg:109]g[fg:109]
    *[fg:255] d[fg:255]e[fg:255]f[fg:255]a[fg:255]u[fg:255]l[fg:255]t[fg:255]
    *[fg:252] m[fg:252]u[fg:252]t[fg:252]e[fg:252]d[fg:252]
    *[fg:153] a[fg:153]c[fg:153]c[fg:153]e[fg:153]n[fg:153]t[fg:153]
    *[fg:114] s[fg:114]u[fg:114]c[fg:114]c[fg:114]e[fg:114]s[fg:114]s[fg:114]
    *[fg:221] w[fg:221]a[fg:221]r[fg:221]n[fg:221]i[fg:221]n[fg:221]g[fg:221]
    *[fg:167] d[fg:167]a[fg:167]n[fg:167]g[fg:167]e[fg:167]r[fg:167]
    ");
}

#[test]
fn a_lone_centered_row_looks_like_the_built_in_empty_state() {
    let mut state = state_with(vec![centered_row("Not a git repository")], None);
    insta::assert_snapshot!(render_to_string(&mut state, 28, 24), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
        Not a git repository
    ");
}

/// Render keeping blank lines, which `render_to_string` strips. Vertical
/// placement is exactly what the centred-row rule is about, so it has to be
/// visible in the snapshot.
fn render_verbatim(state: &mut AppState, width: u16, height: u16) -> String {
    use ratatui::{Terminal, backend::TestBackend};
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| tmux_agent_sidebar::ui::draw(frame, state))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    (buf.area.y..buf.area.y + buf.area.height)
        .map(|y| {
            (buf.area.x..buf.area.x + buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_lone_centered_row_is_centred_vertically_in_a_tall_panel() {
    let mut state = state_with(vec![centered_row("Not a git repository")], None);
    state.bottom_panel_height = 7;
    insta::assert_snapshot!(render_verbatim(&mut state, 28, 12), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾



    ─ Activity │ Git │ PRs ─────


        Not a git repository
    ");
}

#[test]
fn normal_rows_stay_at_the_top_of_a_tall_panel() {
    let mut state = state_with(vec![row("#412 fix it")], None);
    state.bottom_panel_height = 7;
    insta::assert_snapshot!(render_verbatim(&mut state, 28, 12), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾



    ─ Activity │ Git │ PRs ─────
    #412 fix it
    ");
}

#[test]
fn centered_rows_among_normal_rows_stay_in_place() {
    let mut state = state_with(
        vec![
            row("#412 fix it"),
            centered_row("nothing else"),
            row("#410"),
        ],
        None,
    );
    insta::assert_snapshot!(render_to_string(&mut state, 28, 24), @"
     ≡0  ●0  ◎0  ◐0  ○0  ✕0
                             — ▾
    ─ Activity │ Git │ PRs ─────
    #412 fix it
            nothing else
    #410
    ");
}
