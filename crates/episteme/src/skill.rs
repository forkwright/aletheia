//! Skill storage helpers, SKILL.md parser, and CC-format exporter.
//!
//! Skills are facts with `fact_type = "skill"`. This module provides:
//! - Structured content type for skill JSON
//! - Parser for SKILL.md markdown files
//! - Exporter to Claude Code `.claude/skills/<slug>/SKILL.md` format
//! - Query helpers on `KnowledgeStore`

use serde::{Deserialize, Serialize};

/// Decay score thresholds for skill lifecycle management.
///
/// These are compile-time defaults. Callers should prefer the values from
/// `taxis::config::KnowledgeConfig::skill_decay_*` fields when available.
pub mod decay {
    /// Skills below this score are flagged for review.
    ///
    /// Callers should prefer `taxis::config::KnowledgeConfig::skill_decay_needs_review_threshold`.
    pub const NEEDS_REVIEW_THRESHOLD: f64 = 0.3;
    /// Skills below this score are auto-retired.
    ///
    /// Callers should prefer `taxis::config::KnowledgeConfig::skill_decay_retire_threshold`.
    pub const RETIRE_THRESHOLD: f64 = 0.08;
    /// Default days of inactivity before decay reaches review threshold for low-usage skills.
    ///
    /// Callers should prefer `taxis::config::KnowledgeConfig::skill_decay_stale_days`.
    pub const DEFAULT_STALE_DAYS: u32 = 28;
    /// Usage count above which a skill is considered "high-usage" and decays 3× slower.
    ///
    /// Callers should prefer `taxis::config::KnowledgeConfig::skill_decay_high_usage_threshold`.
    pub const HIGH_USAGE_THRESHOLD: u32 = 10;
    /// Multiplier applied to decay half-life for high-usage skills.
    ///
    /// Callers should prefer `taxis::config::KnowledgeConfig::skill_decay_high_usage_factor`.
    pub const HIGH_USAGE_DECAY_FACTOR: f64 = 3.0;
}

/// Compute a decay score for a skill fact.
///
/// Score range: 0.0 (stale) to 1.0 (fully active).
///
/// Formula: `score = recency × usage_boost × confidence`
/// - **recency**: exponential decay with configurable half-life
/// - **`usage_boost`**: high-usage skills (>10 uses) decay 3× slower
/// - **confidence**: fact confidence (0.0--1.0) acts as a ceiling
///
/// The half-life for low-usage skills is `stale_days` (default 28). For
/// high-usage skills (>10 uses), it's `stale_days × 3`.
#[cfg(any(feature = "mneme-engine", test))]
#[must_use]
pub(crate) fn skill_decay_score(
    days_since_last_use: f64,
    usage_count: u32,
    confidence: f64,
) -> f64 {
    let half_life = if usage_count > decay::HIGH_USAGE_THRESHOLD {
        f64::from(decay::DEFAULT_STALE_DAYS) * decay::HIGH_USAGE_DECAY_FACTOR
    } else {
        f64::from(decay::DEFAULT_STALE_DAYS)
    };

    let recency = 2_f64.powf(-days_since_last_use / half_life);
    let usage_floor = f64::from(usage_count.min(20)) / 100.0;
    let raw = recency + usage_floor;
    (raw * confidence.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// Skill health metrics for the quality dashboard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillHealthMetrics {
    /// Total active (non-forgotten) skills.
    pub total_active: usize,
    /// Total retired (forgotten with reason "stale") skills.
    pub total_retired: usize,
    /// Total skills flagged as needing review.
    pub total_needs_review: usize,
    /// Average usage count across active skills.
    pub avg_usage_count: f64,
    /// Median days since last use across active skills.
    pub median_days_since_use: f64,
    /// Top skills by usage count (name, `usage_count`).
    pub top_skills: Vec<(String, u32)>,
    /// Bottom skills by usage count (name, `usage_count`).
    pub bottom_skills: Vec<(String, u32)>,
    /// Dedup rate: candidates discarded / total candidates processed.
    pub dedup_discard_count: u64,
    /// Total candidates processed through the dedup pipeline.
    pub dedup_total_count: u64,
}

/// Structured content stored as JSON in a skill fact's `content` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillContent {
    /// Short identifier (slug), e.g. `"rust-error-handling"`.
    pub name: String,
    /// Human-readable description of what this skill does.
    pub description: String,
    /// Ordered steps to execute the skill.
    pub steps: Vec<String>,
    /// Tools referenced by the skill.
    pub tools_used: Vec<String>,
    /// Domain classification tags (e.g. `["rust", "error-handling"]`).
    pub domain_tags: Vec<String>,
    /// How this skill was created: `"manual"`, `"seeded"`, or `"extracted"`.
    pub origin: String,
    /// Trigger keywords that hint this skill should be loaded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<String>,
    /// Whether this skill is always injected into the system prompt.
    /// When `false` (default), the skill is lazy-loaded via `skill_read`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub always: bool,
}

