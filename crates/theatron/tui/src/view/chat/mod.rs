use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::hyperlink::{self, OscLink};
use crate::markdown;
use crate::state::{FilterScope, MessageKind};
use crate::theme::Theme;
use crate::view::image;

mod streaming;

pub(super) fn format_duration_adaptive(ms: u64) -> String {
    if ms < 60_000 {
        let s = ms / 1000;
        if s == 0 {
            "<1s".to_string()
        } else {
            format!("{s}s")
        }
    } else if ms < 3_600_000 {
        let m = ms / 60_000;
        let s = (ms % 60_000) / 1000;
        format!("{m}m{s}s")
    } else {
        let h = ms / 3_600_000;
        let m = (ms % 3_600_000) / 60_000;
        format!("{h}h{m}m")
    }
}

struct MessageCtx<'a> {
    inner_width: usize,
    theme: &'a Theme,
    selected: bool,
    highlight: Option<&'a str>,
    agent_name: &'a str,
}

pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) -> Vec<OscLink> {
    let inner_width = usize::from(area.width.saturating_sub(2));
    let wrap_width = area.width.saturating_sub(2).max(1);
    // With Borders::NONE the paragraph has the full area height available.
    let visible_height = area.height;
    // Para-relative link data collected from all messages.
    let mut para_links: Vec<(usize, u16, String, String)> = Vec::new(); // (line_idx, col, text, url)

    let filter_active = app.interaction.filter.active
        && app.interaction.filter.scope == FilterScope::Chat
        && !app.interaction.filter.text.is_empty();
    let (pattern, inverted) = app.interaction.filter.pattern();

    let agent_name_lower: &str = app
        .dashboard
        .focused_agent
        .as_ref()
        .and_then(|id| app.dashboard.agents.iter().find(|a| a.id == *id))
        .map(|a| a.name_lower.as_str())
        .unwrap_or("assistant");

    // When filter is active, we fall back to iterating all messages (filter changes
    // the visible set dynamically). For the non-filtered common path, we use the
    // VirtualScroll prefix-sum index for O(log n) range lookup.

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw("")); // top padding

    // PERF: Static buffer for finalized messages. When committed messages haven't
    // changed and the width is the same, reuse the cached rendered lines instead
    // of re-parsing markdown. This prevents redundant work during streaming frames
    // where only the streaming section changes.
    let static_cache_valid = !filter_active
        && app.viewport.render.static_message_count == app.dashboard.messages.len()
        && app.viewport.render.static_width == inner_width
        && !app.dashboard.messages.is_empty();

    if static_cache_valid {
        lines.extend(app.viewport.render.static_lines.iter().cloned());
    } else if filter_active {
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

    // Empty state: no messages and not streaming: show helpful placeholder.
    if app.dashboard.messages.is_empty()
        && app.connection.active_turn_id.is_none()
        && !filter_active
    {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("no messages yet", theme.style_dim()),
        ]));
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "type a message below to start a conversation",
                theme.style_muted(),
            ),
        ]));
    }

    if filter_active && lines.len() <= 1 {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("no matches", theme.style_dim()),
        ]));
    }

    // Submitted decision fact cards
    render_decision_fact_cards(app, &mut lines, inner_width, theme);

    // Streaming response (in progress)
    if !app.connection.streaming_text.is_empty()
        || !app.connection.streaming_thinking.is_empty()
        || app.connection.active_turn_id.is_some()
    {
        streaming::render_streaming(app, &mut lines, inner_width, theme, agent_name_lower);
    }

    // Queued messages: shown below streaming with dimmed "queued" badge
    if !app.interaction.queued_messages.is_empty() {
        streaming::render_queued_messages(app, &mut lines, theme);
    }

    // Bottom alignment: when total content is shorter than the pane, push it to
    // the bottom by prepending empty lines.  Only applies when not scrolled
    // (content fits in the viewport).
    {
        let w = usize::from(wrap_width.max(1));
        let total_visual: usize = lines
            .iter()
            .map(|line| {
                let lw: usize = line.spans.iter().map(|s| s.content.len()).sum();
                if lw == 0 { 1 } else { lw.div_ceil(w) }
            })
            .sum();
        let padding = usize::from(visible_height).saturating_sub(total_visual);
        if padding > 0 {
            let mut padded: Vec<Line<'static>> = vec![Line::raw(""); padding];
            padded.append(&mut lines);
            lines = padded;
            // Shift all paragraph-relative link line indices by the padding.
            for (line_idx, _, _, _) in &mut para_links {
                *line_idx += padding;
            }
        }
    }

    // Pre-compute per-line widths: needed for resolve_osc_links and scroll calc.
    let line_widths: Vec<usize> = lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.len()).sum())
        .collect();

    // Total visual rows of the final rendered lines vector (after padding + streaming).
    // Used for auto-scroll so that streaming content appended after virtual render is
    // always visible when the user is at the bottom.
    let w = usize::from(wrap_width.max(1));
    let total_visual: usize = line_widths
        .iter()
        .map(|&lw| if lw == 0 { 1 } else { lw.div_ceil(w) })
        .sum();
    let vh = usize::from(visible_height);

    // WHY: When the virtual scroll cache is stale (width mismatch or item count mismatch),
    // render_virtual_messages falls back to rendering ALL messages.  In that case the
    // virtual-scroll `line_offset` is meaningless -- it was computed for a partial slice
    // that was never rendered.  Detect the same stale condition here and use the same
    // total-minus-offset formula that the filter path uses.
    let needs_fallback = !filter_active
        && (app.viewport.render.virtual_scroll.len() != app.dashboard.messages.len()
            || (app.viewport.render.virtual_scroll.cached_width() != wrap_width
                && !app.dashboard.messages.is_empty()));

    let scroll = if app.viewport.render.auto_scroll {
        // Pin to the very bottom of whatever was rendered (committed + streaming).
        total_visual.saturating_sub(vh)
    } else if filter_active || needs_fallback {
        // Filtered / fallback path: all messages are in `lines`; use the pre-computed total.
        total_visual
            .saturating_sub(vh)
            .saturating_sub(app.viewport.render.scroll_offset)
    } else {
        // Virtual scroll path with manual offset: line_offset already positions us
        // correctly within the rendered (viewport-only) items.
        let slice = app.viewport.render.virtual_scroll.visible_slice(
            app.viewport.render.scroll_offset,
            app.viewport.render.auto_scroll,
            visible_height,
        );
        usize::from(slice.line_offset)
    };

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0));

    frame.render_widget(paragraph, area);

    // Resolve para-relative links to absolute screen coordinates.
    resolve_osc_links(
        &para_links,
        &line_widths,
        area,
        u16::try_from(scroll).unwrap_or(u16::MAX),
        usize::from(wrap_width),
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
    theme: &Theme,
) -> Vec<OscLink> {
    if !hyperlink::supports_hyperlinks() || para_links.is_empty() {
        return Vec::new();
    }

    // Extract accent RGB: fall back to a sensible default for non-Rgb variants.
    let accent = match theme.colors.accent {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (120, 180, 255),
    };

    // Pre-compute the cumulative visual-row offset for each logical line.
    // visual_row_start[i] = number of visual rows before logical line i.
    let mut visual_row_start: Vec<u32> = Vec::with_capacity(line_widths.len());
    let mut cumulative: u32 = 0;
    for &w in line_widths {
        visual_row_start.push(cumulative);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "terminal dimensions fit in u32"
        )]
        let rows = if w == 0 { 1 } else { w.div_ceil(wrap_width) } as u32;
        cumulative += rows;
    }

    let visible_height = i32::from(area.height);
    let mut osc_links = Vec::with_capacity(para_links.len());

    for (line_idx, col, text, url) in para_links {
        let Some(&vrow_start) = visual_row_start.get(*line_idx) else {
            continue;
        };
        // Adjust for which visual row within the wrapped line this col sits on.
        let col_row = if wrap_width > 0 {
            usize::from(*col) / wrap_width
        } else {
            0
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "visual row count fits in i32 for terminal"
        )]
        let vrow = vrow_start as i32 + col_row as i32;

        // Apply scroll: positive scroll shifts content upward (scroll=0 means show from top).
        let screen_row = vrow - i32::from(scroll);
        if screen_row < 0 || screen_row >= visible_height {
            continue; // link is outside the visible window
        }

        let screen_x =
            area.x + u16::try_from(usize::from(*col) % wrap_width.max(1)).unwrap_or(u16::MAX);
        let screen_y = area.y + u16::try_from(screen_row).unwrap_or(0);

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
#[expect(
    clippy::too_many_arguments,
    reason = "render context requires all params; extracting a struct would add boilerplate without clarity gain"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "slice.range is computed by VirtualScroll::visible_slice which returns only valid item indices into messages"
)]
fn render_virtual_messages(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    wrap_width: u16,
    visible_height: u16,
    theme: &Theme,
    agent_name: &str,
    para_links: &mut Vec<(usize, u16, String, String)>,
) {
    // Ensure the virtual scroll cache is populated and matches current width.
    // This is a read-only check: cache rebuilds happen in the update layer.
    // If the cache is stale (width changed or item count mismatch), fall back to
    // full iteration for this single frame. The next update tick will rebuild.
    let needs_fallback = app.viewport.render.virtual_scroll.len() != app.dashboard.messages.len()
        || (app.viewport.render.virtual_scroll.cached_width() != wrap_width
            && !app.dashboard.messages.is_empty());

    if needs_fallback {
        // Fallback: render all messages this frame. The cache will be rebuilt.
        for (idx, msg) in app.dashboard.messages.iter().enumerate() {
            let ctx = MessageCtx {
                inner_width,
                theme,
                selected: app.interaction.selected_message == Some(idx),
                highlight: None,
                agent_name,
            };
            render_message(app, msg, lines, &ctx, para_links);
        }
        return;
    }

    let slice = app.viewport.render.virtual_scroll.visible_slice(
        app.viewport.render.scroll_offset,
        app.viewport.render.auto_scroll,
        visible_height,
    );

    if slice.range.is_empty() {
        return;
    }

    for idx in slice.range.clone() {
        let msg = &app.dashboard.messages[idx];
        let ctx = MessageCtx {
            inner_width,
            theme,
            selected: app.interaction.selected_message == Some(idx),
            highlight: None,
            agent_name,
        };
        render_message(app, msg, lines, &ctx, para_links);
    }
}

