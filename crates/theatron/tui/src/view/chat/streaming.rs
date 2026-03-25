use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::markdown;
use crate::state::StreamPhase;
use crate::theme::{self, Theme};

use super::format_duration_adaptive;

pub(super) fn render_streaming(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &Theme,
    name: &str,
) {
    let phase = app.connection.stream_phase;

    // Phase-specific status indicator
    render_phase_indicator(app, lines, theme, name, phase);

    // Thinking block (if visible)
    if app.layout.thinking_expanded && !app.connection.streaming_thinking.is_empty() {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("─── thinking ", Style::default().fg(theme.thinking.border)),
            Span::styled(
                "─".repeat(inner_width.saturating_sub(16).min(40)),
                Style::default().fg(theme.thinking.border),
            ),
        ]));
        for line in app.connection.streaming_thinking.lines() {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(line.to_string(), Style::default().fg(theme.thinking.fg)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "─".repeat(inner_width.saturating_sub(4).min(40)),
                Style::default().fg(theme.thinking.border),
            ),
        ]));
    }

    // Streaming text: render complete lines via markdown, partial line as plain text
    if !app.connection.streaming_text.is_empty() {
        // Use cached markdown if text AND width match (streaming content, no OSC 8 links).
        let render_width = inner_width.saturating_sub(2);
        let rendered = if app.viewport.render.markdown_cache.text == app.connection.streaming_text
            && app.viewport.render.markdown_cache.width == render_width
        {
            app.viewport.render.markdown_cache.lines.clone()
        } else {
            markdown::render(
                &app.connection.streaming_text,
                render_width,
                theme,
                &app.highlighter,
            )
            .0
        };

        for line in rendered {
            let mut padded_spans = vec![Span::raw(" ")];
            padded_spans.extend(line.spans);
            lines.push(Line::from(padded_spans));
        }

        // Render the partial line buffer (not yet flushed to streaming_text)
        if !app.connection.streaming_line_buffer.is_empty() {
            // WHY: markdown strips trailing blank lines, so a \n\n paragraph break at
            // the end of streaming_text is invisible in the rendered output.  Re-insert
            // the blank line here so the gap is visible while the next paragraph is
            // still being typed into the buffer.
            if app.connection.streaming_text.ends_with("\n\n") {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    app.connection.streaming_line_buffer.clone(),
                    theme.style_fg(),
                ),
            ]));
        }

        // Braille cursor
        let ch = theme::spinner_frame(app.viewport.tick_count);
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(theme.status.streaming)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else if !app.connection.streaming_line_buffer.is_empty() {
        // Only partial line buffer (no complete lines yet)
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                app.connection.streaming_line_buffer.clone(),
                theme.style_fg(),
            ),
        ]));
        let ch = theme::spinner_frame(app.viewport.tick_count);
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(theme.status.streaming)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else if app.connection.active_turn_id.is_some() {
        // No text yet: show tool call phase lines or thinking indicator
        let tool_calls = &app.layout.ops.tool_calls;
        if tool_calls.is_empty() {
            let ch = theme::spinner_frame(app.viewport.tick_count);
            let label = match phase {
                StreamPhase::Requesting => "connecting…",
                StreamPhase::Thinking => "thinking…",
                StreamPhase::Compacting => "compacting context…",
                StreamPhase::Waiting => "waiting for approval…",
                _ => "thinking…",
            };
            lines.push(Line::from(vec![Span::styled(
                format!("  {} {label}", ch),
                theme.style_muted(),
            )]));
        } else {
            // Show last 5 tool calls as collapsible card lines
            let start = tool_calls.len().saturating_sub(5);
            for call in &tool_calls[start..] {
                let icon: String = match call.status {
                    crate::state::ops::OpsToolStatus::Running => {
                        theme::spinner_frame(app.viewport.tick_count).to_string()
                    }
                    crate::state::ops::OpsToolStatus::Complete => "✓".to_string(),
                    crate::state::ops::OpsToolStatus::Failed => "✗".to_string(),
                };
                let icon_style = match call.status {
                    crate::state::ops::OpsToolStatus::Running => Style::default()
                        .fg(theme.status.spinner)
                        .add_modifier(Modifier::BOLD),
                    crate::state::ops::OpsToolStatus::Complete => {
                        Style::default().fg(theme.status.success)
                    }
                    crate::state::ops::OpsToolStatus::Failed => {
                        Style::default().fg(theme.status.error)
                    }
                };
                let mut phase_text = call.name.clone();
                if let Some(ref arg) = call.primary_arg {
                    phase_text.push_str(&format!(" ({arg})"));
                }
                if let (Some(ms), false) = (
                    call.duration_ms,
                    matches!(call.status, crate::state::ops::OpsToolStatus::Running),
                ) {
                    phase_text.push_str(&format!(" · {}", format_duration_adaptive(ms)));
                }
                // Pulsing border for running tools
                let border_style =
                    if matches!(call.status, crate::state::ops::OpsToolStatus::Running) {
                        let pulse = (app.viewport.tick_count / 8).is_multiple_of(2);
                        if pulse {
                            Style::default().fg(theme.status.spinner)
                        } else {
                            theme.style_dim()
                        }
                    } else {
                        theme.style_dim()
                    };
                lines.push(Line::from(vec![
                    Span::styled("  │", border_style),
                    Span::raw(" "),
                    Span::styled(icon, icon_style),
                    Span::raw(" "),
                    Span::styled(phase_text, theme.style_muted()),
                ]));
            }
        }
    }
}

pub(super) fn render_queued_messages(app: &App, lines: &mut Vec<Line<'static>>, theme: &Theme) {
    for (i, queued) in app.interaction.queued_messages.iter().enumerate() {
        let badge = format!(" queued #{} ", i + 1);
        let preview = if queued.text.len() > 60 {
            format!("{}…", queued.text.get(..60).unwrap_or(&queued.text))
        } else {
            queued.text.clone()
        };
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                badge,
                Style::default()
                    .fg(theme.status.warning)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(preview, theme.style_dim()),
        ]));
    }
    // Hint for canceling
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Esc to cancel last queued", theme.style_muted()),
    ]));
    lines.push(Line::raw(""));
}

/// Render the stream phase indicator header.
fn render_phase_indicator(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    name: &str,
    phase: StreamPhase,
) {
    let has_text = !app.connection.streaming_text.is_empty()
        || !app.connection.streaming_line_buffer.is_empty();

    // Show agent name header for streaming response
    if has_text || app.connection.active_turn_id.is_some() {
        let phase_suffix = match phase {
            StreamPhase::Requesting => " · connecting",
            StreamPhase::Compacting => " · compacting",
            StreamPhase::Waiting => " · waiting",
            StreamPhase::Error => " · error",
            _ => "",
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {name}"), theme.style_assistant()),
            Span::styled(phase_suffix, theme.style_dim()),
        ]));
    }
}