/// Errors from SKILL.md parsing.
#[derive(Debug, Clone)]
pub struct SkillParseError {
    /// Path to the SKILL.md file that failed to parse.
    pub path: String,
    /// Human-readable description of the parse failure.
    pub reason: String,
}

impl std::fmt::Display for SkillParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse {}: {}", self.path, self.reason)
    }
}

/// Parse a SKILL.md file into structured skill content.
///
/// Accepts two title shapes (Claude-Code-compatible):
/// 1. YAML frontmatter with `name:` (body may omit `# Title`).
/// 2. Body `# Title` heading (frontmatter optional).
///
/// Description is taken from frontmatter `description:` when present, else
/// from the body text after the H1, else from a `## When to Use` section.
///
/// # Errors
///
/// Returns an error naming the missing route(s) when no title can be found
/// on either path, or when no description can be derived.
pub fn parse_skill_md(source: &str, slug: &str) -> Result<SkillContent, SkillParseError> {
    let err = |reason: &str| SkillParseError {
        path: slug.to_owned(),
        reason: reason.to_owned(),
    };

    let (frontmatter, body) = split_frontmatter(source);
    let fm = parse_frontmatter(frontmatter);

    let mut lines = body.lines().peekable();
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }

    let has_h1 = lines.peek().is_some_and(|l| l.starts_with("# "));
    if has_h1 {
        lines.next();
    } else if fm.name.is_none() {
        // WHY: title can come from frontmatter `name:` (Claude-Code shape) or
        // a body `# Title` (legacy aletheia shape). Naming both routes in the
        // error keeps the failure mode legible — see #4234.
        return Err(err(
            "missing title: no `name:` in YAML frontmatter and no `# Title` heading in body",
        ));
    }

    let mut desc_lines = Vec::new();
    while let Some(&line) = lines.peek() {
        if line.starts_with("## ") {
            break;
        }
        lines.next();
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            desc_lines.push(trimmed.to_owned());
        }
    }
    let mut description = fm.description.unwrap_or_else(|| desc_lines.join(" "));

    let mut current_section = String::new();
    let mut sections: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for line in lines {
        if let Some(heading) = line.strip_prefix("## ") {
            current_section = heading.trim().to_lowercase();
            sections.entry(current_section.clone()).or_default();
        } else if !current_section.is_empty() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                sections
                    .entry(current_section.clone())
                    .or_default()
                    .push(trimmed.to_owned());
            }
        }
    }

    // kanon:ignore RUST/indexing-slicing — `&[][..]` is an empty-slice literal default for map_or; not real indexing
    let steps = extract_steps(sections.get("steps").map_or(&[][..], |v| v.as_slice()));
    let tools_used = if fm.tools.is_empty() {
        // kanon:ignore RUST/indexing-slicing — `&[][..]` is an empty-slice literal default for map_or; not real indexing
        extract_tools(sections.get("tools used").map_or(&[][..], |v| v.as_slice()))
    } else {
        fm.tools
    };

    let domain_tags = if fm.domains.is_empty() {
        derive_domain_tags(slug)
    } else {
        fm.domains
    };

    if description.is_empty()
        && let Some(when_lines) = sections.get("when to use")
    {
        description = when_lines.join(" ");
    }

    if description.is_empty() {
        // WHY: name the routes that were tried so a hand-authored SKILL.md
        // can be fixed without diffing against the parser source — see #4234.
        return Err(err(
            "missing description: no `description:` in YAML frontmatter, no body text after the title, and no `## When to Use` section",
        ));
    }

    Ok(SkillContent {
        name: slug.to_owned(),
        description,
        steps,
        tools_used,
        domain_tags,
        origin: "seeded".to_owned(),
        triggers: fm.triggers,
        always: fm.always,
    })
}

