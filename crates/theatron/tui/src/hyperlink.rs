//! OSC 8 terminal hyperlink support.

use std::sync::LazyLock;

use regex::Regex;

/// A link found during markdown rendering, positioned within the markdown lines.
///
/// Line index and column are **relative** to the `Vec<Line>` returned by
/// [`crate::markdown::render`] and do **not** include the content-prefix span
/// that the chat view prepends to each line.
#[derive(Debug, Clone)]
pub(crate) struct MdLink {
    /// Zero-based index into the returned `Vec<Line>`.
    pub line_idx: usize,
    /// Display column within that line (byte-based, content-prefix excluded).
    pub col: u16,
    /// Visible display text of the link (for re-rendering with OSC 8).
    pub text: String,
    /// Target URL.
    pub url: String,
}

/// A hyperlink fully resolved to absolute screen coordinates, ready to emit.
#[derive(Debug, Clone)]
pub(crate) struct OscLink {
    /// Absolute terminal column (0-based).
    pub screen_x: u16,
    /// Absolute terminal row (0-based).
    pub screen_y: u16,
    /// Visible display text.
    pub text: String,
    /// Target URL.
    pub url: String,
    /// Accent colour (R, G, B) to apply when re-writing the link text.
    pub accent: (u8, u8, u8),
}

/// Format an OSC 8 hyperlink **opening** sequence.
///
/// Format: `ESC ] 8 ;; URL BEL`
pub fn osc8_open(url: &str) -> String {
    format!("\x1b]8;;{url}\x07")
}

/// OSC 8 hyperlink **closing** sequence: `ESC ] 8 ;; BEL`
pub const fn osc8_close() -> &'static str {
    "\x1b]8;;\x07"
}

/// Returns `true` if the running terminal is known to support OSC 8 hyperlinks.
///
/// Detection is cached on first call (env vars are read once). Terminals that
/// support OSC 8 as of March 2026: Ghostty, iTerm2 ≥ 3.x, WezTerm, Kitty,
/// Windows Terminal, foot, Alacritty ≥ 0.14.
///
/// Terminals that do **not** support it: raw Linux console, most older xterm
/// derivatives. Callers should degrade gracefully (underline + colour only).
pub fn supports_hyperlinks() -> bool {
    static CACHE: LazyLock<bool> = LazyLock::new(probe_hyperlink_support);
    *CACHE
}

fn probe_hyperlink_support() -> bool {
    // NOTE: TERM_PROGRAM: most reliable signal on macOS and some Linux terminals
    if let Ok(prog) = std::env::var("TERM_PROGRAM") {
        match prog.as_str() {
            "iTerm.app" | "WezTerm" | "ghostty" | "Ghostty" | "kitty" => return true,
            // NOTE: unrecognized TERM_PROGRAM, continue probing other signals
            _ => {}
        }
    }

    // NOTE: Ghostty
    if std::env::var("GHOSTTY_BIN_DIR").is_ok() || std::env::var("GHOSTTY_RESOURCES_DIR").is_ok() {
        return true;
    }

    // NOTE: WezTerm
    if std::env::var("WEZTERM_EXECUTABLE").is_ok() || std::env::var("WEZTERM_PANE").is_ok() {
        return true;
    }

    // NOTE: Kitty
    if std::env::var("KITTY_PID").is_ok() || std::env::var("KITTY_WINDOW_ID").is_ok() {
        return true;
    }

    // NOTE: Windows Terminal
    if std::env::var("WT_SESSION").is_ok() {
        return true;
    }

    // NOTE: foot
    if let Ok(term) = std::env::var("TERM")
        && (term == "foot" || term == "foot-extra")
    {
        return true;
    }

    // NOTE: Alacritty
    if std::env::var("ALACRITTY_SOCKET").is_ok() {
        return true;
    }

    false
}

/// Extract HTTP/HTTPS URLs from plain text.
///
/// Returns `(start_byte, end_byte, url_str)` tuples.
///
/// **Trailing punctuation** (`.`, `,`, `;`, `!`, `?`, `:`) is stripped.
/// A trailing `)` is stripped only when the URL contains fewer `(` than `)`.
///
/// The caller is responsible for skipping code blocks and inline code;
/// those contexts should not be passed to this function.
pub fn detect_urls(text: &str) -> Vec<(usize, usize, &str)> {
    #[expect(
        clippy::expect_used,
        reason = "regex is a compile-time string literal and is always valid"
    )]
    static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"https?://[^\s<>{}|\\^`\[\]'"]+"#).expect("hyperlink URL regex is valid")
    });

    let mut out = Vec::new();
    for m in URL_RE.find_iter(text) {
        let start = m.start();
        let trimmed_len = trim_trailing_punct(text.get(start..m.end()).unwrap_or(""));
        let end = start + trimmed_len;
        if end > start {
            out.push((start, end, text.get(start..end).unwrap_or("")));
        }
    }
    out
}

