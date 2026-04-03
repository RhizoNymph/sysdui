use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::config::keys::{KeyAction, KeyBindings, format_key_event};

/// Render the bottom help bar with context-sensitive hints.
pub fn render_help_bar(frame: &mut Frame, area: Rect, bindings: &KeyBindings, is_search: bool) {
    if is_search {
        let hints = vec![
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(":confirm "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(":cancel "),
        ];
        let p = Paragraph::new(Line::from(hints));
        frame.render_widget(p, area);
        return;
    }

    // Two groups: service actions | views & panes
    let service_hints: &[(KeyAction, &str)] = &[
        (KeyAction::Start, "start"),
        (KeyAction::Restart, "restart"),
        (KeyAction::Stop, "stop"),
        (KeyAction::Enable, "enable"),
        (KeyAction::Disable, "disable"),
        (KeyAction::DaemonReload, "reload"),
        (KeyAction::EditUnit, "edit"),
    ];

    let view_hints: &[(KeyAction, &str)] = &[
        (KeyAction::SearchServices, "search"),
        (KeyAction::CycleFilter, "scope"),
        (KeyAction::CycleStatusFilter, "status"),
        (KeyAction::ToggleDisabled, "disabled"),
        (KeyAction::ToggleListMode, "mode"),
        (KeyAction::ToggleInclude, "+incl"),
        (KeyAction::ToggleExclude, "-excl"),
        (KeyAction::CycleSort, "sort"),
        (KeyAction::CycleLogLevel, "log"),
        (KeyAction::PinPane, "split"),
        (KeyAction::ClosePane, "close"),
        (KeyAction::CycleFocus, "focus"),
        (KeyAction::ShowHelp, "help"),
    ];

    let mut spans = Vec::new();

    for (i, (action, label)) in service_hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        push_hint(&mut spans, bindings, *action, label);
    }

    // Separator between groups
    spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));

    for (i, (action, label)) in view_hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        push_hint(&mut spans, bindings, *action, label);
    }

    let p = Paragraph::new(Line::from(spans));
    frame.render_widget(p, area);
}

fn push_hint(
    spans: &mut Vec<Span<'static>>,
    bindings: &KeyBindings,
    action: KeyAction,
    label: &str,
) {
    let key_str = bindings
        .action_to_key(&action)
        .map(format_key_event)
        .unwrap_or_else(|| "?".to_string());
    spans.push(Span::styled(
        format!("[{key_str}]"),
        Style::default().fg(Color::Yellow),
    ));
    spans.push(Span::raw(label.to_string()));
}

/// Render the full-screen help overlay.
pub fn render_help_overlay(frame: &mut Frame, area: Rect, bindings: &KeyBindings) {
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 30u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Keybindings (press any key to close) ");

    let mut lines: Vec<Line> = Vec::new();

    // Group bindings by category for readability
    let categories: &[(&str, &[KeyAction])] = &[
        (
            "Navigation",
            &[
                KeyAction::NavigateUp,
                KeyAction::NavigateDown,
                KeyAction::Select,
                KeyAction::PageUp,
                KeyAction::PageDown,
            ],
        ),
        (
            "Service Actions",
            &[
                KeyAction::Start,
                KeyAction::Restart,
                KeyAction::Stop,
                KeyAction::Enable,
                KeyAction::Disable,
                KeyAction::DaemonReload,
                KeyAction::EditUnit,
            ],
        ),
        (
            "Search & Filter",
            &[
                KeyAction::SearchServices,
                KeyAction::SearchLogs,
                KeyAction::CycleFilter,
                KeyAction::CycleStatusFilter,
                KeyAction::ToggleDisabled,
                KeyAction::ToggleListMode,
                KeyAction::ToggleInclude,
                KeyAction::ToggleExclude,
                KeyAction::CycleSort,
                KeyAction::CycleLogLevel,
            ],
        ),
        (
            "Panes",
            &[
                KeyAction::PinPane,
                KeyAction::ClosePane,
                KeyAction::CycleFocus,
            ],
        ),
        (
            "General",
            &[KeyAction::Quit, KeyAction::ShowHelp, KeyAction::Escape],
        ),
    ];

    for (cat_name, actions) in categories {
        lines.push(Line::from(Span::styled(
            format!("  {cat_name}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for action in *actions {
            if let Some(key) = bindings.action_to_key(action) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{:>12}", format_key_event(key)),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw("  "),
                    Span::raw(action.label()),
                ]));
            }
        }
        lines.push(Line::default());
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(p, overlay_area);
}
