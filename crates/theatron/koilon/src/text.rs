//! Small string utilities for koilon.
//!
//! WHY: A thin local module so `truncate_chars_ellipsis` and `pad_to` have a
//! single home instead of being reimplemented in `diff/parse.rs`,
//! `view/metrics.rs`, and `state/ops/helpers.rs`.

/// Truncate `text` to at most `max_chars` Unicode scalars, counting the
/// ellipsis (`…`) inside the budget.
///
/// A string that already fits is returned unchanged. A string that does not
/// fit keeps the first `max_chars - 1` characters and appends `…`.
pub(crate) fn truncate_chars_ellipsis(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        text.to_string()
    } else if max_chars == 0 {
        String::new()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Left-pad or truncate `s` so it occupies exactly `width` display columns.
pub(crate) fn pad_to(s: String, width: usize) -> String {
    if s.chars().count() >= width {
        s.chars().take(width).collect::<String>()
    } else {
        format!("{s:<width$}")
    }
}