/// Render all messages, skipping those that don't match the filter.
#[expect(
    clippy::too_many_arguments,
    reason = "render context requires all params; extracting a struct would add boilerplate without clarity gain"
)]
fn render_filtered_messages(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &Theme,
    agent_name: &str,
    pattern: &str,
    inverted: bool,
    para_links: &mut Vec<(usize, u16, String, String)>,
) {
    for (idx, msg) in app.dashboard.messages.iter().enumerate() {
        let contains = msg.text_lower.contains(pattern);
        let show = if inverted { !contains } else { contains };
        if !show {
            continue;
        }
        let ctx = MessageCtx {
            inner_width,
            theme,
            selected: app.interaction.selected_message == Some(idx),
            highlight: Some(pattern),
            agent_name,
        };
        render_message(app, msg, lines, &ctx, para_links);
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

    // Dispatch on message kind for distinct rendering per type.
    match msg.kind {
        MessageKind::ToolStatusLine => {
            render_tool_status_line(msg, lines, theme);
            return;
        }
        MessageKind::ThinkingStatusLine => {
            render_thinking_status_line(msg, lines, theme);
            return;
        }
        MessageKind::DistillationMarker => {
            render_distillation_marker(msg, lines, ctx.inner_width, theme);
            return;
        }
        MessageKind::TopicBoundary => {
            render_topic_boundary(lines, ctx.inner_width, theme);
            return;
        }
        MessageKind::Standard => {}
    }

    let (role_label, role_style) = match msg.role.as_str() {
        "user" => ("you", theme.style_user()),
        "assistant" => (ctx.agent_name, theme.style_assistant()),
        _ => ("system", theme.style_muted()),
    };

    // WHY: Subtle background tint on assistant messages creates visual distinction
    // between human and nous/agent messages without being distracting.
    let is_assistant = msg.role == "assistant";
    let msg_bg = if is_assistant {
        Some(theme.colors.surface)
    } else {
        None
    };

    // Selection indicator prefix
    let marker = if ctx.selected { "▸" } else { " " };
    let marker_style = if ctx.selected {
        Style::default().fg(theme.borders.selected)
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

    let mut header_line = Line::from(header_spans);
    if let Some(bg) = msg_bg {
        header_line = header_line.style(Style::default().bg(bg));
    }
    lines.push(header_line);

    // Tool calls as collapsible cards
    if !msg.tool_calls.is_empty() {
        render_tool_cards(app, &msg.tool_calls, lines, ctx.inner_width, theme);
    }

    // Message content: markdown parsed with syntax highlighting
    let (md_lines, md_links) = markdown::render(
        &msg.text,
        ctx.inner_width.saturating_sub(2),
        theme,
        &app.highlighter,
    );
    let content_prefix = if ctx.selected { "│" } else { " " };
    let prefix_width: u16 = u16::try_from(content_prefix.len()).unwrap_or(u16::MAX); // always 1 byte for these strings
    let prefix_style = if ctx.selected {
        Style::default().fg(theme.borders.selected)
    } else {
        Style::default()
    };

    let highlight_bg = Style::default().bg(theme.colors.accent_dim);

    // Offset: paragraph line index of the first markdown line for this message.
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

        let mut content_line = Line::from(padded_spans);
        if let Some(bg) = msg_bg {
            content_line = content_line.style(Style::default().bg(bg));
        }
        lines.push(content_line);
    }

    // Convert markdown-relative MdLink positions to paragraph-relative para_links.
    for link in md_links {
        let abs_line = md_para_offset + link.line_idx;
        let abs_col = prefix_width + link.col;
        para_links.push((abs_line, abs_col, link.text, link.url));
    }

    // Inline image preview: detect image file paths in the message text and
    // render half-block previews (true-color terminals) or filename+size fallback.
    let image_paths = image::detect_image_paths(&msg.text);
    for path in &image_paths {
        let preview_width = ctx.inner_width.saturating_sub(2);
        let preview_lines = image::render_preview_lines(path, preview_width);
        for line in preview_lines {
            let mut padded = vec![Span::styled(content_prefix, prefix_style)];
            padded.extend(line.spans);
            let mut img_line = Line::from(padded);
            if let Some(bg) = msg_bg {
                img_line = img_line.style(Style::default().bg(bg));
            }
            lines.push(img_line);
        }
    }

    // Breathing room between messages
    lines.push(Line::raw(""));
}

/// Render tool calls as collapsible cards with status icons.
///
/// Each card shows: status icon + tool name + duration on one line.
/// Expanded cards show the tool output below. Errors are auto-expanded.
fn render_tool_cards(
    app: &App,
    tool_calls: &[crate::state::ToolCallInfo],
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &Theme,
) {
    for tc in tool_calls {
        let icon = if tc.is_error { "✗" } else { "✓" };
        let icon_style = if tc.is_error {
            Style::default().fg(theme.status.error)
        } else {
            Style::default().fg(theme.status.success)
        };

        let is_expanded = tc
            .tool_id
            .as_ref()
            .is_some_and(|id| app.interaction.tool_expanded.contains(id));

        let toggle = if is_expanded { "▾" } else { "▸" };

        let mut card_spans = vec![
            Span::styled(format!("  {toggle} "), theme.style_dim()),
            Span::styled(icon, icon_style),
            Span::raw(" "),
            Span::styled(tc.name.clone(), theme.style_fg()),
        ];

        if let Some(ms) = tc.duration_ms {
            card_spans.push(Span::styled(
                format!(" · {}", format_duration_adaptive(ms)),
                theme.style_dim(),
            ));
        }

        lines.push(Line::from(card_spans));

        // Expanded output
        if is_expanded && let Some(ref output) = tc.output {
            let max_width = inner_width.saturating_sub(6);
            for output_line in output.lines().take(20) {
                let truncated: String = output_line.chars().take(max_width).collect();
                lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled(truncated, theme.style_dim()),
                ]));
            }
            let line_count = output.lines().count();
            if line_count > 20 {
                lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled(
                        format!("… ({} more lines)", line_count.saturating_sub(20)),
                        theme.style_muted(),
                    ),
                ]));
            }
        }
    }
}

