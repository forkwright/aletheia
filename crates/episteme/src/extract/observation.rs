//! Observation parsing from PR body markdown.
//!
//! Extracts individual observations from the `## Observations` section
//! of a PR body. Each bullet point becomes a `RawObservation` with
//! optional tags inferred from the text (crate names, file paths) and
//! a classified `ObservationType`.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Classification of an observation.
///
/// WHY: Without classification, all observations are treated identically.
/// Bugs need immediate attention; debt/ideas should be batched into digests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ObservationType {
    /// A defect in existing code: crash, panic, error, wrong behavior.
    Bug,
    /// Technical debt: refactoring opportunity, cleanup, code smell.
    Debt,
    /// A new idea or improvement suggestion.
    Idea,
    /// Missing or inadequate test coverage.
    MissingTest,
    /// Missing or outdated documentation.
    DocGap,
}

impl ObservationType {
    /// Classify observation text using keyword matching.
    ///
    /// Priority order: `Bug` > `MissingTest` > `DocGap` > `Debt` > `Idea` (default).
    /// Bug keywords are checked first because misclassifying a bug is costlier
    /// than misclassifying an idea.
    #[must_use]
    pub fn classify(text: &str) -> Self {
        let lower = text.to_lowercase();

        if contains_any(
            &lower,
            &[
                "bug",
                "crash",
                "panic",
                "error",
                "fail",
                "broken",
                "wrong",
                "incorrect",
                "regression",
                "null pointer",
                "segfault",
                "undefined behavior",
                "data loss",
                "infinite loop",
                "deadlock",
                "race condition",
            ],
        ) {
            return Self::Bug;
        }

        if contains_any(
            &lower,
            &[
                "no test",
                "missing test",
                "untested",
                "no coverage",
                "test coverage",
                "needs test",
                "add test",
                "without test",
            ],
        ) {
            return Self::MissingTest;
        }

        if contains_any(
            &lower,
            &[
                "undocumented",
                "no doc",
                "missing doc",
                "doc gap",
                "needs doc",
                "outdated doc",
                "stale doc",
                "add doc",
            ],
        ) {
            return Self::DocGap;
        }

        if contains_any(
            &lower,
            &[
                "refactor",
                "cleanup",
                "clean up",
                "tech debt",
                "technical debt",
                "deprecated",
                "legacy",
                "duplication",
                "duplicate code",
                "dead code",
                "unused",
                "complexity",
                "simplify",
                "too long",
                "god class",
                "code smell",
            ],
        ) {
            return Self::Debt;
        }

        Self::Idea
    }

    /// Database string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Debt => "debt",
            Self::Idea => "idea",
            Self::MissingTest => "missing_test",
            Self::DocGap => "doc_gap",
        }
    }

    /// Parse from database string representation.
    ///
    /// Unknown strings fall back to [`Self::Idea`] — the most permissive
    /// classification — so legacy rows from older schemas don't break.
    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "explicit `\"idea\" => Idea` arm + `_ => Idea` fallback is intentional: documents that 'idea' is a known value AND the fallback target"
    )]
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "bug" => Self::Bug,
            "debt" => Self::Debt,
            "idea" => Self::Idea,
            "missing_test" => Self::MissingTest,
            "doc_gap" => Self::DocGap,
            _ => Self::Idea,
        }
    }
}

impl fmt::Display for ObservationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

/// A raw observation extracted from a PR body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RawObservation {
    /// The observation text, trimmed of leading bullet markers.
    pub text: String,
    /// Tags extracted from the text (crate names, file paths).
    pub tags: Vec<String>,
    /// Classified observation type.
    pub observation_type: ObservationType,
}

/// Parse the `## Observations` section from a PR body.
///
/// Extracts each bullet point (starting with `-` or `*`) as an
/// individual observation. Handles continuation lines (indented text
/// that is part of the same bullet).
#[must_use]
pub fn parse_observations(pr_body: &str) -> Vec<RawObservation> {
    let Some(section) = extract_observations_section(pr_body) else {
        return Vec::new();
    };

    let bullets = extract_bullets(&section);

    bullets
        .into_iter()
        .filter(|b| !b.is_empty())
        .map(|text| {
            let tags = extract_tags(&text);
            let observation_type = ObservationType::classify(&text);
            RawObservation {
                text,
                tags,
                observation_type,
            }
        })
        .collect()
}

/// Extract the text between `## Observations` and the next `##` header or end of body.
fn extract_observations_section(pr_body: &str) -> Option<String> {
    let lines: Vec<&str> = pr_body.lines().collect();
    let mut start = None;
    let mut end = lines.len();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if start.is_none() {
            if trimmed.starts_with("## Observations") {
                start = Some(i + 1);
            }
        } else if trimmed.starts_with("## ") {
            end = i;
            break;
        }
    }

    let start = start?;
    if start >= end {
        return None;
    }

    // WHY: `end` starts at `lines.len()` and is only ever reassigned to a valid
    // enumerate index `i < lines.len()`, so `end <= lines.len()`. Combined with
    // the `start >= end` early-return above, the slice `start..end` is always
    // in bounds.
    #[expect(
        clippy::indexing_slicing,
        reason = "start in (None, lines.len()), end in (lines.len() or i < lines.len()); start < end checked above"
    )]
    let section: String = lines[start..end].join("\n");
    if section.trim().is_empty() {
        return None;
    }

    Some(section)
}

