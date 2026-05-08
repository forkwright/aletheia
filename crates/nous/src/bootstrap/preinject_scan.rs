//! Pre-injection scan for workspace bootstrap files.
//!
//! Scans operator-supplied workspace files (SOUL.md, IDENTITY.md, USER.md,
//! AGENTS.md, etc.) before they are appended to the assembled system prompt.
//! Rejects content containing invisible Unicode or prompt-injection signatures.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use snafu::Snafu;

/// Errors from the pre-injection scan.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum PreInjectError {
    /// Invisible Unicode codepoint detected.
    #[snafu(display("file '{path}' contains invisible-Unicode codepoint {codepoint:?}"))]
    InvisibleUnicode {
        /// Path of the file being scanned.
        path: String,
        /// The offending codepoint.
        codepoint: char,
    },

    /// Threat pattern detected.
    #[snafu(display("file '{path}' matches threat pattern '{pattern}'"))]
    ThreatPattern {
        /// Path of the file being scanned.
        path: String,
        /// Description of the matched pattern.
        pattern: String,
    },
}

/// Scan workspace file content before injection into the system prompt.
///
/// Returns `Ok(())` when the content passes both the invisible-Unicode and
/// threat-pattern checks. Returns `Err` on the first detected violation.
///
/// # Invisible-Unicode scan
///
/// Rejects content containing zero-width spaces, bidi control characters,
/// word joiners, and other invisible codepoints that can be used to hide
/// malicious text or alter rendering.
///
/// # Threat-pattern scan
///
/// Rejects content matching known prompt-injection signatures. Patterns are
/// compiled once via [`OnceLock`] and matched case-insensitively.
pub fn scan_workspace_content(content: &str, path: &Path) -> Result<(), PreInjectError> {
    scan_invisible_unicode(content, path)?;
    scan_threat_patterns(content, path)?;
    Ok(())
}

/// Check whether strict mode is enabled via the `KOINA_PREINJECT_SCAN_STRICT`
/// environment variable. Default is `false` (lenient: log + skip).
#[must_use]
pub fn strict_mode() -> bool {
    strict_mode_from_env(std::env::var("KOINA_PREINJECT_SCAN_STRICT").ok())
}

/// Parse the strict-mode env var value.
///
/// Extracted so tests can verify logic without mutating process state.
#[must_use]
pub fn strict_mode_from_env(val: Option<String>) -> bool {
    val.is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1")
}

fn scan_invisible_unicode(content: &str, path: &Path) -> Result<(), PreInjectError> {
    for ch in content.chars() {
        if is_invisible_unicode(ch) {
            return InvisibleUnicodeSnafu {
                path: path.display().to_string(),
                codepoint: ch,
            }
            .fail();
        }
    }
    Ok(())
}

/// Whether a codepoint is in the blocked invisible-Unicode set.
///
/// Blocks:
/// - U+200B zero-width space
/// - U+200C zero-width non-joiner
/// - U+200D zero-width joiner
/// - U+2060 word joiner
/// - U+FEFF zero-width no-break space (BOM)
/// - U+202A–U+202E bidi control characters
fn is_invisible_unicode(ch: char) -> bool {
    matches!(
        ch,
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}'
    ) || ('\u{202A}'..='\u{202E}').contains(&ch)
}

fn scan_threat_patterns(content: &str, path: &Path) -> Result<(), PreInjectError> {
    let patterns = threat_patterns();
    for (name, regex) in patterns {
        if regex.is_match(content) {
            return ThreatPatternSnafu {
                path: path.display().to_string(),
                pattern: (*name).to_owned(),
            }
            .fail();
        }
    }
    Ok(())
}

/// Threat patterns compiled lazily via [`OnceLock`].
///
/// WHY: `OnceLock` avoids recompiling regexes on every scan. The patterns are
/// static, so a single global initialization is sufficient.
fn threat_patterns() -> &'static [(&'static str, Regex)] {
    static PATTERNS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                "ignore-instructions",
                compile_regex(r"(?i)ignore (all|previous|the above) (instructions|prompt)"),
            ),
            ("role-override", compile_regex(r"(?i)you are now (a|an) ")),
            (
                "disregard-instructions",
                compile_regex(r"(?i)disregard (all|previous|the above)"),
            ),
            ("system-tag", compile_regex(r"(?i)<(system|admin)>")),
            (
                "admin-prefix",
                compile_regex(r"(?i)\[(system|admin|root)\]:"),
            ),
        ]
    })
}

/// Compile a regex literal known at compile time to be valid.
fn compile_regex(re: &str) -> Regex {
    #[expect(
        clippy::expect_used,
        reason = "compile-time-constant regex literals cannot fail"
    )]
    {
        Regex::new(re).expect("compile-time-constant regex literals cannot fail")
    }
}
