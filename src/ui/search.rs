use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub struct SearchBar<'a> {
    pub query: &'a str,
    pub label: &'a str,
    pub cursor_pos: usize,
}

impl<'a> SearchBar<'a> {
    pub fn new(query: &'a str, label: &'a str) -> Self {
        Self {
            query,
            label,
            cursor_pos: query.len(),
        }
    }
}

impl Widget for SearchBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(self.label);

        let inner = block.inner(area);
        block.render(area, buf);

        let display_text = if self.query.is_empty() {
            Span::styled("type to search...", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw(self.query)
        };

        Paragraph::new(Line::from(display_text)).render(inner, buf);

        // Draw cursor
        if inner.width > 0 && inner.height > 0 {
            let cursor_x = inner.x + self.cursor_pos as u16;
            if cursor_x < inner.x + inner.width {
                buf[(cursor_x, inner.y)]
                    .set_style(Style::default().bg(Color::White).fg(Color::Black));
            }
        }
    }
}