/// Extract bullet points from the observations section text.
///
/// Handles continuation lines (non-bullet lines that are part of the
/// same bullet) and code blocks within bullets.
fn extract_bullets(section: &str) -> Vec<String> {
    let mut bullets: Vec<String> = Vec::new();
    let mut current_bullet: Option<String> = None;
    let mut in_code_block = false;

    for line in section.lines() {
        let trimmed = line.trim();

        // Track fenced code blocks to avoid splitting on inner bullets.
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            if let Some(ref mut bullet) = current_bullet {
                bullet.push('\n');
                bullet.push_str(trimmed);
            }
            continue;
        }

        if in_code_block {
            if let Some(ref mut bullet) = current_bullet {
                bullet.push('\n');
                bullet.push_str(trimmed);
            }
            continue;
        }

        if let Some(text) = strip_bullet_prefix(trimmed) {
            if let Some(bullet) = current_bullet.take() {
                bullets.push(bullet.trim().to_owned());
            }
            current_bullet = Some(text.to_owned());
        } else if trimmed.is_empty() {
            if let Some(bullet) = current_bullet.take() {
                bullets.push(bullet.trim().to_owned());
            }
        } else if current_bullet.is_some() {
            // Continuation line — append to current bullet.
            if let Some(ref mut bullet) = current_bullet {
                bullet.push(' ');
                bullet.push_str(trimmed);
            }
        }
    }

    // Flush the last bullet if present.
    if let Some(bullet) = current_bullet {
        let trimmed = bullet.trim().to_owned();
        if !trimmed.is_empty() {
            bullets.push(trimmed);
        }
    }

    bullets
}

/// Strip a bullet prefix (`- ` or `* `) from a line.
fn strip_bullet_prefix(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("- ") {
        Some(rest)
    } else if let Some(rest) = line.strip_prefix("* ") {
        Some(rest)
    } else {
        None
    }
}

// INVARIANT: compile-time regexes — patterns are validated, will not panic.
#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static CRATE_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"crates/([\w][\w-]*)").expect("valid regex pattern"));
#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static BACKTICK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([\w][\w-]*)`").expect("valid regex pattern"));
#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static FILE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s|`)((?:src|crates|tests)/[\w/.-]+\.(?:rs|toml|md))(?:\s|`|$|,|\.)")
        .expect("valid regex pattern")
});

