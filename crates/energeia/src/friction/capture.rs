//! Friction capture: parse structured observations from PR bodies.
//!
//! Worker agents leave observations in a dedicated PR-body section;
//! this module extracts them so ephemeral knowledge can be converted
//! into institutional memory.

/// A single out-of-scope observation captured in a PR body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// Human-readable title summarising the observation.
    pub title: String,
    /// Detailed description of the observation.
    pub body: String,
    /// URL of the PR where the observation was recorded.
    pub source: String,
    /// Files or crates affected by the observation.
    pub files_affected: Vec<String>,
}

/// Template snippet for the "Observations" PR body section.
///
/// Prompt builders can inject this template so worker agents know
/// how to format observations. Replace the `{{placeholder}}` tokens
/// at render time.
pub const TEMPLATE: &str = r"## Observations

### {{title}}
{{body}}

- **Source:** {{source}}
- **Files affected:** {{files}}

";

/// Parse a PR body and extract all observations from the
/// `## Observations` section.
///
/// Returns an empty vector when:
/// - the PR body contains no `## Observations` heading;
/// - the section is present but contains no valid observation blocks.
///
/// Malformed observation blocks are skipped gracefully.
#[must_use]
pub fn parse_pr_body(text: &str) -> Vec<Observation> {
    // Normalize Windows line endings so the rest of the parser
    // only has to deal with '\n'.
    let normalized = text.replace('\r', "");
    let Some(section) = extract_observations_section(&normalized) else {
        return Vec::new();
    };

    split_observation_blocks(&section)
        .into_iter()
        .filter_map(|block| parse_observation_block(&block))
        .collect()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Locate the `## Observations` section and return its body
/// (everything after the heading up to the next `## ` heading or EOF).
fn extract_observations_section(text: &str) -> Option<String> {
    let mut in_section = false;
    let mut section_lines = Vec::new();

    for line in text.lines() {
        if line.trim() == "## Observations" {
            in_section = true;
            continue;
        }
        if in_section && line.starts_with("## ") {
            break;
        }
        if in_section {
            section_lines.push(line);
        }
    }

    if in_section {
        Some(section_lines.join("\n"))
    } else {
        None
    }
}

/// Split a section into raw observation blocks.
///
/// Each block starts with a `### ` line.
fn split_observation_blocks(section: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut in_block = false;

    for line in section.lines() {
        if line.starts_with("### ") {
            if in_block && !current.is_empty() {
                blocks.push(current.join("\n"));
            }
            current.clear();
            in_block = true;
        }
        if in_block {
            current.push(line);
        }
    }

    if in_block && !current.is_empty() {
        blocks.push(current.join("\n"));
    }

    blocks
}

/// Parse a single raw observation block into an [`Observation`].
///
/// Returns `None` when the block has no title or is otherwise
/// too malformed to interpret.
fn parse_observation_block(block: &str) -> Option<Observation> {
    let mut lines = block.lines();
    let first_line = lines.next()?;
    let title = first_line.strip_prefix("### ")?.trim().to_owned();
    if title.is_empty() {
        return None;
    }

    let mut body_lines = Vec::new();
    let mut source = String::new();
    let mut files = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("- **Source:**") {
            value.trim().clone_into(&mut source);
        } else if let Some(value) = trimmed.strip_prefix("- Source:") {
            value.trim().clone_into(&mut source);
        } else if let Some(value) = trimmed.strip_prefix("- **Files affected:**") {
            files = parse_files(value);
        } else if let Some(value) = trimmed.strip_prefix("- Files affected:") {
            files = parse_files(value);
        } else {
            body_lines.push(line);
        }
    }

    // Trim trailing blank lines from body.
    while body_lines.last().is_some_and(|l| l.trim().is_empty()) {
        body_lines.pop();
    }
    // Trim leading blank lines from body.
    while body_lines.first().is_some_and(|l| l.trim().is_empty()) {
        body_lines.remove(0);
    }

    let body = body_lines.join("\n").trim().to_owned();

    Some(Observation {
        title,
        body,
        source,
        files_affected: files,
    })
}

