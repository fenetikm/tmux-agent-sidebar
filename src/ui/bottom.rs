mod activity;
mod git;
mod panel;

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::state::{AppState, BottomTab, BottomTabTarget, Focus};

use super::text::display_width;

fn render_centered(frame: &mut Frame, area: Rect, text: &str, color: Color) {
    // Vertically center: pad with empty lines above
    let top_pad = area.height.saturating_sub(1) / 2;
    let mut lines: Vec<Line<'_>> = Vec::new();
    for _ in 0..top_pad {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(text, Style::default().fg(color))));
    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

pub fn draw_bottom(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let theme = &state.theme;
    let border_color = if state.focus_state.focus == Focus::ActivityLog {
        theme.accent
    } else {
        theme.border_inactive
    };

    let (tab_title, tab_targets) = build_tab_title(state);
    state.layout.bottom_tab_targets = tab_targets
        .into_iter()
        .map(|mut t| {
            t.start += area.x;
            t.end += area.x;
            t
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let title_spans = tab_title.spans;
    let title_dw = title_spans
        .iter()
        .map(|span| display_width(&span.content))
        .sum::<usize>();
    let top_fill_len = (area.width as usize).saturating_sub(title_dw + 4);
    let mut top_line_spans = vec![Span::styled("╭ ", Style::default().fg(border_color))];
    top_line_spans.extend(title_spans);
    top_line_spans.push(Span::styled(
        format!(" {}╮", "─".repeat(top_fill_len)),
        Style::default().fg(border_color),
    ));
    let top_line = Line::from(top_line_spans);
    let top_rect = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(Paragraph::new(top_line), top_rect);

    let bottom_line = Line::from(Span::styled(
        format!("╰{}╯", "─".repeat((area.width as usize).saturating_sub(2))),
        Style::default().fg(border_color),
    ));
    let bottom_rect = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    frame.render_widget(Paragraph::new(bottom_line), bottom_rect);

    match state.bottom_tab {
        BottomTab::Activity => activity::draw_activity_content(frame, state, inner),
        BottomTab::GitStatus => git::draw_git_content(frame, state, inner),
        BottomTab::Panel => panel::draw_panel_content(frame, state, inner),
    }
}

/// Build the bottom panel's tab title, together with the click region of
/// each title. The panel tab's name comes from config, so the ranges are
/// computed here rather than hardcoded.
///
/// Layout is `╭ Activity │ Git │ <name> ─…╮`, so the first title starts at
/// column 2: one for the `╭` and one for the space after it.
pub fn build_tab_title(state: &AppState) -> (Line<'static>, Vec<BottomTabTarget>) {
    let theme = &state.theme;
    let sep_style = Style::default().fg(theme.border_inactive);

    let titles: Vec<(String, BottomTab)> = state
        .enabled_bottom_tabs()
        .into_iter()
        .map(|tab| {
            let label = match tab {
                BottomTab::Activity => "Activity".to_string(),
                BottomTab::GitStatus => "Git".to_string(),
                // `enabled_bottom_tabs` only yields Panel when the config is
                // present, so the fallback here is unreachable in practice.
                BottomTab::Panel => state
                    .panel_config
                    .as_ref()
                    .map(|config| config.name.clone())
                    .unwrap_or_default(),
            };
            (label, tab)
        })
        .collect();

    // Column 0 is the `╭` border, column 1 the space after it.
    const TITLE_START_COL: u16 = 2;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut targets: Vec<BottomTabTarget> = Vec::new();
    let mut col = TITLE_START_COL;

    for (index, (label, tab)) in titles.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" \u{2502} ", sep_style));
            col += 3;
        }
        let width = display_width(&label) as u16;
        let style = if state.bottom_tab == tab {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.text_muted)
        };
        spans.push(Span::styled(label, style));
        targets.push(BottomTabTarget {
            start: col,
            end: col + width,
            tab,
        });
        col += width;
    }

    (Line::from(spans), targets)
}

#[cfg(test)]
mod tests {
    use crate::ui::text::truncate_to_width;

    #[test]
    fn truncate_to_width_short() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn truncate_to_width_exact() {
        assert_eq!(truncate_to_width("hello", 5), "hello");
    }

    #[test]
    fn truncate_to_width_truncated() {
        let result = truncate_to_width("hello world", 8);
        assert!(result.ends_with('…'));
        assert!(result.len() <= 10); // 7 chars + ellipsis in bytes
    }
}
