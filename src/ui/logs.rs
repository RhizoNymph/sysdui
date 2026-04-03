use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::journal::filter::find_matches;
use crate::ui::panes::PaneLeaf;

pub fn render_log_pane(
    frame: &mut Frame,
    area: Rect,
    pane: &PaneLeaf,
    focused: bool,
) {
    let border_color = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title = format!(
        " Logs: {} [{}] ",
        pane.service_name,
        pane.priority_filter
    );

    let follow_indicator = if pane.is_following() {
        " [LIVE] "
    } else {
        &format!(" [+{} new] ", pane.scroll_offset)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title)
        .title_bottom(Line::from(follow_indicator).right_aligned());

    let inner = block.inner(area);
    let visible_height = inner.height as usize;
    let total_lines = pane.log_buffer.len();

    // Calculate which lines to show
    let start = if pane.scroll_offset == 0 {
        // Following: show last N lines
        total_lines.saturating_sub(visible_height)
    } else {
        // Scrolled back: show from offset
        total_lines
            .saturating_sub(visible_height)
            .saturating_sub(pane.scroll_offset)
    };

    let end = (start + visible_height).min(total_lines);

    let lines: Vec<Line> = pane
        .log_buffer
        .iter()
        .skip(start)
        .take(end - start)
        .map(|log_line| {
            if pane.search_query.is_empty() {
                Line::from(Span::raw(log_line.as_str()))
            } else {
                // Highlight search matches
                let matches = find_matches(log_line, &pane.search_query);
                if matches.is_empty() {
                    Line::from(Span::raw(log_line.as_str()))
                } else {
                    let mut spans = Vec::new();
                    let mut last_end = 0;
                    for (ms, me) in &matches {
                        if *ms > last_end {
                            spans.push(Span::raw(&log_line[last_end..*ms]));
                        }
                        spans.push(Span::styled(
                            &log_line[*ms..*me],
                            Style::default().bg(Color::Yellow).fg(Color::Black),
                        ));
                        last_end = *me;
                    }
                    if last_end < log_line.len() {
                        spans.push(Span::raw(&log_line[last_end..]));
                    }
                    Line::from(spans)
                }
            }
        })
        .collect();

    let p = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(p, area);

    // Scrollbar
    if total_lines > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(start)
            .viewport_content_length(visible_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}
