use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::ctx::RowCtx;
use super::status::{badge_color, running_icon_for};
use crate::tmux::PaneStatus;
use crate::ui::icons::StatusIcons;
use crate::ui::text::{
    branch_label, display_width, elapsed_label, truncate_to_width, wait_reason_label, wrap_text,
    wrap_text_char,
};

/// Placeholder for line 2 when a pane has nothing to report, or when the
/// width leaves no room for real content. Rendering a visible `-` rather
/// than whitespace lets the reader tell "nothing to say" apart from a
/// drawing fault, and keeps the two-line shape legible.
const EMPTY_BODY: &str = "-";

/// Columns reserved for the `  ` indent at the start of line 2.
const BODY_PREFIX_WIDTH: usize = 2;

/// Render one agent entry as exactly two lines.
///
/// Line 1 fuses what the expanded rows split across the status row and the
/// branch row (`marker_ctx`, including selection background when selected).
/// Line 2 carries a single contextual detail (`plain_ctx`, same marker but
/// no selection background on the text).
pub(super) fn render_pane_lines(
    pane: &crate::tmux::PaneInfo,
    git_info: &crate::group::PaneGitInfo,
    marker_ctx: &RowCtx,
    plain_ctx: &RowCtx,
    icons: &StatusIcons,
    spinner_frame: usize,
    now: u64,
) -> Vec<Line<'static>> {
    vec![
        header_line(pane, git_info, marker_ctx, icons, spinner_frame, now),
        body_line(pane, plain_ctx),
    ]
}

/// `status icon · provider glyph · badge · branch` on the left, elapsed on
/// the right. Each left part is dropped along with its separating space
/// when empty, so a pane with no branch and no badge does not leave holes.
fn header_line(
    pane: &crate::tmux::PaneInfo,
    git_info: &crate::group::PaneGitInfo,
    ctx: &RowCtx,
    icons: &StatusIcons,
    spinner_frame: usize,
    now: u64,
) -> Line<'static> {
    let theme = ctx.theme;

    let (icon, pulse_color) = running_icon_for(&pane.status, spinner_frame, icons);
    let icon_color =
        pulse_color.unwrap_or_else(|| theme.status_color(&pane.status, pane.attention));
    let badge = pane.permission_mode.badge();
    let branch = branch_label(git_info);
    let elapsed = elapsed_label(pane.started_at, now);

    // Build the left group as (text, colour) pairs first, then measure and
    // style in one pass. Accumulating into a helper closure would borrow
    // `spans` for longer than the branch budget calculation below allows.
    let mut left: Vec<(String, Color)> = Vec::with_capacity(4);
    left.push((icon.to_string(), icon_color));
    left.push((
        format!(" {}", icons.agent_icon(&pane.agent)),
        theme.agent_color(&pane.agent),
    ));
    if !badge.is_empty() {
        left.push((
            format!(" {}", badge),
            badge_color(&pane.permission_mode, theme),
        ));
    }
    let mut left_width: usize = left.iter().map(|(t, _)| display_width(t)).sum();

    // The branch takes whatever is left once the icons, badge, and the
    // right-aligned elapsed label have their columns. There is no fixed
    // cap: it ellipsizes only when it genuinely does not fit.
    let elapsed_width = display_width(&elapsed);
    let elapsed_gap = usize::from(elapsed_width > 0);
    if !branch.is_empty() {
        let budget = ctx
            .inner_width
            .saturating_sub(left_width + 1 + elapsed_gap + elapsed_width);
        let shown = truncate_to_width(&branch, budget);
        if !shown.is_empty() {
            let text = format!(" {}", shown);
            left_width += display_width(&text);
            left.push((text, theme.branch));
        }
    }

    let left_spans: Vec<Span<'static>> = left
        .into_iter()
        .map(|(text, color)| Span::styled(text, ctx.apply_bg(Style::default().fg(color))))
        .collect();

    let elapsed_fg = if pane.status.is_active() {
        theme.text_active
    } else {
        theme.text_muted
    };
    let elapsed = truncate_to_width(&elapsed, ctx.inner_width.saturating_sub(left_width));
    let elapsed_width = display_width(&elapsed);
    let right_spans = vec![Span::styled(
        elapsed,
        ctx.apply_bg(Style::default().fg(elapsed_fg)),
    )];

    ctx.row_line_split(left_spans, left_width, right_spans, elapsed_width)
}

