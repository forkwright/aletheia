use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{AgentStatus, App};
use crate::theme::{self, Theme};

pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
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

        match agent.status {
            AgentStatus::Streaming => {
                let elapsed_secs = if is_focused {
                    app.layout.ops.turn_started_at.map(|t| t.elapsed().as_secs())
                } else {
                    None
                };
                let label = if let Some(secs) = elapsed_secs {
                    format!("thinking… {:02}:{:02}", secs / 60, secs % 60)
                } else {
                    "thinking…".to_string()
                };
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(label, theme.style_muted()),
                ]));
            }
            AgentStatus::Working => {
                if let Some(ref tool) = agent.active_tool {
                    let secs = tool.started_at.elapsed().as_secs();
                    let elapsed_str = format!("{:02}:{:02}", secs / 60, secs % 60);
                    let label = tool_status_label(&tool.name);
                    lines.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled(
                            format!("{label} {elapsed_str}"),
                            theme.style_muted(),
                        ),
                    ]));
                }
            }
            AgentStatus::Idle | AgentStatus::Compacting => {}
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

fn tool_status_label(name: &str) -> &'static str {
    match name {
        "Read" | "read_file" | "Glob" | "Grep" => "reading files…",
        "Write" | "write_file" | "Edit" | "edit_file" | "NotebookEdit" => "writing…",
        "Bash" | "bash" | "exec" => "running command…",
        "WebSearch" | "web_search" => "searching…",
        "WebFetch" | "web_fetch" => "fetching…",
        _ => "working…",
    }
}
