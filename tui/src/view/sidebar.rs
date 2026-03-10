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

    // Small top padding
    lines.push(Line::raw(""));

    for agent in &app.agents {
        let is_focused = app.focused_agent.as_ref() == Some(&agent.id);

        let status_icon = match agent.status {
            AgentStatus::Idle => Span::styled("○", theme.style_dim()),
            AgentStatus::Working => {
                let ch = theme::spinner_frame(app.tick_count);
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

        // Build the line: indicator + name + notification dot
        let mut spans = vec![
            Span::raw("  "),
            status_icon,
            Span::raw(" "),
            Span::styled(name, name_style),
        ];

        // Notification dot — shows when an unfocused agent completed a turn
        if !is_focused && agent.has_notification {
            spans.push(Span::styled(" ●", Style::default().fg(theme.colors.accent)));
        }

        lines.push(Line::from(spans));

        // Show active tool under working agents
        if let Some(ref tool) = agent.active_tool {
            let elapsed = agent
                .tool_started_at
                .map(|t| t.elapsed().as_secs_f32())
                .unwrap_or(0.0);
            let ch = theme::spinner_frame(app.tick_count);
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    format!("{} {} {:.1}s", ch, tool, elapsed),
                    theme.style_muted(),
                ),
            ]));
        }

        // Show compaction stage
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
