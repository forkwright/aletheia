use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, ToolCallInfo};
use crate::hyperlink::{self, OscLink};
use crate::markdown;
use crate::state::FilterScope;
use crate::theme::{self, ThemePalette};

const MS_PER_SECOND: u64 = 1000;

struct MessageCtx<'a> {
    inner_width: usize,
    theme: &'a ThemePalette,
    selected: bool,
    highlight: Option<&'a str>,
    agent_name: &'a str,
}

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) -> Vec<OscLink> {
    let inner_width = area.width.saturating_sub(2) as usize;
    let wrap_width = area.width.saturating_sub(2).max(1);
    let visible_height = area.height.saturating_sub(2);
    // Para-relative link data collected from all messages.
    let mut para_links: Vec<(usize, u16, String, String)> = Vec::new(); // (line_idx, col, text, url)

    let filter_active =
        app.filter.active && app.filter.scope == FilterScope::Chat && !app.filter.text.is_empty();
    let (pattern, inverted) = app.filter.pattern();

    let agent_name_lower: &str = app
        .focused_agent
        .as_ref()
        .and_then(|id| app.agents.iter().find(|a| a.id == *id))
        .map(|a| a.name_lower.as_str())
        .unwrap_or("assistant");

    // --- Virtual scroll: determine which messages to render ---
    //
    // When filter is active, we fall back to iterating all messages (filter changes
    // the visible set dynamically). For the non-filtered common path, we use the
    // VirtualScroll prefix-sum index for O(log n) range lookup.

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw("")); // top padding

    if filter_active {
        // Filtered path: iterate all messages, skip non-matching.
        // This is acceptable because filtering is interactive and users rarely
        // have 15K messages with a filter active.
        render_filtered_messages(
            app,
            &mut lines,
            inner_width,
            theme,
            agent_name_lower,
            pattern,
            inverted,
            &mut para_links,
        );
    } else {
        // Virtual scroll path: only render viewport + buffer items.
        render_virtual_messages(
            app,
            &mut lines,
            inner_width,
            wrap_width,
            visible_height,
            theme,
            agent_name_lower,
            &mut para_links,
        );
    }

    if filter_active && lines.len() <= 1 {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("no matches", theme.style_dim()),
        ]));
    }

    // Streaming response (in progress)
    if !app.streaming_text.is_empty()
        || !app.streaming_thinking.is_empty()
        || app.active_turn_id.is_some()
    {
        render_streaming(app, &mut lines, inner_width, theme, agent_name_lower);
    }

    // Pre-compute per-line widths — needed for resolve_osc_links and legacy scroll.
    let line_widths: Vec<usize> = lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.len()).sum())
        .collect();

    // For virtual scroll path, the scroll offset is already baked into which items
    // we rendered and the line_offset. For filtered path, use legacy scroll calc.
    let scroll = if filter_active {
        compute_legacy_scroll(
            &lines,
            wrap_width,
            visible_height,
            app.auto_scroll,
            app.scroll_offset,
        )
    } else {
        // Virtual scroll: we rendered exactly the right items with correct offset.
        // The line_offset from visible_slice tells us where to start within the
        // rendered content.
        let slice =
            app.virtual_scroll
                .visible_slice(app.scroll_offset, app.auto_scroll, visible_height);
        slice.line_offset as usize
    };

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    frame.render_widget(paragraph, area);

    // Resolve para-relative links to absolute screen coordinates.
    resolve_osc_links(
        &para_links,
        &line_widths,
        area,
        scroll as u16,
        wrap_width as usize,
        theme,
    )
}