/// YAML frontmatter fields extracted from a SKILL.md document.
#[derive(Debug, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    tools: Vec<String>,
    domains: Vec<String>,
    triggers: Vec<String>,
    always: bool,
}

/// Extract SKILL.md frontmatter fields from the raw YAML block (if any).
fn parse_frontmatter(raw: Option<&str>) -> SkillFrontmatter {
    let mut fm = SkillFrontmatter::default();
    let Some(raw) = raw else {
        return fm;
    };
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            let value = parse_yaml_scalar(rest);
            if !value.is_empty() {
                fm.name = Some(value);
            }
        } else if let Some(rest) = line.strip_prefix("description:") {
            let value = parse_yaml_scalar(rest);
            if !value.is_empty() {
                fm.description = Some(value);
            }
        } else if let Some(rest) = line.strip_prefix("tools:") {
            fm.tools = parse_yaml_array(rest);
        } else if let Some(rest) = line.strip_prefix("domains:") {
            fm.domains = parse_yaml_array(rest);
        } else if let Some(rest) = line.strip_prefix("triggers:") {
            fm.triggers = parse_yaml_array(rest);
        } else if let Some(rest) = line.strip_prefix("always:") {
            fm.always = rest.trim().eq_ignore_ascii_case("true");
        }
    }
    fm
}

/// Split optional YAML frontmatter from body.
fn split_frontmatter(source: &str) -> (Option<&str>, &str) {
    let trimmed = source.trim_start();
    if !trimmed.starts_with("---") {
        return (None, source);
    }
    let after_open = trimmed.get(3..).unwrap_or("");
    if let Some(close_pos) = after_open.find("\n---") {
        let fm = after_open.get(..close_pos).unwrap_or("");
        let body = after_open.get(close_pos + 4..).unwrap_or("");
        (Some(fm), body)
    } else {
        (None, source)
    }
}

/// Parse a YAML scalar value (the bit after `key:`).
///
/// Strips surrounding whitespace and a single layer of matching single or
/// double quotes. Returns the empty string for values that are missing,
/// whitespace-only, or empty after quote stripping.
fn parse_yaml_scalar(s: &str) -> String {
    let s = s.trim();
    let stripped = s
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            s.strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
        .unwrap_or(s);
    stripped.to_owned()
}

/// Parse a simple YAML inline array like `[web_fetch, web_search]`.
fn parse_yaml_array(s: &str) -> Vec<String> {
    let s = s.trim();
    let s = s.strip_prefix('[').unwrap_or(s);
    let s = s.strip_suffix(']').unwrap_or(s);
    s.split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Extract ordered steps from lines like `1. Do something`.
fn extract_steps(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let stripped = if let Some(pos) = line.find(". ") {
                let prefix = line.get(..pos).unwrap_or("");
                if prefix.chars().all(|c| c.is_ascii_digit()) {
                    line.get(pos + 2..).unwrap_or("").trim().to_owned()
                } else {
                    line.clone()
                }
            } else if let Some(rest) = line.strip_prefix("- ") {
                rest.trim().to_owned()
            } else {
                return None;
            };
            if stripped.is_empty() {
                None
            } else {
                Some(stripped)
            }
        })
        .collect()
}