/// The single most actionable detail for this pane, as `(text, colour,
/// is_response)`. Precedence matches the expanded path's top-to-bottom
/// order, so compact mode shows whichever row expanded mode shows first.
/// An empty text means "nothing to report" and renders as [`EMPTY_BODY`].
fn body_content(pane: &crate::tmux::PaneInfo, ctx: &RowCtx) -> (String, Color, bool) {
    let theme = ctx.theme;

    if matches!(pane.status, PaneStatus::Waiting | PaneStatus::Error)
        && !pane.wait_reason.is_empty()
    {
        let color = if matches!(pane.status, PaneStatus::Error) {
            theme.status_error
        } else {
            theme.wait_reason
        };
        return (wait_reason_label(&pane.wait_reason), color, false);
    }

    if let Some(cmd) = pane.bg_shell_cmd.as_deref() {
        return (format!("$ {}", cmd.trim()), theme.status_running, false);
    }

    let prompt_color = if ctx.active {
        theme.text_active
    } else {
        theme.text_inactive
    };
    if !pane.prompt.is_empty() {
        return (pane.prompt.clone(), prompt_color, pane.prompt_is_response);
    }
    if matches!(pane.status, PaneStatus::Idle) {
        return ("Waiting for prompt…".to_string(), prompt_color, false);
    }

    (String::new(), theme.text_muted, false)
}

