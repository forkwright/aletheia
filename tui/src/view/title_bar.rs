use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

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

    let mut spans = vec![
        Span::styled(" ✦ ", theme.style_accent_bold()),
        Span::styled("aletheia", theme.style_accent()),
    ];

    // Breadcrumbs — show navigation path when not at Home
    if !app.view_stack.is_home() {
        let breadcrumbs = app.view_stack.breadcrumbs();
        let last_idx = breadcrumbs.len() - 1;

        spans.push(Span::styled(" │ ", theme.style_dim()));

        for (i, crumb) in breadcrumbs.iter().enumerate() {
            if i == last_idx {
                // Current view — bold
                spans.push(Span::styled(
                    crumb.to_string(),
                    theme
                        .style_fg()
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                // Parent views — dim
                spans.push(Span::styled(crumb.to_string(), theme.style_dim()));
                spans.push(Span::styled(" > ", theme.style_dim()));
            }
        }
    } else {
        spans.push(Span::styled(" │ ", theme.style_dim()));
        spans.push(Span::styled(agent_name, theme.style_fg()));
    }

    spans.push(Span::raw(" "));
    spans.push(sse_indicator);

    let title = Line::from(spans);
    let bar = Paragraph::new(title).style(theme.style_surface());
    frame.render_widget(bar, area);
}
