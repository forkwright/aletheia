use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::theme::Theme;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let is_streaming = app.active_turn_id.is_some();

    let prompt_str = if is_streaming { "queued › " } else { "› " };
    let prompt_color = if is_streaming {
        theme.status.warning
    } else {
        theme.colors.accent
    };

    let prompt_width = prompt_str.len(); // ASCII-only prompt, bytes == display columns
    let content_width = area.width.max(1) as usize;
    let visible_rows = area.height.saturating_sub(1) as usize; // subtract top border

    let text = app.input.text.as_str();
    let cursor_byte = app.input.cursor;

    let first_line_avail = content_width.saturating_sub(prompt_width).max(1);
    let line_ranges = word_wrap_lines(text, first_line_avail, content_width);

    let (cursor_row, cursor_col) =
        cursor_visual_position(&line_ranges, text, cursor_byte, prompt_width);

    // Scroll to keep cursor visible (cursor_row is 0-indexed).
    let scroll = if visible_rows > 0 {
        (cursor_row as usize).saturating_sub(visible_rows - 1)
    } else {
        0
    };

    let total_rows = line_ranges.len();
    let has_above = scroll > 0;
    let has_below = scroll + visible_rows < total_rows;

    // Build one ratatui Line per word-wrapped visual line.
    let mut ratatui_lines: Vec<Line<'static>> = Vec::with_capacity(total_rows);
    for (i, &(start, end)) in line_ranges.iter().enumerate() {
        let line_text = text[start..end].to_string();
        if i == 0 {
            ratatui_lines.push(Line::from(vec![
                Span::styled(prompt_str.to_string(), Style::default().fg(prompt_color)),
                Span::styled(line_text, theme.style_fg()),
            ]));
        } else {
            ratatui_lines.push(Line::from(vec![Span::styled(line_text, theme.style_fg())]));
        }
    }

    // Scroll indicator: annotate the last visible row when lines are hidden.
    if has_above || has_below {
        let indicator = match (has_above, has_below) {
            (true, true) => " ↕",
            (true, false) => " ↑",
            _ => " ↓",
        };
        let last_visible = (scroll + visible_rows - 1).min(total_rows - 1);
        if let Some(line) = ratatui_lines.get_mut(last_visible) {
            line.spans.push(Span::styled(indicator, theme.style_dim()));
        }
    }

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.borders.separator));

    let paragraph = Paragraph::new(Text::from(ratatui_lines))
        .block(block)
        .scroll((scroll as u16, 0));
    frame.render_widget(paragraph, area);

    // Position terminal cursor.
    let visible_row = (cursor_row as usize).saturating_sub(scroll);
    let cursor_y = area.y + 1 + visible_row as u16; // +1 for top border
    let cursor_x = area.x + cursor_col;
    frame.set_cursor_position((cursor_x, cursor_y));
}

/// Compute word-wrapped line ranges for `text`.
///
/// Returns a `Vec` of `(start_byte, end_byte)` pairs — one per visual line.
/// The content to render for line `i` is `&text[start..end]`.
/// Whitespace at word-wrap break points is consumed (not included in either line).
///
/// `first_line_avail`: display columns available on the first line (after the prompt).
/// `line_avail`: display columns available on subsequent lines (full width).
pub(crate) fn word_wrap_lines(
    text: &str,
    first_line_avail: usize,
    line_avail: usize,
) -> Vec<(usize, usize)> {
    if text.is_empty() {
        return vec![(0, 0)];
    }

    let mut lines: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0usize;
    let mut is_first = true;

    while pos <= text.len() {
        let avail = if is_first {
            first_line_avail.max(1)
        } else {
            line_avail.max(1)
        };
        is_first = false;

        let remaining = &text[pos..];

        if remaining.is_empty() {
            break;
        }

        // Count characters and find the last whitespace within `avail` columns.
        let mut last_ws_byte: Option<usize> = None; // byte offset within `remaining`
        let mut byte_at_avail: usize = remaining.len(); // byte offset of char at position `avail`
        let mut char_count = 0usize;

        for (b, c) in remaining.char_indices() {
            if char_count == avail {
                byte_at_avail = b;
                break;
            }
            if c.is_whitespace() {
                last_ws_byte = Some(b);
            }
            char_count += 1;
        }

        // If remaining text fits entirely, this is the last line.
        if char_count < avail || remaining.chars().count() <= avail {
            lines.push((pos, pos + remaining.len()));
            break;
        }

        // Determine the break point.
        if let Some(ws_b) = last_ws_byte {
            // Break at the last whitespace: exclude it from both lines.
            let ws_len = remaining[ws_b..].chars().next().map_or(1, |c| c.len_utf8());
            lines.push((pos, pos + ws_b));
            pos += ws_b + ws_len; // skip over the whitespace
        } else {
            // No whitespace within `avail` — char-wrap at the boundary.
            lines.push((pos, pos + byte_at_avail));
            pos += byte_at_avail;
        }
    }

    if lines.is_empty() {
        lines.push((0, text.len()));
    }

    lines
}