fn body_line(pane: &crate::tmux::PaneInfo, ctx: &RowCtx) -> Line<'static> {
    let (text, color, is_response) = body_content(pane, ctx);

    // `wrap_text` with `max_lines: 1` already ellipsizes on overflow, so
    // this is the same call the expanded prompt rows make with a smaller
    // budget — no separate truncation path to keep in sync.
    let room = ctx.inner_width.saturating_sub(BODY_PREFIX_WIDTH);
    let wrapped = if is_response {
        wrap_text_char(&text, room, 1)
    } else {
        wrap_text(&text, room, 1)
    };
    let shown = wrapped.into_iter().next().unwrap_or_default();

    if shown.is_empty() || shown == "…" {
        // Either there was nothing to say, or the width left no room for
        // it. Both read better as a muted dash than as a blank line.
        let text = truncate_to_width(&format!("  {}", EMPTY_BODY), ctx.inner_width);
        let width = display_width(&text);
        return ctx.row_line(
            vec![Span::styled(
                text,
                ctx.apply_bg(Style::default().fg(ctx.theme.text_muted)),
            )],
            width,
        );
    }

    let text = format!("  {}", shown);
    let width = display_width(&text);
    ctx.row_line(
        vec![Span::styled(text, ctx.apply_bg(Style::default().fg(color)))],
        width,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::PaneGitInfo;
    use crate::tmux::{AgentType, PaneInfo, PermissionMode, WorktreeMetadata};
    use crate::ui::colors::ColorTheme;
    use crate::ui::icons::StatusIcons;

    const NOW: u64 = 1_700_000_000;

    fn pane(status: PaneStatus) -> PaneInfo {
        PaneInfo {
            pane_id: "%1".into(),
            pane_active: false,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/tmp/project".into(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: Some(NOW - 200),
            wait_reason: String::new(),
            permission_mode: PermissionMode::Auto,
            subagents: vec![],
            pane_pid: None,
            worktree: WorktreeMetadata::default(),
            session_id: None,
            session_name: "my-session".into(),
            tmux_session: String::new(),
            window_id: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }

    fn git(branch: &str) -> PaneGitInfo {
        PaneGitInfo {
            repo_root: Some("/tmp/project".into()),
            branch: Some(branch.into()),
            is_worktree: false,
            worktree_name: None,
        }
    }

    fn ctx<'a>(theme: &'a ColorTheme, inner_width: usize) -> RowCtx<'a> {
        RowCtx {
            marker_char: " ",
            marker_style: Style::default(),
            inner_width,
            theme,
            bg: None,
            active: false,
        }
    }

    /// Render both lines to plain text, one per output line, with the
    /// trailing pad stripped so the snapshot shows content not whitespace.
    fn render(pane: &PaneInfo, git_info: &PaneGitInfo, width: usize) -> String {
        let theme = ColorTheme::default();
        let c = ctx(&theme, width);
        let lines = render_pane_lines(pane, git_info, &c, &c, &StatusIcons::default(), 0, NOW);
        assert_eq!(lines.len(), 2, "compact rows are always exactly two lines");
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn header_shows_status_provider_badge_branch_and_elapsed() {
        let mut p = pane(PaneStatus::Running);
        p.prompt = "Refactor the notification backend".into();
        insta::assert_snapshot!(render(&p, &git("feat/session-blocklist"), 44), @"● ✳ auto feat/session-blocklist        3m20s
  Refactor the notification backend");
    }

    #[test]
    fn header_omits_badge_when_permission_mode_is_default() {
        let mut p = pane(PaneStatus::Idle);
        p.permission_mode = PermissionMode::Default;
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"○ ✳ main                               3m20s
  Waiting for prompt…");
    }

    #[test]
    fn header_omits_branch_when_git_info_is_empty() {
        let p = pane(PaneStatus::Idle);
        insta::assert_snapshot!(render(&p, &PaneGitInfo::default(), 44), @"○ ✳ auto                               3m20s
  Waiting for prompt…");
    }

    #[test]
    fn header_ellipsizes_branch_at_narrow_width() {
        let mut p = pane(PaneStatus::Running);
        p.prompt = "Investigate the failing snapshot test".into();
        insta::assert_snapshot!(render(&p, &git("feat/a-very-long-branch-name"), 26), @"● ✳ auto feat/a-ver… 3m20s
  Investigate the failing…");
    }

    #[test]
    fn body_prefers_wait_reason_when_waiting() {
        let mut p = pane(PaneStatus::Waiting);
        p.wait_reason = "permission_prompt".into();
        p.prompt = "this prompt must not win".into();
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"◐ ✳ auto main                          3m20s
  permission required");
    }

    #[test]
    fn body_prefers_wait_reason_when_errored() {
        let mut p = pane(PaneStatus::Error);
        p.wait_reason = "rate_limit".into();
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"✕ ✳ auto main                          3m20s
  rate limit");
    }

    #[test]
    fn body_shows_background_command_over_prompt() {
        let mut p = pane(PaneStatus::Background);
        p.bg_shell_cmd = Some("cargo watch -x test".into());
        p.prompt = "this prompt must not win".into();
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"◎ ✳ auto main                          3m20s
  $ cargo watch -x test");
    }

    #[test]
    fn body_shows_single_prompt_line_truncated() {
        let mut p = pane(PaneStatus::Running);
        p.prompt = "Add a compact display mode that renders every agent entry in two lines".into();
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"● ✳ auto main                          3m20s
  Add a compact display mode that renders e…");
    }

    #[test]
    fn body_shows_response_without_arrow() {
        let mut p = pane(PaneStatus::Running);
        p.prompt = "Done — the backend now dispatches by name".into();
        p.prompt_is_response = true;
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"● ✳ auto main                          3m20s
  Done — the backend now dispatches by name");
    }

    #[test]
    fn body_shows_idle_hint_when_idle_without_prompt() {
        let p = pane(PaneStatus::Idle);
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"○ ✳ auto main                          3m20s
  Waiting for prompt…");
    }

    #[test]
    fn body_falls_back_to_dash_when_there_is_nothing_to_report() {
        // Running with no prompt, no wait reason, no background command.
        let p = pane(PaneStatus::Running);
        insta::assert_snapshot!(render(&p, &git("main"), 44), @"● ✳ auto main                          3m20s
  -");
    }

    #[test]
    fn body_falls_back_to_dash_when_the_prompt_truncates_away() {
        let mut p = pane(PaneStatus::Running);
        p.prompt = "a prompt with no room to render".into();
        insta::assert_snapshot!(render(&p, &git("main"), 3), @"● ✳ auto
  -");
    }

    #[test]
    fn every_status_produces_exactly_two_lines() {
        let theme = ColorTheme::default();
        for status in [
            PaneStatus::Running,
            PaneStatus::Background,
            PaneStatus::Waiting,
            PaneStatus::Idle,
            PaneStatus::Error,
            PaneStatus::Unknown,
        ] {
            for width in [0usize, 1, 3, 10, 44, 120] {
                let c = ctx(&theme, width);
                let lines = render_pane_lines(
                    &pane(status.clone()),
                    &git("main"),
                    &c,
                    &c,
                    &StatusIcons::default(),
                    0,
                    NOW,
                );
                assert_eq!(
                    lines.len(),
                    2,
                    "status {status:?} at width {width} must render two lines"
                );
            }
        }
    }
}
