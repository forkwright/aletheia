use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{AgentStatus, App};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    for agent in &app.agents {
        let is_focused = app.focused_agent.as_deref() == Some(&agent.id);

        let status_icon = match agent.status {
            AgentStatus::Idle => Span::styled("○", Style::default().fg(Color::DarkGray)),
            AgentStatus::Working => {
                let spinners = ['◐', '◓', '◑', '◒'];
                let idx = (app.tick_count / 4) as usize % spinners.len();
                Span::styled(
                    spinners[idx].to_string(),
                    Style::default().fg(Color::Yellow),
                )
            }
            AgentStatus::Streaming => Span::styled("●", Style::default().fg(Color::Green)),
            AgentStatus::Compacting => Span::styled("◉", Style::default().fg(Color::Magenta)),
        };

        let name_style = if is_focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let emoji = agent.emoji.as_deref().unwrap_or("");
        let name = if emoji.is_empty() {
            agent.name.clone()
        } else {
            format!("{}{}", emoji, agent.name)
        };

        lines.push(Line::from(vec![
            Span::raw(" "),
            status_icon,
            Span::raw(" "),
            Span::styled(name, name_style),
        ]));

        // Show active tool under working agents
        if let Some(ref tool) = agent.active_tool {
            let elapsed = agent
                .tool_started_at
                .map(|t| t.elapsed().as_secs_f32())
                .unwrap_or(0.0);
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    format!("⚙ {} ({:.1}s)", tool, elapsed),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }

        // Show compaction stage
        if let Some(ref stage) = agent.compaction_stage {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    format!("↻ {}", stage),
                    Style::default().fg(Color::Magenta),
                ),
            ]));
        }
    }

    let block = Block::default()
        .title(" Agents ")
        .borders(Borders::RIGHT);

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