/// Returns the byte length of `url` with trailing punctuation stripped.
fn trim_trailing_punct(url: &str) -> usize {
    let bytes = url.as_bytes();
    let mut end = bytes.len();
    loop {
        if end == 0 {
            break;
        }
        match bytes[end - 1] {
            b'.' | b',' | b';' | b'!' | b'?' | b':' => end -= 1,
            b')' => {
                let sub = url.get(..end).unwrap_or("");
                let opens = sub.bytes().filter(|&b| b == b'(').count();
                let closes = sub.bytes().filter(|&b| b == b')').count();
                if closes > opens {
                    end -= 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    end
}

/// Detect `path/to/file.rs` and `path/to/file.rs:LINE` patterns in text.
///
/// Returns `(start, end, path, url)` where `url` is a `file://` URL suitable
/// for OSC 8. Only relative paths with known source extensions are matched to
/// avoid false positives on arbitrary words.
///
/// Not yet called by the renderer; kept in `#[cfg(test)]` until wired in.
#[cfg(test)]
fn detect_file_paths(text: &str) -> Vec<(usize, usize, &str, String)> {
    #[expect(
        clippy::expect_used,
        reason = "regex is a compile-time string literal and is always valid"
    )]
    static PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
        // NOTE: crates/foo/src/bar.rs:142  or  src/foo.rs  or  ./src/foo.ts
        Regex::new(r"(?:\.{0,2}/)?(?:[a-zA-Z0-9_\-]+/)+[a-zA-Z0-9_\-]+\.[a-zA-Z]{1,6}(?::[0-9]+)?")
            .expect("file path regex is valid")
    });

    static SOURCE_EXTS: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "c", "cpp", "h", "java", "kt", "swift", "rb",
        "toml", "yaml", "yml", "json", "md", "txt",
    ];

    let mut out = Vec::new();
    for m in PATH_RE.find_iter(text) {
        let s = m.as_str();
        let base = s.split(':').next().unwrap_or(s);
        let ext = base.rsplit('.').next().unwrap_or("");
        if !SOURCE_EXTS.contains(&ext) {
            continue;
        }
        let path_only = base.trim_start_matches("./");
        let url = format!("file://{path_only}");
        out.push((m.start(), m.end(), m.as_str(), url));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_https_url() {
        let urls = detect_urls("See https://example.com for details");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://example.com");
    }

    #[test]
    fn detects_http_url() {
        let urls = detect_urls("Go to http://example.com");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "http://example.com");
    }

    #[test]
    fn strips_trailing_dot() {
        let urls = detect_urls("Visit https://example.com. More text.");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://example.com");
    }

    #[test]
    fn strips_trailing_comma() {
        let urls = detect_urls("See https://example.com, then proceed");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://example.com");
    }

    #[test]
    fn strips_trailing_semicolon() {
        let urls = detect_urls("Done https://example.com; next");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://example.com");
    }

    #[test]
    fn strips_trailing_colon() {
        let urls = detect_urls("Source: https://example.com:");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://example.com");
    }

    #[test]
    fn strips_unbalanced_closing_paren() {
        let urls = detect_urls("(see https://example.com)");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://example.com");
    }

    #[test]
    fn keeps_balanced_parens_in_url() {
        let url = "https://en.wikipedia.org/wiki/Rust_(programming_language)";
        let urls = detect_urls(url);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, url);
    }

    #[test]
    fn detects_multiple_urls() {
        let text = "Visit https://one.com and https://two.org today";
        let urls = detect_urls(text);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].2, "https://one.com");
        assert_eq!(urls[1].2, "https://two.org");
    }

    #[test]
    fn no_urls_returns_empty() {
        assert!(detect_urls("just plain text here").is_empty());
    }

    #[test]
    fn detects_url_with_path_and_query() {
        let urls = detect_urls("See https://docs.anthropic.com/en/docs/agents?v=2 here");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://docs.anthropic.com/en/docs/agents?v=2");
    }

    #[test]
    fn url_positions_are_correct() {
        let text = "hello https://foo.com world";
        let urls = detect_urls(text);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].0, 6);
        assert_eq!(
            text.get(urls[0].0..urls[0].1).unwrap_or(""),
            "https://foo.com"
        );
    }

    #[test]
    fn strips_multiple_trailing_chars() {
        let urls = detect_urls("See https://foo.com/bar.,");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].2, "https://foo.com/bar");
    }

    #[test]
    fn osc8_open_format() {
        let seq = osc8_open("https://example.com");
        assert_eq!(seq, "\x1b]8;;https://example.com\x07");
    }

    #[test]
    fn osc8_close_format() {
        assert_eq!(osc8_close(), "\x1b]8;;\x07");
    }

    #[test]
    fn osc8_open_starts_with_esc() {
        assert!(osc8_open("https://x.com").starts_with('\x1b'));
    }

    #[test]
    fn osc8_open_ends_with_bel() {
        assert!(osc8_open("https://x.com").ends_with('\x07'));
    }

    #[test]
    fn osc8_close_ends_with_bel() {
        assert!(osc8_close().ends_with('\x07'));
    }

    #[test]
    fn osc8_full_wraps_text() {
        let full = format!(
            "{}{}{}",
            osc8_open("https://example.com"),
            "click me",
            osc8_close()
        );
        assert_eq!(full, "\x1b]8;;https://example.com\x07click me\x1b]8;;\x07");
    }

    #[test]
    fn supports_hyperlinks_returns_bool() {
        // NOTE: function must not panic regardless of environment
        let _ = supports_hyperlinks();
    }

    #[test]
    #[expect(
        unsafe_code,
        reason = "test-only env mutation in single-threaded test context"
    )]
    fn probe_detects_ghostty_resources_dir() {
        // WHY: Use the raw probe (not cached) to test detection logic.
        // SAFETY: test-only env mutation; env vars are not read concurrently here.
        unsafe { std::env::set_var("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty") };
        let result = probe_hyperlink_support();
        unsafe { std::env::remove_var("GHOSTTY_RESOURCES_DIR") };
        assert!(result, "should detect Ghostty via GHOSTTY_RESOURCES_DIR");
    }

    #[test]
    #[expect(
        unsafe_code,
        reason = "test-only env mutation in single-threaded test context"
    )]
    fn probe_detects_wezterm_pane() {
        // SAFETY: test-only env mutation; env vars are not read concurrently here.
        unsafe { std::env::set_var("WEZTERM_PANE", "1") };
        let result = probe_hyperlink_support();
        unsafe { std::env::remove_var("WEZTERM_PANE") };
        assert!(result, "should detect WezTerm via WEZTERM_PANE");
    }

    #[test]
    #[expect(
        unsafe_code,
        reason = "test-only env mutation in single-threaded test context"
    )]
    fn probe_detects_kitty_window_id() {
        // SAFETY: test-only env mutation; env vars are not read concurrently here.
        unsafe { std::env::set_var("KITTY_WINDOW_ID", "3") };
        let result = probe_hyperlink_support();
        unsafe { std::env::remove_var("KITTY_WINDOW_ID") };
        assert!(result, "should detect Kitty via KITTY_WINDOW_ID");
    }

    #[test]
    #[expect(
        unsafe_code,
        reason = "test-only env mutation in single-threaded test context"
    )]
    fn probe_detects_windows_terminal() {
        // SAFETY: test-only env mutation; env vars are not read concurrently here.
        unsafe { std::env::set_var("WT_SESSION", "some-uuid") };
        let result = probe_hyperlink_support();
        unsafe { std::env::remove_var("WT_SESSION") };
        assert!(result, "should detect Windows Terminal via WT_SESSION");
    }

    #[test]
    fn detects_rust_file_path() {
        let paths = detect_file_paths("See crates/nous/src/actor.rs:142 for details");
        assert_eq!(paths.len(), 1);
        assert!(paths[0].3.starts_with("file://"));
        assert!(paths[0].3.contains("actor.rs"));
    }

    #[test]
    fn ignores_non_source_extension() {
        // NOTE: .bin files are not in SOURCE_EXTS
        let paths = detect_file_paths("target/debug/aletheia.bin");
        assert!(paths.is_empty());
    }
}
