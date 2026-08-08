use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::state::{AppState, SessionRowTarget};
use crate::tmux::PaneStatus;
use crate::ui::text::{display_width, truncate_to_width};

const DIVIDER_CHAR: char = '\u{254c}';

pub fn draw_sessions_panel(frame: &mut Frame, state: &mut AppState, area: Rect) {
    if area.height == 0 || state.sessions.rows.is_empty() {
        return;
    }

    let theme = &state.theme;
    let waiting_icon = state.icons.status_icon(&PaneStatus::Waiting);
    let row_width = area.width as usize;

    state.sessions.scroll.total_lines = state.sessions.rows.len();
    state.sessions.scroll.visible_height = area.height as usize;
    // Clamp offset when row count or viewport shrinks (mirror bottom/activity.rs).
    state.sessions.scroll.scroll(0);

    let offset = state.sessions.scroll.offset;
    let visible_count = area.height as usize;
    let mut lines: Vec<Line> = Vec::new();

    for (visible_idx, row) in state
        .sessions
        .rows
        .iter()
        .skip(offset)
        .take(visible_count)
        .enumerate()
    {
        let suffix = format!(" · ({})", row.agent_count);

        let prefix = if row.has_attention {
            format!("{} ", waiting_icon)
        } else {
            String::new()
        };

        let prefix_w = display_width(&prefix);
        let suffix_w = display_width(&suffix);
        let name_max_w = row_width.saturating_sub(prefix_w + suffix_w);
        let name = truncate_to_width(&row.tmux_session, name_max_w);
        let text = format!("{prefix}{name}{suffix}");

        let fg = if row.is_current {
            theme.accent
        } else {
            theme.text_active
        };

        lines.push(Line::from(Span::styled(text, Style::default().fg(fg))));

        state.layout.session_row_targets.push(SessionRowTarget {
            rect: Rect {
                x: area.x,
                y: area.y + visible_idx as u16,
                width: area.width,
                height: 1,
            },
            tmux_session: row.tmux_session.clone(),
        });
    }

    frame.render_widget(Paragraph::new(lines), area);
}

pub fn draw_sessions_divider(frame: &mut Frame, state: &AppState, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let line: String = std::iter::repeat_n(DIVIDER_CHAR, area.width as usize).collect();
    let span = Span::styled(line, Style::default().fg(state.theme.border_inactive));
    frame.render_widget(Paragraph::new(Line::from(span)), area);
}
