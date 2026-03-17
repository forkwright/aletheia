use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{AgentStatus, App};
use crate::theme::{self, Theme};

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::raw(""));

    if app.dashboard.agents.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("no agents", theme.style_dim()),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("waiting for connection…", theme.style_muted()),
        ]));
    }

    for agent in &app.dashboard.agents {
        let is_focused = app.dashboard.focused_agent.as_ref() == Some(&agent.id);

        let status_icon = match agent.status {
            AgentStatus::Idle => Span::styled("○", theme.style_dim()),
            AgentStatus::Working => {
                let ch = theme::spinner_frame(app.viewport.tick_count);
                Span::styled(ch.to_string(), Style::default().fg(theme.status.spinner))
            }
            AgentStatus::Streaming => {
                Span::styled("●", Style::default().fg(theme.status.streaming))
            }
            AgentStatus::Compacting => {
                Span::styled("◉", Style::default().fg(theme.status.compacting))
            }
        };

        let name_style = if is_focused {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text.fg)
        };

        let emoji = agent.emoji.as_deref().unwrap_or("");
        let name = if emoji.is_empty() {
            agent.name.clone()
        } else {
            format!("{} {}", emoji, agent.name)
        };

        let mut spans = vec![
            Span::raw("  "),
            status_icon,
            Span::raw(" "),
            Span::styled(name, name_style),
        ];

        // NOTE: shown only for unfocused agents: clears when the user switches focus
        if !is_focused && agent.unread_count > 0 {
            let badge = if agent.unread_count > 9 {
                " 9+".to_string()
            } else {
                format!(" {}", agent.unread_count)
            };
            spans.push(Span::styled(
                badge,
                Style::default().fg(theme.colors.accent),
            ));
        }

        lines.push(Line::from(spans));

        if let Some(ref tool) = agent.active_tool {
            let elapsed = tool.started_at.elapsed().as_secs_f32();
            let ch = theme::spinner_frame(app.viewport.tick_count);
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    format!("{} {} {:.1}s", ch, tool.name, elapsed),
                    theme.style_muted(),
                ),
            ]));
        }

        if let Some(ref stage) = agent.compaction_stage {
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    format!("↻ {}", stage),
                    Style::default().fg(theme.status.compacting),
                ),
            ]));
        }
    }

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_set(symbols::border::PLAIN)
        .border_style(Style::default().fg(theme.borders.separator));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