/// Map a cursor byte offset to `(visual_row, visual_col)`.
///
/// `visual_col` for row 0 includes `prompt_width`.
pub(crate) fn cursor_visual_position(
    line_ranges: &[(usize, usize)],
    text: &str,
    cursor_byte: usize,
    prompt_width: usize,
) -> (u16, u16) {
    for (row, &(start, end)) in line_ranges.iter().enumerate() {
        // Cursor on this line if it falls within [start, end] (inclusive at end
        // so cursor-at-end-of-line is on this line rather than the next).
        if cursor_byte >= start && cursor_byte <= end {
            let col_chars = text[start..cursor_byte].chars().count();
            let col = if row == 0 {
                prompt_width + col_chars
            } else {
                col_chars
            };
            return (row as u16, col as u16);
        }
    }

    // Cursor is past the last range's end (e.g. on a stripped space at a break point).
    // Place it at the start of the next line.
    let last_row = line_ranges.len().saturating_sub(1);
    if let Some(&(start, _)) = line_ranges.last() {
        let col_chars = text[start..cursor_byte.min(text.len())].chars().count();
        let col = if last_row == 0 {
            prompt_width + col_chars
        } else {
            col_chars
        };
        return (last_row as u16, col as u16);
    }

    (0, prompt_width as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_gives_single_range() {
        let ranges = word_wrap_lines("", 5, 10);
        assert_eq!(ranges, vec![(0, 0)]);
    }

    #[test]
    fn short_text_fits_on_first_line() {
        let ranges = word_wrap_lines("hello", 10, 10);
        assert_eq!(ranges, vec![(0, 5)]);
    }

    #[test]
    fn word_wrap_breaks_at_space() {
        // "hello world" with first_line_avail=7, line_avail=10
        // "hello w" = 7 chars but last space is at 5, so break at 5
        let ranges = word_wrap_lines("hello world", 7, 10);
        assert_eq!(ranges[0], (0, 5)); // "hello"
        assert_eq!(ranges[1], (6, 11)); // "world"
    }

    #[test]
    fn char_wrap_when_no_space() {
        // "abcdefgh" with avail=5 — no spaces, char wrap
        let ranges = word_wrap_lines("abcdefgh", 5, 5);
        assert_eq!(ranges[0], (0, 5)); // "abcde"
        assert_eq!(ranges[1], (5, 8)); // "fgh"
    }

    #[test]
    fn cursor_on_first_line_includes_prompt() {
        let text = "hello";
        let ranges = word_wrap_lines(text, 10, 10);
        let (row, col) = cursor_visual_position(&ranges, text, 3, 2); // cursor at 'l'
        assert_eq!(row, 0);
        assert_eq!(col, 5); // prompt(2) + chars before cursor(3)
    }

    #[test]
    fn cursor_on_second_line_no_prompt() {
        let text = "hello world";
        let ranges = word_wrap_lines(text, 7, 10);
        // "hello" on line 0 (0..5), "world" on line 1 (6..11)
        let (row, col) = cursor_visual_position(&ranges, text, 8, 2); // cursor at 'r'
        assert_eq!(row, 1);
        assert_eq!(col, 2); // 2 chars into "world" ('wo')
    }

    #[test]
    fn cursor_at_end_of_text() {
        let text = "hello";
        let ranges = word_wrap_lines(text, 10, 10);
        let (row, col) = cursor_visual_position(&ranges, text, 5, 2);
        assert_eq!(row, 0);
        assert_eq!(col, 7); // prompt(2) + 5 chars
    }

    #[test]
    fn delete_to_end_semantics() {
        // Verify the range returned ends at text.len() for last line
        let text = "foo bar";
        let ranges = word_wrap_lines(text, 4, 4);
        assert_eq!(*ranges.last().unwrap(), (4, 7)); // "bar" ends at text.len()
    }
}
