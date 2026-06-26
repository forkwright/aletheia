#![deny(missing_docs)]
//! poiesis-lint: report prose quality linting.
//!
//! Checks banned words, citation coverage, structural patterns, required
//! sections, and header length. Ported from a prior private project, used with
//! permission.

mod banned_words;
mod citations;
/// Error types for the lint pipeline.
pub mod error;
mod raw_latex;
mod structure;

pub use error::LintError;
/// Export-target selector for document portability linting.
pub use raw_latex::ExportTarget;
/// Lint raw `LaTeX` portability against a target export format.
pub use raw_latex::check_raw_latex_nonportable;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

// ── Finding types ─────────────────────────────────────────────────────────────

/// A single lint finding with location and message.
#[derive(Debug, Serialize, Deserialize)]
pub struct Finding {
    /// 1-indexed first line of the finding.
    pub line_start: usize,
    /// 1-indexed last line of the finding (same as `line_start` for single-line findings).
    pub line_end: usize,
    /// Human-readable description of the issue.
    pub message: String,
    /// Category of this finding.
    pub kind: FindingKind,
    /// Auto-fix data, if this finding can be fixed automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<LineFix>,
}

/// Category of a lint finding.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum FindingKind {
    /// A banned word or phrase was found.
    BannedWord,
    /// A table or data display lacks a nearby citation.
    MissingCitation,
    /// An AI structural tell (e.g. transition density) was detected.
    StructuralPattern,
    /// A required section (lead or closing) is absent.
    RequiredSectionMissing,
    /// A heading exceeds the allowed length.
    HeaderLength,
    /// Raw `LaTeX` was found in a document exported to a non-`LaTeX` target.
    RawLatexNonPortable,
}

/// Data needed to apply an automatic fix for a banned word.
#[derive(Debug, Serialize, Deserialize)]
pub struct LineFix {
    /// 1-indexed line number where the match occurred.
    pub line_number: usize,
    /// The original matched text (may differ in case from the pattern).
    pub matched: String,
    /// Replacement text to write back.
    pub replacement: String,
}

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for the lint pipeline.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag independently enables a distinct checker; this is a configuration struct not a state machine"
)]
pub struct LintConfig {
    /// Enable banned word checks.
    pub check_banned_words: bool,
    /// Enable citation presence checks.
    pub check_citations: bool,
    /// Enable structural pattern checks.
    pub check_structure: bool,
    /// Enable required section checks.
    pub check_sections: bool,
    /// Enable header length checks.
    pub check_header_length: bool,
    /// Maximum H2 heading length in characters.
    pub max_header_length: usize,
    /// Number of lines before/after a table to search for citations.
    pub citation_window: usize,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            check_banned_words: true,
            check_citations: true,
            check_structure: true,
            check_sections: true,
            check_header_length: true,
            max_header_length: 60,
            citation_window: 10,
        }
    }
}

// ── Linter ────────────────────────────────────────────────────────────────────

/// Stateless report linter. Construct once; call `check` for each document.
pub struct Linter {
    config: LintConfig,
}

impl Linter {
    /// Create a new `Linter` with the given configuration.
    pub fn new(config: LintConfig) -> Self {
        Self { config }
    }

