use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{AgentStatus, App};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let mut spans: Vec<Span> = vec![Span::raw(" ")];

    let working_agents: Vec<_> = app
        .agents
        .iter()
        .filter(|a| a.status != AgentStatus::Idle)
        .collect();

    if working_agents.is_empty() {
        spans.push(Span::styled(
            "all idle",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        for (i, agent) in working_agents.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
            }

            let status_str = match agent.status {
                AgentStatus::Working => {
                    if let Some(ref tool) = agent.active_tool {
                        let elapsed = agent
                            .tool_started_at
                            .map(|t| format!("{:.1}s", t.elapsed().as_secs_f32()))
                            .unwrap_or_default();
                        format!("⚙ {}: {} ({})", agent.name, tool, elapsed)
                    } else {
                        format!("◐ {}: working", agent.name)
                    }
                }
                AgentStatus::Streaming => format!("● {}: streaming", agent.name),
                AgentStatus::Compacting => {
                    let stage = agent.compaction_stage.as_deref().unwrap_or("...");
                    format!("◉ {}: compacting ({})", agent.name, stage)
                }
                AgentStatus::Idle => unreachable!(),
            };

            let color = match agent.status {
                AgentStatus::Working => Color::Yellow,
                AgentStatus::Streaming => Color::Green,
                AgentStatus::Compacting => Color::Magenta,
                AgentStatus::Idle => unreachable!(),
            };

            spans.push(Span::styled(status_str, Style::default().fg(color)));
        }
    }

    // Right-align keybinding hints
    let hints = "Ctrl+A: agents │ Ctrl+F: sidebar │ Ctrl+Q: quit";
    let used_width: usize = spans.iter().map(|s| s.content.len()).sum();
    let remaining = area.width as usize - used_width.min(area.width as usize);
    if remaining > hints.len() + 2 {
        spans.push(Span::raw(" ".repeat(remaining - hints.len() - 1)));
        spans.push(Span::styled(hints, Style::default().fg(Color::DarkGray)));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(bar, area);
}
