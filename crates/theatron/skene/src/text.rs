//! Shared chat-transcript text projections used by both first-party frontends.

/// Append a bracketed terminal notice (e.g. `"turn aborted: ..."`) to an
/// in-progress assistant message, joining on a blank line unless one is
/// already present.
#[must_use]
pub fn append_terminal_notice(mut text: String, notice: &str) -> String {
    if text.is_empty() {
        format!("[{notice}]")
    } else {
        if text.ends_with("\n\n") {
            text.push('[');
        } else if text.ends_with('\n') {
            text.push_str("\n[");
        } else {
            text.push_str("\n\n[");
        }
        text.push_str(notice);
        text.push(']');
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_wraps_notice_alone() {
        assert_eq!(
            append_terminal_notice(String::new(), "turn aborted"),
            "[turn aborted]"
        );
    }

    #[test]
    fn text_with_no_trailing_newline_gets_blank_line_separator() {
        assert_eq!(
            append_terminal_notice("hello".to_string(), "turn aborted"),
            "hello\n\n[turn aborted]"
        );
    }

    #[test]
    fn text_with_single_trailing_newline_gets_one_more() {
        assert_eq!(
            append_terminal_notice("hello\n".to_string(), "turn aborted"),
            "hello\n\n[turn aborted]"
        );
    }

    #[test]
    fn text_with_blank_line_already_present_appends_directly() {
        assert_eq!(
            append_terminal_notice("hello\n\n".to_string(), "turn aborted"),
            "hello\n\n[turn aborted]"
        );
    }
}