/// Parse a comma-separated list of file names, stripping backtick wrappers.
fn parse_files(text: &str) -> Vec<String> {
    text.split(',')
        .map(|s| {
            let trimmed = s.trim();
            trimmed.trim_matches('`').trim().to_owned()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions over fixture data")]
mod tests {
    use super::*;

    #[test]
    fn empty_body_returns_empty() {
        assert!(parse_pr_body("").is_empty());
    }

    #[test]
    fn body_without_observations_section() {
        let text = "## Description\n\nSome PR description.\n\n## Checklist\n- [x] Tests pass";
        assert!(parse_pr_body(text).is_empty());
    }

    #[test]
    fn single_observation() {
        let text = r"## Description

Fixed the bug.

## Observations

### Missing test coverage for edge case
The fuzzer found a crash when input is exactly 64 KiB.

- **Source:** https://github.com/forkwright/aletheia/pull/1234
- **Files affected:** `crates/energeia/src/engine.rs`

## Checklist
- [x] Tests added
";
        let obs = parse_pr_body(text);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].title, "Missing test coverage for edge case");
        assert_eq!(
            obs[0].body,
            "The fuzzer found a crash when input is exactly 64 KiB."
        );
        assert_eq!(
            obs[0].source,
            "https://github.com/forkwright/aletheia/pull/1234"
        );
        assert_eq!(obs[0].files_affected, vec!["crates/energeia/src/engine.rs"]);
    }

    #[test]
    fn multiple_observations() {
        let text = r"## Observations

### First observation
Body one.

- **Source:** https://example.com/1
- **Files affected:** `a.rs`, `b.rs`

### Second observation
Body two.

- **Source:** https://example.com/2
- **Files affected:** `c.rs`

";
        let obs = parse_pr_body(text);
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].title, "First observation");
        assert_eq!(obs[0].body, "Body one.");
        assert_eq!(obs[0].source, "https://example.com/1");
        assert_eq!(obs[0].files_affected, vec!["a.rs", "b.rs"]);
        assert_eq!(obs[1].title, "Second observation");
        assert_eq!(obs[1].body, "Body two.");
        assert_eq!(obs[1].source, "https://example.com/2");
        assert_eq!(obs[1].files_affected, vec!["c.rs"]);
    }

    #[test]
    fn malformed_fragments_graceful_degrade() {
        let text = r"## Observations

### No source line
This block has no source bullet.

###
Empty title, should be skipped.

### Valid observation
This one is good.

- **Source:** https://example.com/valid
- **Files affected:** `foo.rs`

Some trailing text without heading.
";
        let obs = parse_pr_body(text);
        assert_eq!(obs.len(), 2);
        // No source line observation: title kept, source empty, body kept.
        assert_eq!(obs[0].title, "No source line");
        assert_eq!(obs[0].source, "");
        assert!(obs[0].body.contains("no source bullet"));
        // Valid observation.
        assert_eq!(obs[1].title, "Valid observation");
        assert_eq!(obs[1].source, "https://example.com/valid");
        assert_eq!(obs[1].files_affected, vec!["foo.rs"]);
    }

    #[test]
    fn observations_section_with_no_blocks() {
        let text =
            "## Observations\n\nJust some free-form text without observation blocks.\n\n## Footer";
        let obs = parse_pr_body(text);
        assert!(obs.is_empty());
    }

    #[test]
    fn template_is_non_empty() {
        assert!(!TEMPLATE.is_empty());
        assert!(TEMPLATE.contains("## Observations"));
        assert!(TEMPLATE.contains("{{title}}"));
        assert!(TEMPLATE.contains("{{body}}"));
        assert!(TEMPLATE.contains("{{source}}"));
        assert!(TEMPLATE.contains("{{files}}"));
    }

    #[test]
    fn source_without_bold_markup() {
        let text = r"## Observations

### Plain style
Body.

- Source: https://example.com/plain
- Files affected: foo.rs, bar.rs
";
        let obs = parse_pr_body(text);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].source, "https://example.com/plain");
        assert_eq!(obs[0].files_affected, vec!["foo.rs", "bar.rs"]);
    }

    #[test]
    fn windows_line_endings() {
        let text = "## Observations\r\n\r\n### Title\r\nBody.\r\n\r\n- **Source:** https://example.com\r\n- **Files affected:** `a.rs`\r\n";
        let obs = parse_pr_body(text);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].title, "Title");
        assert_eq!(obs[0].body, "Body.");
        assert_eq!(obs[0].source, "https://example.com");
        assert_eq!(obs[0].files_affected, vec!["a.rs"]);
    }
}
