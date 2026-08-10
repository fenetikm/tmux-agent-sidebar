#[allow(dead_code, unused_imports)]
mod test_helpers;

use test_helpers::*;
use tmux_agent_sidebar::state::SessionsPanelHeight;
use tmux_agent_sidebar::tmux::{AgentType, PaneInfo, PaneStatus};

fn pane_with_session(id: &str, tmux_session: &str, attention: bool) -> PaneInfo {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    pane.pane_id = id.into();
    pane.tmux_session = tmux_session.into();
    pane.attention = attention;
    pane
}

fn setup_three_sessions_state() -> tmux_agent_sidebar::state::AppState {
    let mut state = make_state(vec![]);
    state.bottom_panel_height = 0;
    state.sessions.current_tmux_session = "main".into();
    state.sessions.height_mode = SessionsPanelHeight::Fixed(3);
    state.repo_groups = vec![make_repo_group(
        "project",
        vec![
            pane_with_session("%1", "main", false),
            pane_with_session("%2", "main", false),
            pane_with_session("%3", "main", false),
            pane_with_session("%4", "work", true),
            pane_with_session("%5", "work", false),
            pane_with_session("%6", "feat", false),
        ],
    )];
    state.sessions.refresh_rows(&state.repo_groups);
    state.rebuild_row_targets();
    state
}

fn setup_single_session_state() -> tmux_agent_sidebar::state::AppState {
    let mut state = make_state(vec![]);
    state.bottom_panel_height = 0;
    state.sessions.current_tmux_session = "main".into();
    state.repo_groups = vec![make_repo_group(
        "project",
        vec![pane_with_session("%1", "main", false)],
    )];
    state.sessions.refresh_rows(&state.repo_groups);
    state.rebuild_row_targets();
    state
}

#[test]
fn snapshot_sessions_panel_three_sessions() {
    let mut state = setup_three_sessions_state();
    insta::assert_snapshot!(render_to_string(&mut state, 28, 18), @"
    feat · (1)
    main · (3)
    ◐ work · (2)
    ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
     ≡6  ●0  ◎0  ◐0  ○6  ✕0
    ⓘ                        — ▾
    project
    ┃ ○ claude
    ┃   Waiting for prompt…
      ○ claude
        Waiting for prompt…
      ○ claude
        Waiting for prompt…
      ○ claude
        Waiting for prompt…
      ○ claude
        Waiting for prompt…
      ○ claude
    ");
}

#[test]
fn snapshot_sessions_panel_hidden_single_session() {
    let mut state = setup_single_session_state();
    insta::assert_snapshot!(render_to_string(&mut state, 28, 18), @"
     ≡1  ●0  ◎0  ◐0  ○1  ✕0
    ⓘ                        — ▾
    project
    ┃ ○ claude
    ┃   Waiting for prompt…
    ");
}

#[test]
fn snapshot_sessions_panel_current_session_accent() {
    let mut state = setup_three_sessions_state();
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 18), @"
    f[fg:255]e[fg:255]a[fg:255]t[fg:255] [fg:255]·[fg:255] [fg:255]([fg:255]1[fg:255])[fg:255]
    m[fg:153]a[fg:153]i[fg:153]n[fg:153] [fg:153]·[fg:153] [fg:153]([fg:153]3[fg:153])[fg:153]
    ◐[fg:255] [fg:255]w[fg:255]o[fg:255]r[fg:255]k[fg:255] [fg:255]·[fg:255] [fg:255]([fg:255]2[fg:255])[fg:255]
    ╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]╌[fg:240]
     ≡[fg:111]6[fg:255]  ●[fg:245]0[fg:245]  ◎[fg:245]0[fg:245]  ◐[fg:245]0[fg:245]  ○[fg:245]6[fg:255]  ✕[fg:245]0[fg:245]
    ⓘ[fg:221]                        —[fg:252] ▾[fg:252]
    p[fg:153]r[fg:153]o[fg:153]j[fg:153]e[fg:153]c[fg:153]t[fg:153]
    ┃[fg:153,bg:239] [bg:239]○[fg:110,bg:239] [fg:174,bg:239]c[fg:174,bg:239]l[fg:174,bg:239]a[fg:174,bg:239]u[fg:174,bg:239]d[fg:174,bg:239]e[fg:174,bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239] [bg:239]
    ┃[fg:153,bg:239]  [fg:255] [fg:255]W[fg:255]a[fg:255]i[fg:255]t[fg:255]i[fg:255]n[fg:255]g[fg:255] [fg:255]f[fg:255]o[fg:255]r[fg:255] [fg:255]p[fg:255]r[fg:255]o[fg:255]m[fg:255]p[fg:255]t[fg:255]…[fg:255]
      ○[fg:110] [fg:174]c[fg:174]l[fg:174]a[fg:174]u[fg:174]d[fg:174]e[fg:174]
       [fg:244] [fg:244]W[fg:244]a[fg:244]i[fg:244]t[fg:244]i[fg:244]n[fg:244]g[fg:244] [fg:244]f[fg:244]o[fg:244]r[fg:244] [fg:244]p[fg:244]r[fg:244]o[fg:244]m[fg:244]p[fg:244]t[fg:244]…[fg:244]
      ○[fg:110] [fg:174]c[fg:174]l[fg:174]a[fg:174]u[fg:174]d[fg:174]e[fg:174]
       [fg:244] [fg:244]W[fg:244]a[fg:244]i[fg:244]t[fg:244]i[fg:244]n[fg:244]g[fg:244] [fg:244]f[fg:244]o[fg:244]r[fg:244] [fg:244]p[fg:244]r[fg:244]o[fg:244]m[fg:244]p[fg:244]t[fg:244]…[fg:244]
      ○[fg:221] [fg:174]c[fg:174]l[fg:174]a[fg:174]u[fg:174]d[fg:174]e[fg:174]
       [fg:244] [fg:244]W[fg:244]a[fg:244]i[fg:244]t[fg:244]i[fg:244]n[fg:244]g[fg:244] [fg:244]f[fg:244]o[fg:244]r[fg:244] [fg:244]p[fg:244]r[fg:244]o[fg:244]m[fg:244]p[fg:244]t[fg:244]…[fg:244]
      ○[fg:110] [fg:174]c[fg:174]l[fg:174]a[fg:174]u[fg:174]d[fg:174]e[fg:174]
       [fg:244] [fg:244]W[fg:244]a[fg:244]i[fg:244]t[fg:244]i[fg:244]n[fg:244]g[fg:244] [fg:244]f[fg:244]o[fg:244]r[fg:244] [fg:244]p[fg:244]r[fg:244]o[fg:244]m[fg:244]p[fg:244]t[fg:244]…[fg:244]
      ○[fg:110] [fg:174]c[fg:174]l[fg:174]a[fg:174]u[fg:174]d[fg:174]e[fg:174]
    ");
}