    /// Check `text` for all configured lint rules.
    ///
    /// Returns findings sorted by line number (top-to-bottom, deterministic).
    pub fn check(&self, text: &str) -> Vec<Finding> {
        let all_lines: Vec<(usize, &str)> =
            text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();

        // WHY: structural and banned-word checks run on non-comment lines only.
        // Citation checks run on all lines because source markers may appear
        // adjacent to comment lines.
        let cleaned = strip_comments(text);
        let cleaned_lines: Vec<(usize, &str)> = cleaned
            .lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l))
            .collect();
        let effective: Vec<(usize, &str)> = cleaned_lines
            .iter()
            .copied()
            .filter(|(_, l)| !l.trim().is_empty())
            .collect();

        let mut findings: Vec<Finding> = Vec::new();

        if self.config.check_banned_words {
            findings.extend(banned_words::check(&effective));
        }
        if self.config.check_citations {
            findings.extend(citations::check(&all_lines, self.config.citation_window));
        }
        if self.config.check_structure {
            // WHY: preserve blank lines so paragraph-boundary resets in
            // check_structure can fire; comments are already stripped to spaces.
            findings.extend(structure::check_structure(&cleaned_lines));
        }
        if self.config.check_sections {
            findings.extend(structure::check_sections(&effective));
        }
        if self.config.check_header_length {
            findings.extend(structure::check_header_length(
                &effective,
                self.config.max_header_length,
            ));
        }

        findings.sort_by_key(|f| f.line_start);
        findings
    }

    /// Apply all auto-fixable findings to `text` and return the modified content.
    ///
    /// Only findings with a `fix` field are processed; unfixable findings are ignored.
    pub fn apply_fixes(&self, text: &str, findings: &[Finding]) -> String {
        let fixable: Vec<&LineFix> = findings.iter().filter_map(|f| f.fix.as_ref()).collect();

        if fixable.is_empty() {
            return text.to_owned();
        }

        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();

        for fix in fixable {
            // line_number is 1-indexed; saturate to avoid underflow on malformed input.
            let idx = fix.line_number.saturating_sub(1);
            if let Some(line) = lines.get_mut(idx) {
                *line = replace_first_case_insensitive(line, &fix.matched, &fix.replacement);
            }
        }

        // WHY: preserve trailing newline from the original file to avoid spurious diffs.
        let mut new_content = lines.join("\n");
        if text.ends_with('\n') {
            new_content.push('\n');
        }
        new_content
    }

    /// Lint a file at `path`, optionally applying fixes in place.
    ///
    /// Returns the list of findings. If `apply_fix` is true and any fixable
    /// findings exist, the file is rewritten with fixes applied.
    ///
    /// # Errors
    ///
    /// Returns `LintError` if the file cannot be read or written.
    pub fn check_file(
        &self,
        path: &std::path::Path,
        apply_fix: bool,
    ) -> Result<Vec<Finding>, LintError> {
        let content = std::fs::read_to_string(path).context(error::ReadFileSnafu {
            path: path.display().to_string(),
        })?;

        let findings = self.check(&content);

        if apply_fix && !findings.is_empty() {
            let fixed = self.apply_fixes(&content, &findings);
            std::fs::write(path, fixed).context(error::WriteFileSnafu {
                path: path.display().to_string(),
            })?;
        }

        Ok(findings)
    }

    /// Serialize `findings` to a pretty-printed JSON string.
    ///
    /// # Errors
    ///
    /// Returns `LintError::Serialize` if serialization fails.
    pub fn to_json(findings: &[Finding]) -> Result<String, LintError> {
        serde_json::to_string_pretty(findings).context(error::SerializeSnafu)
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::new(LintConfig::default())
    }
}

/// Return a copy of `text` with Rust/Typst line comments and block comments
/// replaced by spaces, preserving line lengths and newline structure so that
/// line numbers stay aligned.
///
/// Preserves `//` inside URLs (e.g. `http://`) by checking whether the
/// preceding character is `:`.
fn strip_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_block = false;

    for line in text.lines() {
        if in_block {
            if let Some(end) = line.find("*/") {
                in_block = false;
                let spaces = " ".repeat(end + 2);
                result.push_str(&spaces);
                if let Some(tail) = line.get(end + 2..) {
                    result.push_str(tail);
                }
                result.push('\n');
            } else {
                result.push_str(&" ".repeat(line.len()));
                result.push('\n');
            }
            continue;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("//!") || trimmed.starts_with("///") {
            result.push_str(&" ".repeat(line.len()));
            result.push('\n');
            continue;
        }

        let mut cleaned = String::with_capacity(line.len());
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if i + 1 < bytes.len() {
                let here = bytes.get(i).copied();
                let next = bytes.get(i + 1).copied();
                if here == Some(b'/') && next == Some(b'*') {
                    if let Some(remaining) = line.get(i + 2..)
                        && let Some(end) = remaining.find("*/")
                    {
                        cleaned.push_str(&" ".repeat(2 + end + 2));
                        i += 2 + end + 2;
                        continue;
                    }
                    in_block = true;
                    cleaned.push_str(&" ".repeat(line.len() - i));
                    break;
                }
                if here == Some(b'/') && next == Some(b'/') {
                    // Avoid stripping // inside URLs like http://
                    let prev = if i >= 1 {
                        bytes.get(i - 1).copied()
                    } else {
                        None
                    };
                    if prev == Some(b':') {
                        cleaned.push('/');
                        cleaned.push('/');
                        i += 2;
                        continue;
                    }
                    cleaned.push_str(&" ".repeat(line.len() - i));
                    break;
                }
            }
            if let Some(b) = bytes.get(i).copied() {
                cleaned.push(char::from(b));
            }
            i += 1;
        }

        result.push_str(&cleaned);
        result.push('\n');
    }

    if text.ends_with('\n') || result.is_empty() {
        result
    } else {
        result.pop();
        result
    }
}

