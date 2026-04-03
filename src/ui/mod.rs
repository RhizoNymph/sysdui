pub mod confirm;
pub mod context_menu;
pub mod detail;
pub mod help;
pub mod logs;
pub mod panes;
pub mod search;
pub mod sidebar;

use ratatui::prelude::*;
use ratatui::widgets::ListState;

use crate::app::App;
use crate::app::InputMode;
use crate::ui::panes::PaneId;

#[derive(Default)]
pub struct LayoutCache {
    pub sidebar_area: Rect,
    pub detail_area: Rect,
    pub pane_rects: Vec<(PaneId, Rect)>,
    pub status_line_area: Rect,
    pub sidebar_scroll_offset: usize,
    pub frame_size: Rect,
}


pub fn render(app: &App, sidebar_state: &mut ListState, frame: &mut Frame) -> LayoutCache {
    let size = frame.area();

    // Main layout: sidebar | main panel, with help bar at bottom
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // main content
            Constraint::Length(2), // help bar
        ])
        .split(size);

    let main_area = outer[0];
    let help_area = outer[1];

    // Search bar at top if searching
    let (search_area, content_area) = if matches!(
        app.input_mode,
        InputMode::SearchServices | InputMode::SearchLogs
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(main_area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, main_area)
    };

    // Sidebar + main panel split
    let sidebar_width = 35u16.min(content_area.width / 3);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_width), Constraint::Min(1)])
        .split(content_area);

    let sidebar_area = columns[0];
    let panel_area = columns[1];

    // Render search bar if active
    if let Some(area) = search_area {
        let label = match app.input_mode {
            InputMode::SearchServices => " Search Services ",
            InputMode::SearchLogs => " Search Logs ",
            _ => " Search ",
        };
        let sb = search::SearchBar::new(&app.search_query, label);
        frame.render_widget(sb, area);
    }

    // Render sidebar
    let sidebar_focused = matches!(app.input_mode, InputMode::Normal);
    sidebar::render_sidebar(
        frame,
        sidebar_area,
        &sidebar::SidebarParams {
            units: &app.filtered_units,
            selected_index: app.selected_index,
            focused: sidebar_focused,
            include_list: &app.config.filter.include,
            list_mode: app.list_mode,
        },
        sidebar_state,
    );

    // Split main panel: detail (top) + logs (bottom)
    let panel_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9), // detail panel
            Constraint::Min(1),    // log panel(s)
        ])
        .split(panel_area);

    let detail_area = panel_chunks[0];
    let log_area = panel_chunks[1];

    // Render detail panel
    let selected_unit = app.selected_unit();
    let selected_detail = selected_unit.and_then(|u| app.unit_details.get(&u.name));
    detail::render_detail(frame, detail_area, selected_unit, selected_detail);

    // Render pane layout for logs
    let pane_layouts = app.pane_tree.layout(log_area);
    for (pane_id, rect) in &pane_layouts {
        if let Some(pane) = app.pane_tree.get_leaf(*pane_id) {
            let is_focused = *pane_id == app.focused_pane;
            logs::render_log_pane(frame, *rect, pane, is_focused);
        }
    }

    // Render status + help bar
    let help_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(help_area);

    let status_line_area = help_chunks[0];

    // Status line
    let mode_label = match app.list_mode {
        crate::app::ListMode::Include => {
            format!(
                "{} ({})",
                app.list_mode.label(),
                app.config.filter.include.len()
            )
        }
        crate::app::ListMode::Exclude => {
            format!(
                "{} ({})",
                app.list_mode.label(),
                app.config.filter.exclude.len()
            )
        }
        crate::app::ListMode::All => app.list_mode.label().to_string(),
    };
    let status_spans = vec![
        Span::styled(" [Scope: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.filter_mode.label(), Style::default().fg(Color::Cyan)),
        Span::styled("] [Status: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.status_filter.label(), Style::default().fg(Color::Cyan)),
        Span::styled("] [Mode: ", Style::default().fg(Color::DarkGray)),
        Span::styled(mode_label, Style::default().fg(Color::Cyan)),
        Span::styled("] [Sort: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.sort_mode.label(), Style::default().fg(Color::Cyan)),
        Span::styled("] [Disabled: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if app.show_disabled { "Show" } else { "Hide" },
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("]", Style::default().fg(Color::DarkGray)),
    ];
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(status_spans)),
        status_line_area,
    );

    let is_search = matches!(
        app.input_mode,
        InputMode::SearchServices | InputMode::SearchLogs
    );
    help::render_help_bar(frame, help_chunks[1], &app.config.keys, is_search);

    // Render overlays
    match &app.input_mode {
        InputMode::Confirm => {
            if let Some(dialog) = &app.confirm_dialog {
                dialog.render(frame, size);
            }
        }
        InputMode::Help => {
            help::render_help_overlay(frame, size, &app.config.keys);
        }
        InputMode::SplitPrompt => {
            render_split_prompt(frame, size);
        }
        InputMode::ContextMenu => {
            if let Some(menu) = &app.context_menu {
                context_menu::render_context_menu(frame, menu, size);
            }
        }
        _ => {}
    }

    LayoutCache {
        sidebar_area,
        detail_area,
        pane_rects: pane_layouts,
        status_line_area,
        sidebar_scroll_offset: sidebar_state.offset(),
        frame_size: size,
    }
}

fn render_split_prompt(frame: &mut Frame, area: Rect) {
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    let width = 40u16.min(area.width.saturating_sub(4));
    let height = 3u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Split ");

    let p = Paragraph::new("[h]orizontal / [v]ertical")
        .block(block)
        .alignment(Alignment::Center);

    frame.render_widget(p, dialog_area);
}
