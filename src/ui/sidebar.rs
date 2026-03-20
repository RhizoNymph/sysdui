use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::systemd::types::{ActiveState, UnitInfo};

pub fn render_sidebar(
    frame: &mut Frame,
    area: Rect,
    units: &[UnitInfo],
    selected_index: usize,
    focused: bool,
) {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Services ");

    let items: Vec<ListItem> = units
        .iter()
        .map(|unit| {
            let (icon, color) = match unit.active_state {
                ActiveState::Active => ("●", Color::Green),
                ActiveState::Failed => ("✗", Color::Red),
                ActiveState::Activating | ActiveState::Deactivating | ActiveState::Reloading => {
                    ("◎", Color::Yellow)
                }
                _ => ("○", Color::DarkGray),
            };

            let name = unit.short_name();
            let text = format!("{icon} {name}");
            ListItem::new(text).style(Style::default().fg(color))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    if !units.is_empty() {
        state.select(Some(selected_index));
    }

    frame.render_stateful_widget(list, area, &mut state);
}