/// Extract tool names from lines like `- ToolName: description`.
fn extract_tools(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let line = line.strip_prefix("- ").unwrap_or(line);
            let name = if let Some(colon_pos) = line.find(':') {
                line.get(..colon_pos).unwrap_or("").trim()
            } else {
                line.trim()
            };
            if name.is_empty() {
                None
            } else {
                Some(name.to_owned())
            }
        })
        .collect()
}

/// Derive domain tags from a slug like `rust-error-handling` → `["rust", "error-handling"]`.
fn derive_domain_tags(slug: &str) -> Vec<String> {
    slug.split('-')
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Scan a directory for subdirectories containing SKILL.md files.
///
/// Returns `(slug, content_string)` pairs for each found skill.
///
/// # Errors
///
/// Returns an error if the directory cannot be read or if a skill file
/// cannot be read.
pub fn scan_skill_dir(dir: &std::path::Path) -> Result<Vec<(String, String)>, std::io::Error> {
    let mut skills = Vec::new();

    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                let slug = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_owned();
                let content = std::fs::read_to_string(&skill_md)?;
                skills.push((slug, content));
            }
        }
    }

    skills.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(skills)
}

/// Convert a skill name into a filesystem-safe slug.
///
/// Lowercases, replaces whitespace/non-alphanumeric runs with `-`, and trims
/// leading/trailing dashes.
#[must_use]
pub(crate) fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    let mut result = String::with_capacity(slug.len());
    let mut prev_dash = true;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    if result.ends_with('-') {
        result.pop();
    }
    result
}

/// Format a [`SkillContent`] as a CC-native SKILL.md with YAML frontmatter.
///
/// The output matches Claude Code's expected format:
/// ```text
/// ---
/// name: <slug>
/// description: <description>
/// allowed-tools: <tool1>, <tool2>
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
#[must_use]
pub fn format_skill_md(skill: &SkillContent) -> String {
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
        // WHY: Write both keys: CC reads 'allowed-tools'; parse_skill_md reads 'tools'.
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

    // WHY: Title heading is required for parse_skill_md round-trip.
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

/// Export result for a single skill.
#[derive(Debug)]
pub struct ExportedSkill {
    /// Path to the written SKILL.md file.
    pub path: std::path::PathBuf,
    /// The slug used for the directory name.
    pub slug: String,
    /// The skill name (from content).
    pub name: String,
}

/// Export a collection of skills to Claude Code's `.claude/skills/<slug>/SKILL.md` format.
///
/// Creates the directory structure and writes each skill as a SKILL.md file
/// with YAML frontmatter. Existing files are overwritten.
///
/// This is a pure library function: no knowledge store dependency. Pass in
/// already-resolved `SkillContent` values. The CLI and energeia bridge both
/// use this same function.
///
/// # Errors
///
/// Returns `std::io::Error` if directory creation or file writing fails.
pub fn export_skills_to_cc(
    skills: &[SkillContent],
    output_dir: &std::path::Path,
    domain_filter: Option<&[&str]>,
) -> Result<Vec<ExportedSkill>, std::io::Error> {
    let mut exported = Vec::new();

    for skill in skills {
        if let Some(filter) = domain_filter {
            let matches = filter
                .iter()
                .any(|tag| skill.domain_tags.iter().any(|dt| dt == tag));
            if !matches {
                continue;
            }
        }

        let slug = slugify(&skill.name);
        if slug.is_empty() {
            continue;
        }
        let skill_dir = output_dir.join(&slug);
        std::fs::create_dir_all(&skill_dir)?;

        let md = format_skill_md(skill);
        let path = skill_dir.join("SKILL.md");
        #[expect(
            clippy::disallowed_methods,
            reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
        )]
        std::fs::write(&path, &md)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        exported.push(ExportedSkill {
            path,
            slug,
            name: skill.name.clone(),
        });
    }

    Ok(exported)
}

#[cfg(test)]
#[path = "skill_tests.rs"]
mod tests;
