//! Input sanitization for terminal-rendered content.

use std::borrow::Cow;
use std::sync::LazyLock;

static ANSI_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b\(B")
        .expect("valid ANSI regex")
});

pub fn strip_ansi(s: &str) -> Cow<'_, str> {
    ANSI_RE.replace_all(s, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_color_codes() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn clean_text_passthrough() {
        let clean = "no escapes here";
        let result = strip_ansi(clean);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(&*result, clean);
    }

    #[test]
    fn strip_complex_sequences() {
        let input = "\x1b[1;31;42mbold red on green\x1b[0m normal";
        assert_eq!(strip_ansi(input), "bold red on green normal");
    }

    #[test]
    fn strip_osc_sequences() {
        let input = "\x1b]0;window title\x07visible text";
        assert_eq!(strip_ansi(input), "visible text");
    }
}