/// Convert paragraph-relative link positions to absolute screen [`OscLink`]s.
///
/// Returns an empty vec when the terminal does not support hyperlinks.
/// Uses the same visual-line calculation as the scroll offset logic above.
fn resolve_osc_links(
    para_links: &[(usize, u16, String, String)],
    line_widths: &[usize],
    area: Rect,
    scroll: u16,
    wrap_width: usize,
    theme: &ThemePalette,
) -> Vec<OscLink> {
    if !hyperlink::supports_hyperlinks() || para_links.is_empty() {
        return Vec::new();
    }

    // Extract accent RGB — fall back to a sensible default for non-Rgb variants.
    let accent = match theme.accent {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (120, 180, 255),
    };

    // Pre-compute the cumulative visual-row offset for each logical line.
    // visual_row_start[i] = number of visual rows before logical line i.
    let mut visual_row_start: Vec<u32> = Vec::with_capacity(line_widths.len());
    let mut cumulative: u32 = 0;
    for &w in line_widths {
        visual_row_start.push(cumulative);
        let rows = if w == 0 { 1 } else { w.div_ceil(wrap_width) } as u32;
        cumulative += rows;
    }

    let visible_height = area.height as i32;
    let mut osc_links = Vec::with_capacity(para_links.len());

    for (line_idx, col, text, url) in para_links {
        let Some(&vrow_start) = visual_row_start.get(*line_idx) else {
            continue;
        };
        // Adjust for which visual row within the wrapped line this col sits on.
        let col_row = if wrap_width > 0 {
            (*col as usize) / wrap_width
        } else {
            0
        };
        let vrow = vrow_start as i32 + col_row as i32;

        // Apply scroll: positive scroll shifts content upward (scroll=0 means show from top).
        let screen_row = vrow - scroll as i32;
        if screen_row < 0 || screen_row >= visible_height {
            continue; // link is outside the visible window
        }

        let screen_x = area.x + (*col as usize % wrap_width.max(1)) as u16;
        let screen_y = area.y + screen_row as u16;

        osc_links.push(OscLink {
            screen_x,
            screen_y,
            text: text.clone(),
            url: url.clone(),
            accent,
        });
    }

    osc_links
}

/// Render only the messages in the virtual scroll viewport + buffer zone.
#[expect(clippy::too_many_arguments, reason = "render context requires all params; extracting a struct would add boilerplate without clarity gain")]
fn render_virtual_messages(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    wrap_width: u16,
    visible_height: u16,
    theme: &ThemePalette,
    agent_name: &str,
    para_links: &mut Vec<(usize, u16, String, String)>,
) {
    // Ensure the virtual scroll cache is populated and matches current width.
    // This is a read-only check — cache rebuilds happen in the update layer.
    // If the cache is stale (width changed or item count mismatch), fall back to
    // full iteration for this single frame. The next update tick will rebuild.
    let needs_fallback = app.virtual_scroll.len() != app.messages.len()
        || (app.virtual_scroll.cached_width() != wrap_width && !app.messages.is_empty());

    if needs_fallback {
        // Fallback: render all messages this frame. The cache will be rebuilt.
        for (idx, msg) in app.messages.iter().enumerate() {
            let ctx = MessageCtx {
                inner_width,
                theme,
                selected: app.selected_message == Some(idx),
                highlight: None,
                agent_name,
            };
            render_message(app, msg, lines, &ctx, para_links);
        }
        return;
    }

    let slice =
        app.virtual_scroll
            .visible_slice(app.scroll_offset, app.auto_scroll, visible_height);

    if slice.range.is_empty() {
        return;
    }

    for idx in slice.range.clone() {
        let msg = &app.messages[idx];
        let ctx = MessageCtx {
            inner_width,
            theme,
            selected: app.selected_message == Some(idx),
            highlight: None,
            agent_name,
        };
        render_message(app, msg, lines, &ctx, para_links);
    }
}

/// Render all messages, skipping those that don't match the filter.
#[expect(clippy::too_many_arguments, reason = "render context requires all params; extracting a struct would add boilerplate without clarity gain")]
fn render_filtered_messages(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &ThemePalette,
    agent_name: &str,
    pattern: &str,
    inverted: bool,
    para_links: &mut Vec<(usize, u16, String, String)>,
) {
    for (idx, msg) in app.messages.iter().enumerate() {
        let contains = msg.text_lower.contains(pattern);
        let show = if inverted { !contains } else { contains };
        if !show {
            continue;
        }
        let ctx = MessageCtx {
            inner_width,
            theme,
            selected: app.selected_message == Some(idx),
            highlight: Some(pattern),
            agent_name,
        };
        render_message(app, msg, lines, &ctx, para_links);
    }
}

/// Legacy scroll calculation for filtered mode (iterates all rendered lines).
fn compute_legacy_scroll(
    lines: &[Line<'_>],
    wrap_width: u16,
    visible_height: u16,
    auto_scroll: bool,
    scroll_offset: usize,
) -> usize {
    let w = wrap_width.max(1) as usize;
    let total_visual_lines: usize = lines
        .iter()
        .map(|line| {
            let line_width: usize = line.spans.iter().map(|s| s.content.len()).sum();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(w)
            }
        })
        .sum();
    let vh = visible_height as usize;
    if auto_scroll {
        total_visual_lines.saturating_sub(vh)
    } else {
        total_visual_lines
            .saturating_sub(vh)
            .saturating_sub(scroll_offset)
    }
}

