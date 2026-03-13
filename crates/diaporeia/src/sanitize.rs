//! Error message sanitization for MCP responses.
//!
//! Strips server-side details (file system paths) before messages reach clients.

/// Replace absolute file system paths in an error message with `[server path]`.
///
/// Detects Unix absolute paths: a `/` preceded by a word boundary (not another `/`
/// or `:`) and followed by at least one alphanumeric character, `.`, `_`, or `-`.
/// Tracks the last emitted byte across slice boundaries so that `://` in URI
/// schemes (e.g. `aletheia://…`) is never misidentified as a path start.
pub(crate) fn strip_paths(message: &str) -> String {
    let mut result = String::with_capacity(message.len());
    let mut remaining = message;
    // WHY: `remaining` resets its indices each iteration, losing context about
    // the byte immediately before the current slice. Track the last emitted byte
    // so `://` and `//` sequences spanning slice boundaries are handled correctly.
    let mut last_pushed_byte: Option<u8> = None;

    loop {
        let Some(slash_pos) = remaining.find('/') else {
            result.push_str(remaining);
            break;
        };

        // The byte immediately before this '/': either within `remaining`, or
        // the last byte emitted by a previous iteration.
        let prev_byte = if slash_pos > 0 {
            remaining.as_bytes().get(slash_pos - 1).copied()
        } else {
            last_pushed_byte
        };

        // Skip `/` that follows another `/` or `:` — those are URI separators.
        let is_word_boundary = !matches!(prev_byte, Some(b'/' | b':'));

        let after_slash = &remaining[slash_pos + 1..];
        let next_is_path_char = after_slash
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));

        if is_word_boundary && next_is_path_char {
            // Emit everything before the slash, then the replacement.
            result.push_str(&remaining[..slash_pos]);
            result.push_str("[server path]");
            last_pushed_byte = Some(b']');

            // Advance past the entire path token (including any embedded slashes).
            let path_len = after_slash
                .find(|c: char| !c.is_alphanumeric() && !matches!(c, '/' | '_' | '-' | '.'))
                .unwrap_or(after_slash.len());
            remaining = &after_slash[path_len..];
        } else {
            result.push_str(&remaining[..=slash_pos]);
            last_pushed_byte = Some(b'/');
            remaining = after_slash;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_absolute_path_from_message() {
        let msg = "failed to read /home/alice/project/file.rs: No such file or directory";
        let sanitized = strip_paths(msg);
        assert!(!sanitized.contains("/home/alice"));
        assert!(sanitized.contains("[server path]"));
        assert!(sanitized.contains("No such file or directory"));
    }

    #[test]
    fn strips_path_at_start_of_string() {
        let sanitized = strip_paths("/etc/aletheia/config.toml: permission denied");
        assert!(!sanitized.contains("/etc/"));
        assert!(sanitized.starts_with("[server path]"));
    }

    #[test]
    fn preserves_uri_scheme_double_slash() {
        // `://` must not be treated as a path boundary — the ':' preceding '/'
        // means no boundary, and last_pushed_byte carries that across iterations.
        let msg = "unknown resource URI: aletheia://nous/agent1/soul";
        let sanitized = strip_paths(msg);
        assert!(
            sanitized.contains("aletheia://"),
            "URI scheme must be preserved"
        );
    }

    #[test]
    fn double_slash_prefix_is_not_a_path() {
        // A string starting with `//` is not a Unix absolute path.
        let sanitized = strip_paths("//see-the-docs");
        assert!(
            !sanitized.contains("[server path]"),
            "// prefix must not be replaced"
        );
    }

    #[test]
    fn handles_empty_string() {
        assert_eq!(strip_paths(""), "");
    }

    #[test]
    fn handles_string_with_no_paths() {
        let msg = "nous agent not found: some-agent-id";
        assert_eq!(strip_paths(msg), msg);
    }

    #[test]
    fn strips_multiple_paths_in_one_message() {
        let msg = "copy /src/a.rs to /dst/b.rs failed";
        let sanitized = strip_paths(msg);
        assert!(!sanitized.contains("/src/"));
        assert!(!sanitized.contains("/dst/"));
        assert_eq!(
            sanitized.matches("[server path]").count(),
            2,
            "both paths must be replaced"
        );
    }

    #[test]
    fn handles_bare_slash_at_end() {
        // A lone trailing '/' with nothing after it should not be treated as a path.
        let msg = "root is /";
        let sanitized = strip_paths(msg);
        assert_eq!(
            sanitized, "root is /",
            "trailing lone slash must be left unchanged"
        );
    }
}