/// Compact one-line tool status: `✓ tool_name · duration`.
fn render_tool_status_line(
    msg: &crate::app::ChatMessage,
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
) {
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("·", theme.style_dim()),
        Span::raw(" "),
        Span::styled(msg.text.clone(), theme.style_muted()),
    ]));
}

/// Compact thinking indicator line.
fn render_thinking_status_line(
    msg: &crate::app::ChatMessage,
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
) {
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("◆", Style::default().fg(theme.thinking.border)),
        Span::raw(" "),
        Span::styled(msg.text.clone(), Style::default().fg(theme.thinking.fg)),
    ]));
}

/// Distillation summary boundary marker.
fn render_distillation_marker(
    msg: &crate::app::ChatMessage,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &Theme,
) {
    let border_len = inner_width.saturating_sub(18).min(30);
    let border = "─".repeat(border_len);
    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled(
            format!("─── distilled {border}"),
            Style::default().fg(theme.colors.accent),
        ),
    ]));
    if !msg.text.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(msg.text.clone(), theme.style_dim()),
        ]));
    }
    lines.push(Line::raw(""));
}

/// Visual separator between conversation topics.
fn render_topic_boundary(lines: &mut Vec<Line<'static>>, inner_width: usize, theme: &Theme) {
    let border = "─".repeat(inner_width.saturating_sub(4).min(40));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(border, theme.style_dim()),
    ]));
    lines.push(Line::raw(""));
}

