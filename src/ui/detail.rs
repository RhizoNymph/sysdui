use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::systemd::types::{ActiveState, ServiceDetail, UnitInfo};

pub fn render_detail(
    frame: &mut Frame,
    area: Rect,
    unit: Option<&UnitInfo>,
    detail: Option<&ServiceDetail>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Detail ");

    let Some(unit) = unit else {
        let p = Paragraph::new("No service selected").block(block);
        frame.render_widget(p, area);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    // Service name header
    lines.push(Line::from(vec![
        Span::styled("Service: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &unit.name,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(detail) = detail {
        // Status line
        let state_color = match ActiveState::from_str(&detail.active_state) {
            ActiveState::Active => Color::Green,
            ActiveState::Failed => Color::Red,
            ActiveState::Activating | ActiveState::Deactivating => Color::Yellow,
            _ => Color::DarkGray,
        };

        let status_text = format!("{} ({})", detail.active_state, detail.sub_state);
        let mut status_parts = vec![
            Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(status_text, Style::default().fg(state_color)),
        ];

        if detail.main_pid > 0 {
            status_parts.push(Span::raw("   "));
            status_parts.push(Span::styled("PID: ", Style::default().fg(Color::DarkGray)));
            status_parts.push(Span::raw(detail.main_pid.to_string()));
        }

        lines.push(Line::from(status_parts));

        // Memory + Uptime
        let mut info_parts = vec![
            Span::styled("Memory: ", Style::default().fg(Color::DarkGray)),
            Span::raw(detail.memory_human()),
        ];
        info_parts.push(Span::raw("   "));
        info_parts.push(Span::styled("Up: ", Style::default().fg(Color::DarkGray)));
        info_parts.push(Span::raw(detail.uptime_human()));
        lines.push(Line::from(info_parts));

        // Fragment path
        if !detail.fragment_path.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("File: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&detail.fragment_path),
            ]));
        }

        // Enabled state
        if !detail.unit_file_state.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Enabled: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&detail.unit_file_state),
            ]));
        }

        // Dependencies (compact)
        let deps: Vec<&str> = detail
            .requires
            .iter()
            .chain(detail.wants.iter())
            .map(|s| s.as_str())
            .take(5)
            .collect();
        if !deps.is_empty() {
            let deps_str = deps.join(", ");
            let suffix = if detail.requires.len() + detail.wants.len() > 5 {
                ", ..."
            } else {
                ""
            };
            lines.push(Line::from(vec![
                Span::styled("Deps: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{deps_str}{suffix}")),
            ]));
        }

        // Description
        if !detail.description.is_empty() {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                &detail.description,
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Loading...",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let p = Paragraph::new(lines).block(block);
    frame.render_widget(p, area);
}
