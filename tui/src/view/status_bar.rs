use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{AgentStatus, App};
use crate::theme::{self, ThemePalette};

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let mut spans: Vec<Span> = vec![Span::raw(" ")];

    let working_agents: Vec<_> = app
        .agents
        .iter()
        .filter(|a| a.status != AgentStatus::Idle)
        .collect();

    if working_agents.is_empty() {
        spans.push(Span::styled("idle", theme.style_dim()));
    } else {
        for (i, agent) in working_agents.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" │ ", theme.style_dim()));
            }

            let ch = theme::spinner_frame(app.tick_count);

            let status_str = match agent.status {
                AgentStatus::Working => {
                    if let Some(ref tool) = agent.active_tool {
                        let elapsed = agent
                            .tool_started_at
                            .map(|t| format!("{:.1}s", t.elapsed().as_secs_f32()))
                            .unwrap_or_default();
                        format!("{} {}: {} ({})", ch, agent.name, tool, elapsed)
                    } else {
                        format!("{} {}: working", ch, agent.name)
                    }
                }
                AgentStatus::Streaming => format!("● {}: streaming", agent.name),
                AgentStatus::Compacting => {
                    let stage = agent.compaction_stage.as_deref().unwrap_or("...");
                    format!("{} {}: compacting ({})", ch, agent.name, stage)
                }
                AgentStatus::Idle => unreachable!(),
            };

            let color = match agent.status {
                AgentStatus::Working => theme.spinner,
                AgentStatus::Streaming => theme.streaming,
                AgentStatus::Compacting => theme.compacting,
                AgentStatus::Idle => unreachable!(),
            };

            spans.push(Span::styled(status_str, Style::default().fg(color)));
        }
    }

    // Right-align keybinding hints
    let hints = "^A agents │ ^F sidebar │ ^Q quit";
    let used_width: usize = spans.iter().map(|s| s.content.len()).sum();
    let remaining = area.width as usize - used_width.min(area.width as usize);
    if remaining > hints.len() + 2 {
        spans.push(Span::raw(" ".repeat(remaining - hints.len() - 1)));
        spans.push(Span::styled(hints, theme.style_dim()));
    }

    let bar = Paragraph::new(Line::from(spans)).style(theme.style_surface());
    frame.render_widget(bar, area);
}
