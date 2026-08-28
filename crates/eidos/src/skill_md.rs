//! Canonical SKILL.md wire shape and renderer.
//!
//! [`SkillMd`] and [`format_skill_md`] are the single owner of the SKILL.md
//! frontmatter/body projection. Every consumer that materializes a stored
//! skill as SKILL.md text -- however it obtained the stored JSON -- renders
//! through this function so the same stored skill always produces the same
//! bytes, regardless of which path (eager export, lazy `skill_read`, ...)
//! did the materializing.

use serde::{Deserialize, Serialize};

/// Fields needed to render a stored skill as SKILL.md text.
///
/// This is a projection of the full persisted skill record (e.g.
/// `episteme::skill::SkillContent`), carrying only what the renderer
/// consumes. Deserializing directly from stored skill JSON silently ignores
/// fields this type does not declare (e.g. `origin`), so a lightweight
/// consumer can read the same JSON without depending on the crate that owns
/// the full record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMd {
    /// Short identifier (slug), e.g. `"rust-error-handling"`.
    pub name: String,
    /// Human-readable description of what this skill does.
    pub description: String,
    /// Ordered steps to execute the skill.
    #[serde(default)]
    pub steps: Vec<String>,
    /// Tools referenced by the skill.
    #[serde(default)]
    pub tools_used: Vec<String>,
    /// Domain classification tags (e.g. `["rust", "error-handling"]`).
    #[serde(default)]
    pub domain_tags: Vec<String>,
    /// Trigger keywords that hint this skill should be loaded.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// Whether this skill is always injected into the system prompt.
    #[serde(default)]
    pub always: bool,
}

/// Format a [`SkillMd`] as a SKILL.md with YAML frontmatter.
///
/// The output matches Claude Code's expected format:
/// ```text
/// ---
/// name: <slug>
/// description: <description>
/// allowed-tools: <tool1>, <tool2>
/// tools: [<tool1>, <tool2>]
/// ---
///
/// ## When to Use
/// <description>
///
/// ## Steps
/// 1. <step>
///
/// ## Tools Used
/// - <tool>
/// ```
///
/// WHY: writes both `allowed-tools` and `tools` -- Claude Code reads
/// `allowed-tools`, while `parse_skill_md`/Aletheia's own frontmatter reader
/// reads `tools`. Both consumers need the same rendered bytes to work.
#[must_use]
pub fn format_skill_md(skill: &SkillMd) -> String {
    use std::fmt::Write as _;
    let mut md = String::with_capacity(512);

    md.push_str("---\n");
    // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
    let _ = writeln!(md, "name: {}", skill.name);
    let desc_needs_quoting = skill.description.contains(':')
        || skill.description.contains('#')
        || skill.description.contains('"');
    if desc_needs_quoting {
        let escaped = skill.description.replace('"', r#"\""#);
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "description: \"{escaped}\"");
    } else {
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "description: {}", skill.description);
    }
    if !skill.tools_used.is_empty() {
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "allowed-tools: {}", skill.tools_used.join(", "));
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "tools: [{}]", skill.tools_used.join(", "));
    }
    if !skill.domain_tags.is_empty() {
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "domains: [{}]", skill.domain_tags.join(", "));
    }
    if !skill.triggers.is_empty() {
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "triggers: [{}]", skill.triggers.join(", "));
    }
    if skill.always {
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "always: true");
    }
    md.push_str("---\n\n");

    // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
    let _ = writeln!(md, "# {}\n", skill.name);

    md.push_str("## When to Use\n");
    // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
    let _ = writeln!(md, "{}\n", skill.description);

    if !skill.steps.is_empty() {
        md.push_str("## Steps\n");
        for (i, step) in skill.steps.iter().enumerate() {
            // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
            let _ = writeln!(md, "{}. {}", i + 1, step);
        }
        md.push('\n');
    }

    if !skill.tools_used.is_empty() {
        md.push_str("## Tools Used\n");
        for tool in &skill.tools_used {
            // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
            let _ = writeln!(md, "- {tool}");
        }
        md.push('\n');
    }

    if !skill.domain_tags.is_empty() {
        md.push_str("## Tags\n");
        // kanon:ignore RUST/no-silent-result-swallow — String::write is infallible
        let _ = writeln!(md, "{}", skill.domain_tags.join(", "));
    }

    md
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn sample() -> SkillMd {
        SkillMd {
            name: "rust-error-handling".to_owned(),
            description: "Pattern for converting error types".to_owned(),
            steps: vec!["Identify the source error type".to_owned()],
            tools_used: vec!["Read".to_owned(), "Edit".to_owned()],
            domain_tags: vec!["rust".to_owned()],
            triggers: vec![],
            always: false,
        }
    }

    /// WHY(#7026): the canonical projection must write both keys -- Claude
    /// Code reads `allowed-tools`, `parse_skill_md`/Aletheia's own
    /// frontmatter reader reads `tools`. Every consumer of this function
    /// (eager export via `episteme::skill::format_skill_md`, lazy load via
    /// `organon::builtins::skill_read`) shares this one implementation, so a
    /// regression here fails every consumer's tests at once instead of one
    /// silently drifting from the other.
    #[test]
    fn writes_both_allowed_tools_and_tools_keys() {
        let md = format_skill_md(&sample());
        assert!(
            md.contains("allowed-tools: Read, Edit"),
            "missing allowed-tools key: {md}"
        );
        assert!(
            md.contains("tools: [Read, Edit]"),
            "missing tools key: {md}"
        );
    }

    #[test]
    fn no_tools_omits_both_keys() {
        let mut skill = sample();
        skill.tools_used.clear();
        let md = format_skill_md(&skill);
        assert!(!md.contains("allowed-tools:"));
        assert!(!md.contains("tools:"));
    }

    #[test]
    fn deserializes_ignoring_unknown_fields() {
        // WHY: stored skill JSON (episteme::skill::SkillContent) carries an
        // `origin` field this projection does not declare. A dependency-light
        // consumer must be able to read the same JSON without erroring.
        let json = serde_json::json!({
            "name": "n",
            "description": "d",
            "origin": "manual",
        })
        .to_string();
        let skill: SkillMd = serde_json::from_str(&json).expect("unknown fields are ignored");
        assert_eq!(skill.name, "n");
    }
}
