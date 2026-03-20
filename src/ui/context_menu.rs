use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use crate::systemd::commands::ServiceAction;
use crate::ui::panes::PaneId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    ServiceAction(ServiceAction),
    SplitNewPaneHorizontal,
    SplitNewPaneVertical,
    SplitHorizontal,
    SplitVertical,
    ClosePane,
}

#[derive(Debug, Clone)]
pub enum ContextMenuTarget {
    SidebarService { unit_name: String },
    Pane { pane_id: PaneId },
}

#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    pub label: String,
    pub action: ContextMenuAction,
}

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub x: u16,
    pub y: u16,
    pub items: Vec<ContextMenuItem>,
    pub selected_index: usize,
    pub target: ContextMenuTarget,
}

pub fn compute_menu_rect(
    x: u16,
    y: u16,
    item_count: usize,
    max_label_width: usize,
    frame_size: Rect,
) -> Rect {
    let width = (max_label_width as u16 + 4).min(frame_size.width);
    let height = (item_count as u16 + 2).min(frame_size.height);

    let x = if x + width > frame_size.x + frame_size.width {
        (frame_size.x + frame_size.width).saturating_sub(width)
    } else {
        x
    };
    let y = if y + height > frame_size.y + frame_size.height {
        (frame_size.y + frame_size.height).saturating_sub(height)
    } else {
        y
    };

    Rect::new(x, y, width, height)
}

pub fn render_context_menu(frame: &mut Frame, menu: &ContextMenu, frame_size: Rect) {
    let max_label_width = menu.items.iter().map(|i| i.label.len()).max().unwrap_or(0);
    let rect = compute_menu_rect(
        menu.x,
        menu.y,
        menu.items.len(),
        max_label_width,
        frame_size,
    );

    frame.render_widget(Clear, rect);

    let items: Vec<ListItem> = menu
        .items
        .iter()
        .map(|item| ListItem::new(format!(" {}", item.label)))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = ListState::default();
    state.select(Some(menu.selected_index));

    frame.render_stateful_widget(list, rect, &mut state);
}