fn render_message(
    app: &App,
    msg: &crate::app::ChatMessage,
    lines: &mut Vec<Line<'static>>,
    ctx: &MessageCtx<'_>,
    para_links: &mut Vec<(usize, u16, String, String)>,
) {
    let theme = ctx.theme;
    let (role_label, role_style) = match msg.role.as_str() {
        "user" => ("you", theme.style_user()),
        "assistant" => (ctx.agent_name, theme.style_assistant()),
        _ => ("system", theme.style_muted()),
    };

    // Selection indicator prefix
    let marker = if ctx.selected { "▸" } else { " " };
    let marker_style = if ctx.selected {
        Style::default().fg(theme.selected)
    } else {
        Style::default()
    };

    // Header: selection marker + role name + optional model (dim) + timestamp
    let mut header_spans = vec![
        Span::styled(marker, marker_style),
        Span::styled(format!(" {}", role_label), role_style),
    ];

    if let Some(ref model) = msg.model {
        let short_model = model.split('/').next_back().unwrap_or(model);
        header_spans.push(Span::styled(
            format!(" · {}", short_model),
            theme.style_dim(),
        ));
    }

    if let Some(ref ts) = msg.timestamp {
        let time_str = ts
            .split('T')
            .nth(1)
            .and_then(|t| t.split('.').next())
            .unwrap_or(ts);
        header_spans.push(Span::styled(format!("  {}", time_str), theme.style_dim()));
    }

    lines.push(Line::from(header_spans));

    // Inline tool call summary (compact, between header and content)
    if !msg.tool_calls.is_empty() {
        render_tool_summary(&msg.tool_calls, lines, theme);
    }

    // Message content — markdown parsed with syntax highlighting
    let (md_lines, md_links) = markdown::render(
        &msg.text,
        ctx.inner_width.saturating_sub(2),
        theme,
        &app.highlighter,
    );
    let content_prefix = if ctx.selected { "│" } else { " " };
    let prefix_width: u16 = content_prefix.len() as u16; // always 1 byte for these strings
    let prefix_style = if ctx.selected {
        Style::default().fg(theme.selected)
    } else {
        Style::default()
    };

    let highlight_bg = Style::default().bg(theme.accent_dim);

    // Offset: paragraph line index of the first markdown line for this message.
    // +1 for the header line; +1 more if tool calls were rendered.
    let md_para_offset = lines.len();

    for line in md_lines {
        let mut padded_spans = vec![Span::styled(content_prefix, prefix_style)];

        if let Some(pattern) = ctx.highlight {
            for span in &line.spans {
                highlight_span(span, pattern, highlight_bg, &mut padded_spans);
            }
        } else {
            padded_spans.extend(line.spans);
        }

        lines.push(Line::from(padded_spans));
    }

    // Convert markdown-relative MdLink positions to paragraph-relative para_links.
    for link in md_links {
        let abs_line = md_para_offset + link.line_idx;
        let abs_col = prefix_width + link.col;
        para_links.push((abs_line, abs_col, link.text, link.url));
    }

    // Breathing room between messages
    lines.push(Line::raw(""));
}

fn highlight_span(
    span: &Span<'static>,
    pattern: &str,
    highlight_style: Style,
    out: &mut Vec<Span<'static>>,
) {
    let content = &span.content;

    // Bail early without allocating: non-ASCII content cannot be safely byte-indexed
    // after lowercasing (byte offsets may shift), and an empty pattern matches nothing.
    if pattern.is_empty() || !content.is_ascii() {
        out.push(span.clone());
        return;
    }

    let content_lower = content.to_lowercase();

    let mut last_end = 0;
    for (start, _) in content_lower.match_indices(pattern) {
        let end = start + pattern.len();
        if start > last_end {
            out.push(Span::styled(
                content[last_end..start].to_string(),
                span.style,
            ));
        }
        out.push(Span::styled(
            content[start..end].to_string(),
            span.style.patch(highlight_style),
        ));
        last_end = end;
    }
    if last_end < content.len() {
        out.push(Span::styled(content[last_end..].to_string(), span.style));
    } else if last_end == 0 {
        out.push(span.clone());
    }
}