fn highlight_span(
    span: &Span<'static>,
    pattern: &str,
    highlight_style: Style,
    out: &mut Vec<Span<'static>>,
) {
    let content = &span.content;

    if pattern.is_empty() {
        out.push(span.clone());
        return;
    }

    let content_lower = content.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    let mut last_char_idx = 0;

    for (byte_start, _) in content_lower.match_indices(&pattern_lower) {
        // Convert byte offset in the original string to char index
        let char_start = content.get(..byte_start).unwrap_or("").chars().count();
        let pattern_chars = pattern.chars().count();
        let char_end = char_start + pattern_chars;

        // Re-slice the original content by reconstructing from char indices
        if char_start > last_char_idx {
            let before: String = content
                .chars()
                .skip(last_char_idx)
                .take(char_start - last_char_idx)
                .collect();
            out.push(Span::styled(before, span.style));
        }

        let highlighted: String = content
            .chars()
            .skip(char_start)
            .take(pattern_chars)
            .collect();
        out.push(Span::styled(highlighted, span.style.patch(highlight_style)));
        last_char_idx = char_end;
    }

    if last_char_idx < content.chars().count() {
        let remaining: String = content.chars().skip(last_char_idx).collect();
        out.push(Span::styled(remaining, span.style));
    } else if last_char_idx == 0 {
        out.push(span.clone());
    }
}

fn render_decision_fact_cards(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &Theme,
) {
    if app.dashboard.submitted_decisions.is_empty() {
        return;
    }
    for decision in &app.dashboard.submitted_decisions {
        let border_len = inner_width.saturating_sub(14).min(30);
        let border_line = "─".repeat(border_len);
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                format!("─── decision {border_line}"),
                Style::default().fg(theme.colors.accent),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("Q: ", theme.style_dim()),
            Span::styled(decision.question.clone(), theme.style_fg()),
        ]));
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("A: ", theme.style_dim()),
            Span::styled(decision.chosen_label.clone(), theme.style_accent_bold()),
        ]));
        if !decision.notes.is_empty() {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("  note: ", theme.style_dim()),
                Span::styled(decision.notes.clone(), theme.style_muted()),
            ]));
        }
        let bottom_border = "─".repeat(inner_width.saturating_sub(2).min(38));
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(bottom_border, Style::default().fg(theme.colors.accent)),
        ]));
        lines.push(Line::raw(""));
    }
}