/// Replace the first occurrence of `pattern` in `line` (case-insensitively)
/// with `replacement`, preserving surrounding characters.
fn replace_first_case_insensitive(line: &str, pattern: &str, replacement: &str) -> String {
    let lower = line.to_lowercase();
    let lower_pattern = pattern.to_lowercase();

    if let Some(pos) = lower.find(lower_pattern.as_str()) {
        let end = pos + pattern.len();
        if line.is_char_boundary(pos) && line.is_char_boundary(end) {
            let mut result = String::with_capacity(line.len());
            // SAFETY: pos and end are verified as char boundaries immediately above
            #[expect(
                clippy::string_slice,
                reason = "pos and end are verified as char boundaries immediately above"
            )]
            result.push_str(&line[..pos]); // kanon:ignore RUST/indexing-slicing — pos verified as char boundary immediately above
            result.push_str(replacement);
            #[expect(
                clippy::string_slice,
                reason = "pos and end are verified as char boundaries immediately above"
            )]
            result.push_str(&line[end..]); // kanon:ignore RUST/indexing-slicing — end verified as char boundary immediately above
            return result;
        }
    }

    line.to_owned()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_linter_finds_banned_words() {
        let linter = Linter::default();
        let findings = linter.check("The approach is robust and comprehensive.\n");
        assert!(
            !findings.is_empty(),
            "default linter must flag banned words"
        );
    }

    #[test]
    fn clean_text_produces_no_findings() {
        let linter = Linter {
            config: LintConfig {
                check_citations: false,
                check_sections: false,
                ..LintConfig::default()
            },
        };
        let findings = linter.check("The analysis shows 47 cases across three employers.\n");
        assert!(
            findings.is_empty(),
            "clean text must produce no findings, got: {findings:?}"
        );
    }

    #[test]
    fn apply_fixes_replaces_utilize() {
        let linter = Linter::default();
        let text = "We utilize the data pipeline.\n";
        let findings = linter.check(text);
        let fixed = linter.apply_fixes(text, &findings);
        assert!(
            fixed.contains("use"),
            "fix must replace 'utilize' with 'use', got: {fixed:?}"
        );
    }

    #[test]
    fn to_json_round_trips() {
        let findings = vec![Finding {
            line_start: 5,
            line_end: 5,
            message: "banned word \"robust\"".to_owned(),
            kind: FindingKind::BannedWord,
            fix: None,
        }];
        let json = Linter::to_json(&findings).expect("serialize");
        let parsed: Vec<Finding> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.len(), 1, "round-trip must preserve finding count");
    }

    #[test]
    fn replace_first_case_insensitive_replaces() {
        let result = replace_first_case_insensitive("We UTILIZE the data.", "UTILIZE", "use");
        assert_eq!(result, "We use the data.", "must replace matched text");
    }

    #[test]
    fn replace_first_case_insensitive_no_match() {
        let line = "Clean prose here.";
        let result = replace_first_case_insensitive(line, "utilize", "use");
        assert_eq!(result, line, "no match must return line unchanged");
    }

    #[test]
    fn comment_lines_are_filtered() {
        let linter = Linter::default();
        let text =
            "// this is a line comment\n/// doc comment\n//! inner doc\nrobust prose here.\n";
        let findings = linter.check(text);
        assert!(
            findings.iter().any(|f| f.message.contains("robust")),
            "must still flag banned word in prose"
        );
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.message.contains("robust"))
                .count(),
            1,
            "must flag exactly one 'robust' in prose, not in comments"
        );
    }

    #[test]
    fn block_comments_are_filtered() {
        let linter = Linter::default();
        let text = "/* robust block comment */\nprose robust here.\n/* multi\nrobust line\n*/\nmore prose.\n";
        let findings = linter.check(text);
        assert!(
            findings.iter().any(|f| f.message.contains("robust")),
            "must flag banned word in prose"
        );
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.message.contains("robust"))
                .count(),
            1,
            "must flag exactly one 'robust' in prose, not in block comments"
        );
    }

    #[test]
    fn url_double_slash_is_preserved() {
        let linter = Linter::default();
        let text = "Visit https://example.com/robust for details.\n";
        let findings = linter.check(text);
        assert!(
            findings.iter().any(|f| f.message.contains("robust")),
            "must flag banned word inside URL path"
        );
    }

    #[test]
    fn blank_lines_reset_transition_density() {
        let linter = Linter {
            config: LintConfig {
                check_banned_words: false,
                check_citations: false,
                check_sections: false,
                check_header_length: false,
                ..LintConfig::default()
            },
        };
        let text = "Furthermore, X.\n\nAdditionally, Y.\n\nMoreover, Z.\n";
        let findings = linter.check(text);
        assert!(
            findings
                .iter()
                .all(|f| f.kind != FindingKind::StructuralPattern),
            "blank-line-separated transitions must not produce a structural finding: {findings:?}"
        );
    }
}
