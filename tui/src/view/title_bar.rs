use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let agent_name = app
        .focused_agent
        .as_ref()
        .and_then(|id| app.agents.iter().find(|a| a.id == *id))
        .map(|a| {
            let emoji = a.emoji.as_deref().unwrap_or("");
            if emoji.is_empty() {
                a.name.clone()
            } else {
                format!("{} {}", emoji, a.name)
            }
        })
        .unwrap_or_else(|| "no agent".to_string());

    let sse_indicator = if app.sse_connected {
        Span::styled("●", Style::default().fg(Color::Green))
    } else {
        Span::styled("○", Style::default().fg(Color::Red))
    };

    let cost = format!("${:.2} today", app.daily_cost_cents as f64 / 100.0);

    let title = Line::from(vec![
        Span::styled(" Aletheia", Style::default().fg(Color::Cyan)),
        Span::raw(" │ "),
        Span::styled(agent_name, Style::default().fg(Color::White)),
        Span::raw(" "),
        sse_indicator,
        // Right-align cost — fill with spaces
        Span::raw(" ".repeat(
            area.width
                .saturating_sub(30 + cost.len() as u16)
                .into(),
        )),
        Span::styled(cost, Style::default().fg(Color::Yellow)),
        Span::raw(" "),
    ]);

    let bar = Paragraph::new(title).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(bar, area);
}
