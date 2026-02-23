use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::theme::ThemePalette;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
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
        Span::styled("●", theme.style_success())
    } else {
        Span::styled("○", theme.style_error())
    };

    let title = Line::from(vec![
        Span::styled(" ✦ ", theme.style_accent_bold()),
        Span::styled("aletheia", theme.style_accent()),
        Span::styled(" │ ", theme.style_dim()),
        Span::styled(agent_name, theme.style_fg()),
        Span::raw(" "),
        sse_indicator,
    ]);

    let bar = Paragraph::new(title).style(theme.style_surface());
    frame.render_widget(bar, area);
}
