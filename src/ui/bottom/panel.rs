use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::panel::{PanelColor, PanelRow};
use crate::state::{AppState, HyperlinkOverlay};
use crate::ui::colors::ColorTheme;
use crate::ui::text::{display_width, truncate_to_width};

/// Resolve a script-supplied colour name onto the active theme. Kept in the
/// UI layer so `src/panel.rs` stays free of ratatui types.
pub fn color_for(color: PanelColor, theme: &ColorTheme) -> Color {
    match color {
        PanelColor::Default => theme.text_active,
        PanelColor::Muted => theme.text_muted,
        PanelColor::Accent => theme.accent,
        PanelColor::Success => theme.status_running,
        PanelColor::Warning => theme.status_waiting,
        PanelColor::Danger => theme.status_error,
    }
}

/// Build one row's spans, returning them with the column offset and text of
/// any hyperlink so the caller can register an OSC 8 overlay.
fn row_line(
    row: &PanelRow,
    width: usize,
    theme: &ColorTheme,
) -> (Line<'static>, Option<(u16, String)>) {
    if row.heading {
        let text = truncate_to_width(&row.text, width);
        return (
            Line::from(Span::styled(text, Style::default().fg(theme.section_title))),
            None,
        );
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut text_col = 0usize;
    if let Some(ref icon) = row.icon {
        let icon_w = display_width(icon);
        spans.push(Span::styled(
            icon.clone(),
            Style::default().fg(color_for(row.icon_color, theme)),
        ));
        spans.push(Span::raw(" "));
        text_col = icon_w + 1;
    }

    let text = truncate_to_width(&row.text, width.saturating_sub(text_col));
    let link = row.url.as_ref().map(|_| (text_col as u16, text.clone()));
    spans.push(Span::styled(
        text,
        Style::default().fg(color_for(row.text_color, theme)),
    ));
    (Line::from(spans), link)
}

pub(super) fn draw_panel_content(frame: &mut Frame, state: &mut AppState, inner: Rect) {
    let theme = state.theme.clone();
    let width = inner.width as usize;

    let Some(data) = state.panel.clone() else {
        super::render_centered(frame, inner, "Loading…", theme.text_muted);
        return;
    };

    if data.rows.is_empty() && data.error.is_none() {
        super::render_centered(frame, inner, "No rows", theme.text_muted);
        return;
    }

    // Reserve the last line for the error footer when there is one.
    let error_height = u16::from(data.error.is_some());
    let rows_height = inner.height.saturating_sub(error_height);

    state.scrolls.panel.total_lines = data.rows.len();
    state.scrolls.panel.visible_height = rows_height as usize;
    let offset = state
        .scrolls
        .panel
        .offset
        .min(data.rows.len().saturating_sub(1));

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut overlays: Vec<HyperlinkOverlay> = Vec::new();
    for (index, row) in data
        .rows
        .iter()
        .skip(offset)
        .take(rows_height as usize)
        .enumerate()
    {
        let (line, link) = row_line(row, width, &theme);
        if let Some((col, text)) = link
            && let Some(ref url) = row.url
        {
            overlays.push(HyperlinkOverlay {
                x: inner.x + col,
                y: inner.y + index as u16,
                text,
                url: url.clone(),
            });
        }
        lines.push(line);
    }

    if rows_height > 0 {
        let rows_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: rows_height,
        };
        frame.render_widget(Paragraph::new(lines), rows_area);
    }

    state.layout.hyperlink_overlays.extend(overlays);

    if let Some(ref error) = data.error {
        let footer = Rect {
            x: inner.x,
            y: inner.y + rows_height,
            width: inner.width,
            height: 1,
        };
        let text = truncate_to_width(error, width);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(theme.status_error),
            ))),
            footer,
        );
    }
}
