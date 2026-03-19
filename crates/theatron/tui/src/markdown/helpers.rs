//! Internal helper functions for the markdown renderer.
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::hyperlink::MdLink;
use crate::theme::Theme;

pub(super) fn push_span(spans: &mut Vec<Span<'static>>, col: &mut u16, span: Span<'static>) {
    *col = col.saturating_add(span.content.len().min(u16::MAX as usize) as u16);
    spans.push(span);
}

/// Emit styled spans for a text chunk that contains detected URLs.
///
/// Non-URL portions use the base style. URL portions get underline + accent
/// colour and are recorded in `md_links` for OSC 8 post-processing.
#[expect(
    clippy::too_many_arguments,
    reason = "all args are needed for span building"
)]
pub(super) fn linkify_text(
    text: &str,
    urls: &[(usize, usize, &str)],
    spans: &mut Vec<Span<'static>>,
    col: &mut u16,
    md_links: &mut Vec<MdLink>,
    line_idx: usize,
    base_style: Style,
    theme: &Theme,
) {
    let link_style = base_style
        .add_modifier(Modifier::UNDERLINED)
        .fg(theme.colors.accent);

    let mut last = 0usize;
    for &(start, end, url) in urls {
        if start > last {
            let before = text.get(last..start).unwrap_or("");
            push_span(spans, col, Span::styled(before.to_string(), base_style));
        }
        let url_text = text.get(start..end).unwrap_or("");
        let link_col = *col;
        push_span(spans, col, Span::styled(url_text.to_string(), link_style));
        md_links.push(MdLink {
            line_idx,
            col: link_col,
            text: url_text.to_string(),
            url: url.to_string(),
        });
        last = end;
    }
    if last < text.len() {
        push_span(
            spans,
            col,
            Span::styled(text.get(last..).unwrap_or("").to_string(), base_style),
        );
    }
}

/// Render a table with box-drawing characters.
#[expect(
    clippy::indexing_slicing,
    reason = "i < num_cols guard ensures i is within col_widths (len == num_cols)"
)]
pub(super) fn render_table(rows: &[Vec<String>], lines: &mut Vec<Line<'static>>, theme: &Theme) {
    if rows.is_empty() {
        return;
    }

    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths: Vec<usize> = vec![0; num_cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < num_cols {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    // NOTE: cap column widths to prevent overflow on pathologically wide tables
    for w in &mut col_widths {
        *w = (*w).min(40);
    }

    let border_style = Style::default().fg(theme.borders.normal);
    let header_style = theme.style_accent_bold();
    let cell_style = Style::default().fg(theme.text.fg);

    let top = format!(
        " ┌{}┐",
        col_widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .fold(String::new(), |mut acc, s| {
                if !acc.is_empty() {
                    acc.push('┬');
                }
                acc.push_str(&s);
                acc
            })
    );
    lines.push(Line::from(Span::styled(top, border_style)));

    for (row_idx, row) in rows.iter().enumerate() {
        let mut row_spans = vec![Span::styled(" │", border_style)];
        for (i, width) in col_widths.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let padded = format!(" {:width$} ", cell, width = width);
            let style = if row_idx == 0 {
                header_style
            } else {
                cell_style
            };
            row_spans.push(Span::styled(padded, style));
            row_spans.push(Span::styled("│", border_style));
        }
        lines.push(Line::from(row_spans));

        if row_idx == 0 {
            let sep = format!(
                " ├{}┤",
                col_widths
                    .iter()
                    .map(|w| "─".repeat(w + 2))
                    .fold(String::new(), |mut acc, s| {
                        if !acc.is_empty() {
                            acc.push('┼');
                        }
                        acc.push_str(&s);
                        acc
                    })
            );
            lines.push(Line::from(Span::styled(sep, border_style)));
        }
    }

    let bottom = format!(
        " └{}┘",
        col_widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .fold(String::new(), |mut acc, s| {
                if !acc.is_empty() {
                    acc.push('┴');
                }
                acc.push_str(&s);
                acc
            })
    );
    lines.push(Line::from(Span::styled(bottom, border_style)));
}

pub(super) fn flush_line(
    lines: &mut Vec<Line<'static>>,
    spans: &mut Vec<Span<'static>>,
    col: &mut u16,
) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
    *col = 0;
}

pub(super) fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}