/// Render a compact tool call summary line:
///   ╰─ exec (0.3s) → read (0.1s) → grep (0.2s)
fn render_tool_summary(
    tools: &[ToolCallInfo],
    lines: &mut Vec<Line<'static>>,
    theme: &ThemePalette,
) {
    let mut spans: Vec<Span> = vec![Span::raw("  "), Span::styled("╰─ ", theme.style_dim())];

    for (i, tc) in tools.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" → ", theme.style_dim()));
        }

        let color = if tc.is_error {
            theme.error
        } else {
            theme.fg_dim
        };
        let icon = if tc.is_error { "✗ " } else { "" };

        let label = if let Some(ms) = tc.duration_ms {
            if ms >= MS_PER_SECOND {
                format!(
                    "{}{} ({:.1}s)",
                    icon,
                    tc.name,
                    ms as f64 / MS_PER_SECOND as f64
                )
            } else {
                format!("{}{}  ({}ms)", icon, tc.name, ms)
            }
        } else {
            format!("{}{}", icon, tc.name)
        };

        spans.push(Span::styled(label, Style::default().fg(color)));
    }

    lines.push(Line::from(spans));
}

fn render_streaming(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &ThemePalette,
    name: &str,
) {
    // Thinking block (if visible)
    if app.thinking_expanded && !app.streaming_thinking.is_empty() {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("─── thinking ", Style::default().fg(theme.thinking_border)),
            Span::styled(
                "─".repeat(inner_width.saturating_sub(16).min(40)),
                Style::default().fg(theme.thinking_border),
            ),
        ]));
        for line in app.streaming_thinking.lines() {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(line.to_string(), Style::default().fg(theme.thinking)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "─".repeat(inner_width.saturating_sub(4).min(40)),
                Style::default().fg(theme.thinking_border),
            ),
        ]));
    }

    // Active tool calls during streaming (show completed + current)
    if !app.streaming_tool_calls.is_empty() {
        let mut tool_spans: Vec<Span> =
            vec![Span::raw("  "), Span::styled("╰─ ", theme.style_dim())];

        for (i, tc) in app.streaming_tool_calls.iter().enumerate() {
            if i > 0 {
                tool_spans.push(Span::styled(" → ", theme.style_dim()));
            }

            if tc.duration_ms.is_some() {
                // Completed tool
                let color = if tc.is_error {
                    theme.error
                } else {
                    theme.fg_dim
                };
                let icon = if tc.is_error { "✗ " } else { "" };
                let label = if let Some(ms) = tc.duration_ms {
                    if ms >= MS_PER_SECOND {
                        format!(
                            "{}{} ({:.1}s)",
                            icon,
                            tc.name,
                            ms as f64 / MS_PER_SECOND as f64
                        )
                    } else {
                        format!("{}{} ({}ms)", icon, tc.name, ms)
                    }
                } else {
                    tc.name.clone()
                };
                tool_spans.push(Span::styled(label, Style::default().fg(color)));
            } else {
                // Currently running tool — animated
                let ch = theme::spinner_frame(app.tick_count);
                tool_spans.push(Span::styled(
                    format!("{} {}", ch, tc.name),
                    Style::default().fg(theme.spinner),
                ));
            }
        }

        lines.push(Line::from(tool_spans));
    }

    // Streaming text with cursor
    if !app.streaming_text.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(" {}", name),
            theme.style_assistant(),
        )]));

        // Use cached markdown if available (streaming content — links not tracked for OSC 8)
        let rendered = if app.cached_markdown_text == app.streaming_text {
            app.cached_markdown_lines.clone()
        } else {
            markdown::render(
                &app.streaming_text,
                inner_width.saturating_sub(2),
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

        // Braille cursor
        let ch = theme::spinner_frame(app.tick_count);
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(theme.streaming)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else if app.active_turn_id.is_some() {
        // No text yet — show spinner with agent name
        let ch = theme::spinner_frame(app.tick_count);

        lines.push(Line::from(vec![
            Span::styled(format!(" {}", name), theme.style_assistant()),
            Span::styled(format!(" {} thinking…", ch), theme.style_muted()),
        ]));
    }
}