/// Extract tags from observation text.
///
/// Tags include crate names matching `crates/{name}` or backtick-wrapped
/// crate references, and file paths matching common patterns.
#[must_use]
pub fn extract_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();

    for cap in CRATE_PATH_RE.captures_iter(text) {
        let Some(m) = cap.get(1) else { continue };
        let name = m.as_str().to_owned();
        if !tags.contains(&name) {
            tags.push(name);
        }
    }

    for cap in BACKTICK_RE.captures_iter(text) {
        let Some(m) = cap.get(1) else { continue };
        let name = m.as_str().to_owned();
        if name.contains('-') && !tags.contains(&name) {
            tags.push(name);
        }
    }

    for cap in FILE_PATH_RE.captures_iter(text) {
        let Some(m) = cap.get(1) else { continue };
        let path = m.as_str().to_owned();
        if !tags.contains(&path) {
            tags.push(path);
        }
    }

    tags
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions on collections with known length")]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_body() {
        let result = parse_observations("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_no_observations_section() {
        let body = "## Summary\nSome changes\n\n## Test plan\n- Run tests";
        let result = parse_observations(body);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_simple_observations() {
        let body = "\
## Summary
Some changes

## Observations

- Found unused dependency in `crates/aletheia-lib`
- The `src/steward/mod.rs` file needs refactoring
- CI flake in integration tests

## Test plan
- Run tests";

        let result = parse_observations(body);
        assert_eq!(result.len(), 3);
        assert!(result[0].text.contains("unused dependency"));
        assert!(result[1].text.contains("refactoring"));
        assert!(result[2].text.contains("CI flake"));
    }

    #[test]
    fn parse_asterisk_bullets() {
        let body = "\
## Observations

* First observation
* Second observation";

        let result = parse_observations(body);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "First observation");
        assert_eq!(result[1].text, "Second observation");
    }

    #[test]
    fn parse_multiline_bullets() {
        let body = "\
## Observations

- This is a long observation that
  continues on the next line
- Short observation";

        let result = parse_observations(body);
        assert_eq!(result.len(), 2);
        assert!(result[0].text.contains("long observation"));
        assert!(result[0].text.contains("continues"));
    }

    #[test]
    fn parse_observations_with_code_block() {
        let body = "\
## Observations

- Found issue with code:
  ```
  - this is not a bullet
  ```
- Another observation";

        let result = parse_observations(body);
        assert_eq!(result.len(), 2);
        assert!(result[0].text.contains("code:"));
        assert_eq!(result[1].text, "Another observation");
    }

    #[test]
    fn extract_crate_tags() {
        let tags = extract_tags("Found issue in `crates/aletheia-lib` and `aletheia-cli`");
        assert!(tags.contains(&"aletheia-lib".to_owned()));
        assert!(tags.contains(&"aletheia-cli".to_owned()));
    }

    #[test]
    fn extract_path_tags() {
        let tags = extract_tags("The file `src/steward/mod.rs` needs work");
        assert!(tags.contains(&"src/steward/mod.rs".to_owned()));
    }

    #[test]
    fn parse_observations_at_end_of_body() {
        let body = "\
## Observations

- Final observation with no trailing section";

        let result = parse_observations(body);
        assert_eq!(result.len(), 1);
        assert!(result[0].text.contains("Final observation"));
    }

    #[test]
    fn empty_observations_section() {
        let body = "\
## Observations

## Test plan
- Run tests";

        let result = parse_observations(body);
        assert!(result.is_empty());
    }

    #[test]
    fn deduplicates_tags() {
        let tags = extract_tags("`aletheia-lib` and also crates/aletheia-lib/src");
        let count = tags.iter().filter(|t| *t == "aletheia-lib").count();
        assert_eq!(count, 1);
    }

    // -------------------------------------------------------------------
    // ObservationType classification tests
    // -------------------------------------------------------------------

    #[test]
    fn classify_bug_keywords() {
        assert_eq!(
            ObservationType::classify("null pointer crash in dispatch loop"),
            ObservationType::Bug
        );
        assert_eq!(
            ObservationType::classify("This function panics on empty input"),
            ObservationType::Bug
        );
        assert_eq!(
            ObservationType::classify("error handling is wrong in parser"),
            ObservationType::Bug
        );
        assert_eq!(
            ObservationType::classify("test fails intermittently"),
            ObservationType::Bug
        );
        assert_eq!(
            ObservationType::classify("race condition in concurrent access"),
            ObservationType::Bug
        );
    }

    #[test]
    fn classify_missing_test() {
        assert_eq!(
            ObservationType::classify("no tests for the merge conflict resolver"),
            ObservationType::MissingTest
        );
        assert_eq!(
            ObservationType::classify("missing test coverage for edge cases"),
            ObservationType::MissingTest
        );
    }

    #[test]
    fn classify_doc_gap() {
        assert_eq!(
            ObservationType::classify("undocumented public API in dispatch module"),
            ObservationType::DocGap
        );
        assert_eq!(
            ObservationType::classify("no docs for the configuration system"),
            ObservationType::DocGap
        );
    }

    #[test]
    fn classify_debt() {
        assert_eq!(
            ObservationType::classify("should refactor the query builder"),
            ObservationType::Debt
        );
        assert_eq!(
            ObservationType::classify("cleanup needed in legacy migration code"),
            ObservationType::Debt
        );
        assert_eq!(
            ObservationType::classify("dead code in the old parser"),
            ObservationType::Debt
        );
    }

    #[test]
    fn classify_idea_default() {
        assert_eq!(
            ObservationType::classify("could batch observations by project"),
            ObservationType::Idea
        );
        assert_eq!(
            ObservationType::classify("this API shape would be nicer as a builder"),
            ObservationType::Idea
        );
    }

    #[test]
    fn classify_bug_takes_priority() {
        // WHY: "error" is a bug keyword and should win over "refactor" (debt).
        assert_eq!(
            ObservationType::classify("refactor the error handling — it crashes"),
            ObservationType::Bug
        );
    }

    #[test]
    fn observation_type_roundtrip() {
        for ty in [
            ObservationType::Bug,
            ObservationType::Debt,
            ObservationType::Idea,
            ObservationType::MissingTest,
            ObservationType::DocGap,
        ] {
            assert_eq!(ObservationType::from_str_lossy(ty.as_str()), ty);
        }
    }

    #[test]
    fn observation_type_display() {
        assert_eq!(ObservationType::Bug.to_string(), "bug");
        assert_eq!(ObservationType::MissingTest.to_string(), "missing_test");
    }

    #[test]
    fn observation_type_unknown_defaults_to_idea() {
        assert_eq!(
            ObservationType::from_str_lossy("unknown"),
            ObservationType::Idea
        );
    }

    #[test]
    fn parse_observations_classifies_types() {
        let body = "\
## Observations

- Found a crash in the dispatch loop
- The merge module needs refactoring
- Consider adding a caching layer";

        let result = parse_observations(body);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].observation_type, ObservationType::Bug);
        assert_eq!(result[1].observation_type, ObservationType::Debt);
        assert_eq!(result[2].observation_type, ObservationType::Idea);
    }
}