#[test]
fn sessions_panel_scroll_tracks_row_count() {
    let mut state = setup_three_sessions_state();
    let _ = render_to_string(&mut state, 28, 18);
    assert_eq!(state.sessions.scroll.total_lines, 3);
    assert_eq!(state.sessions.scroll.visible_height, 3);
    assert_eq!(state.sessions.scroll.offset, 0);
}

#[test]
fn sessions_panel_populates_click_targets() {
    let mut state = setup_three_sessions_state();
    let _ = render_to_string(&mut state, 28, 18);
    assert_eq!(state.layout.session_row_targets.len(), 3);
    assert_eq!(state.layout.session_row_targets[0].tmux_session, "feat");
    assert_eq!(state.layout.session_row_targets[1].tmux_session, "main");
    assert_eq!(state.layout.session_row_targets[2].tmux_session, "work");
}

#[test]
fn sessions_panel_hidden_has_no_click_targets() {
    let mut state = setup_single_session_state();
    let _ = render_to_string(&mut state, 28, 18);
    assert!(state.layout.session_row_targets.is_empty());
    assert_eq!(state.sessions.total_band_height(18, 0, 0), 0);
}

#[test]
fn click_session_row_switches_when_not_current() {
    use tmux_agent_sidebar::state::resolve_session_row_click;

    let mut state = setup_three_sessions_state();
    let _ = render_to_string(&mut state, 28, 18);
    let target = state.layout.session_row_targets[0].rect;
    let session = resolve_session_row_click(
        target.y,
        target.x,
        &state.layout.session_row_targets,
        &state.sessions.current_tmux_session,
    );
    assert_eq!(session, Some("feat"));
}

#[test]
fn click_current_session_row_is_no_op() {
    use tmux_agent_sidebar::state::resolve_session_row_click;

    let mut state = setup_three_sessions_state();
    let _ = render_to_string(&mut state, 28, 18);
    let target = state.layout.session_row_targets[1].rect;
    assert_eq!(state.layout.session_row_targets[1].tmux_session, "main");
    assert_eq!(state.sessions.current_tmux_session, "main");
    let session = resolve_session_row_click(
        target.y,
        target.x,
        &state.layout.session_row_targets,
        &state.sessions.current_tmux_session,
    );
    assert_eq!(session, None);
}

#[test]
fn click_outside_session_row_targets_is_no_op() {
    use tmux_agent_sidebar::state::resolve_session_row_click;

    let mut state = setup_three_sessions_state();
    let _ = render_to_string(&mut state, 28, 18);
    let session = resolve_session_row_click(
        99,
        99,
        &state.layout.session_row_targets,
        &state.sessions.current_tmux_session,
    );
    assert_eq!(session, None);
}

#[test]
fn mouse_scroll_in_sessions_band_scrolls_sessions() {
    let mut state = setup_three_sessions_state();
    state.sessions.scroll = tmux_agent_sidebar::state::ScrollState {
        offset: 0,
        total_lines: 10,
        visible_height: 3,
    };
    state.handle_mouse_scroll(1, 18, 0, 3);
    assert_eq!(state.sessions.scroll.offset, 3);
    assert_eq!(state.scrolls.panes.offset, 0);
}
