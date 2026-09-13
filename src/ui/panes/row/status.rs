use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::ctx::RowCtx;
use crate::tmux::{PaneStatus, PermissionMode};
use crate::ui::colors::ColorTheme;
use crate::ui::icons::StatusIcons;
use crate::ui::text::{display_width, elapsed_label, truncate_to_width};

/// Colour for a permission-mode badge. Shared by the expanded status row
/// and the compact header line so the two cannot drift apart.
pub(super) fn badge_color(mode: &PermissionMode, theme: &ColorTheme) -> Color {
    match mode {
        PermissionMode::BypassPermissions => theme.badge_danger,
        PermissionMode::Auto => theme.badge_auto,
        PermissionMode::DontAsk => theme.badge_auto,
        PermissionMode::Plan => theme.badge_plan,
        PermissionMode::AcceptEdits => theme.badge_auto,
        PermissionMode::Defer => theme.badge_auto,
        PermissionMode::Default => theme.text_muted,
    }
}

pub(super) fn status_row(
    pane: &crate::tmux::PaneInfo,
    ctx: &RowCtx,
    icons: &StatusIcons,
    spinner_frame: usize,
    now: u64,
    show_session_names: bool,
) -> Line<'static> {
    let theme = ctx.theme;

    let (icon, pulse_color) = running_icon_for(&pane.status, spinner_frame, icons);
    // `needs_user_attention`, not the raw `attention` flag: a pane held at an
    // idle prompt is blocked on the user without the flag, and the icon is
    // the only place that now says so.
    let icon_color = pulse_color
        .unwrap_or_else(|| theme.status_color(&pane.status, pane.needs_user_attention()));
    let title_raw: &str = if show_session_names && !pane.session_name.is_empty() {
        &pane.session_name
    } else {
        pane.agent.label()
    };
    let badge = pane.permission_mode.badge();
    let elapsed = elapsed_label(pane.started_at, now);

    let title_fg = theme.agent_color(&pane.agent);
    let elapsed_fg = if pane.status.is_active() {
        theme.text_active
    } else {
        theme.text_muted
    };

    let badge_extra = if badge.is_empty() { 0 } else { 1 };
    let fixed_width = display_width(icon) + 1 + badge_extra + display_width(badge);
    // User-supplied session names (set via `/rename`) can be arbitrarily
    // long; cap the title to the space left after reserving room for the
    // icon, badge, and elapsed label so they stay visible instead of
    // being pushed off-screen.
    let elapsed_width = display_width(&elapsed);
    let elapsed_gap = usize::from(elapsed_width > 0);
    let title_budget = ctx
        .inner_width
        .saturating_sub(fixed_width + elapsed_gap + elapsed_width);
    let title = truncate_to_width(title_raw, title_budget);

    let left_width = fixed_width + display_width(&title);
    let available_for_elapsed = ctx.inner_width.saturating_sub(left_width);
    let elapsed = truncate_to_width(&elapsed, available_for_elapsed);
    let elapsed_width = display_width(&elapsed);

    let mut left_spans: Vec<Span<'static>> = Vec::with_capacity(3);
    left_spans.push(Span::styled(
        icon.to_string(),
        ctx.apply_bg(Style::default().fg(icon_color)),
    ));
    left_spans.push(Span::styled(
        format!(" {}", title),
        ctx.apply_bg(Style::default().fg(title_fg)),
    ));
    if !badge.is_empty() {
        left_spans.push(Span::styled(
            format!(" {}", badge),
            ctx.apply_bg(Style::default().fg(badge_color(&pane.permission_mode, theme))),
        ));
    }

    let right_spans = vec![Span::styled(
        elapsed,
        ctx.apply_bg(Style::default().fg(elapsed_fg)),
    )];

    ctx.row_line_split(left_spans, left_width, right_spans, elapsed_width)
}

pub(super) fn running_icon_for<'a>(
    status: &PaneStatus,
    spinner_frame: usize,
    icons: &'a StatusIcons,
) -> (&'a str, Option<Color>) {
    use crate::SPINNER_PULSE;

    match status {
        PaneStatus::Running => {
            let color_idx = SPINNER_PULSE[spinner_frame % SPINNER_PULSE.len()];
            (icons.status_icon(status), Some(Color::Indexed(color_idx)))
        }
        _ => (icons.status_icon(status), None),
    }
}
