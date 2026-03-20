use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::systemd::commands::ServiceAction;

pub struct ConfirmDialog {
    pub action: ServiceAction,
    pub unit_name: String,
}

impl ConfirmDialog {
    pub fn new(action: ServiceAction, unit_name: String) -> Self {
        Self { action, unit_name }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        // Center the dialog
        let width = 50u16.min(area.width.saturating_sub(4));
        let height = 5u16.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(x, y, width, height);

        // Clear the area behind the dialog
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Confirm ");

        let msg = if self.action.needs_unit() {
            format!(
                "{} {}?\n\n[y]es / any key to cancel",
                self.action.label(),
                self.unit_name
            )
        } else {
            format!("{}?\n\n[y]es / any key to cancel", self.action.label())
        };

        let p = Paragraph::new(msg)
            .block(block)
            .alignment(Alignment::Center);

        frame.render_widget(p, dialog_area);
    }
}
