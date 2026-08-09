//! Shared unified-diff parsing helpers used by both the QA gate and the
//! steward pipeline.

/// Parse the new-file start line from a unified diff hunk header.
///
/// Format: `@@ -old_start,old_count +new_start,new_count @@`
#[must_use]
pub(crate) fn parse_hunk_new_start(hunk_line: &str) -> Option<u32> {
    let plus_idx = hunk_line.find('+')?;
    let after_plus = hunk_line.get(plus_idx + 1..)?;
    let end = after_plus.find(|c: char| !c.is_ascii_digit())?;
    after_plus.get(..end)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hunk_new_start_normal() {
        assert_eq!(parse_hunk_new_start("@@ -1,3 +1,5 @@"), Some(1));
        assert_eq!(parse_hunk_new_start("@@ -10,2 +42,7 @@"), Some(42));
    }

    #[test]
    fn parse_hunk_new_start_single_line_old() {
        assert_eq!(parse_hunk_new_start("@@ -1 +1,2 @@"), Some(1));
    }

    #[test]
    fn parse_hunk_new_start_single_line_new() {
        // WHY: no comma in the `+` section is the edge case that motivated
        // deduplicating this parser -- both call sites must agree on it.
        assert_eq!(parse_hunk_new_start("@@ -0,0 +1 @@"), Some(1));
    }

    #[test]
    fn parse_hunk_new_start_invalid() {
        assert_eq!(parse_hunk_new_start("not a hunk header"), None);
    }
}
