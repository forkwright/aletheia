//! Skill storage helpers, SKILL.md parser, and CC-format exporter.
//!
//! Skills are facts with `fact_type = "skill"`. This module provides:
//! - Structured content type for skill JSON
//! - Parser for SKILL.md markdown files
//! - Exporter to Claude Code `.claude/skills/<slug>/SKILL.md` format
//! - Query helpers on `KnowledgeStore`

use serde::{Deserialize, Serialize};

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
}

/// Errors from SKILL.md parsing.
#[derive(Debug, Clone)]
pub struct SkillParseError {
    pub path: String,
    pub reason: String,
}

impl std::fmt::Display for SkillParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse {}: {}", self.path, self.reason)
    }
}

/// Parse a SKILL.md file into structured skill content.
///
/// Supports optional YAML frontmatter (delimited by `---`) with `tools` and
/// `domains` fields. Falls back to extracting from markdown sections.
pub fn parse_skill_md(source: &str, slug: &str) -> Result<SkillContent, SkillParseError> {
    let err = |reason: &str| SkillParseError {
        path: slug.to_owned(),
        reason: reason.to_owned(),
    };

    let (frontmatter, body) = split_frontmatter(source);

    // Parse frontmatter if present
    let mut fm_tools: Vec<String> = Vec::new();
    let mut fm_domains: Vec<String> = Vec::new();
    if let Some(fm) = frontmatter {
        for line in fm.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("tools:") {
                fm_tools = parse_yaml_array(rest);
            } else if let Some(rest) = line.strip_prefix("domains:") {
                fm_domains = parse_yaml_array(rest);
            }
        }
    }

    // Extract title from first `# ` heading
    let mut lines = body.lines().peekable();

    // Skip blank lines before title
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }

    let title_line = lines.next().ok_or_else(|| err("empty document"))?;
    if !title_line.starts_with("# ") {
        return Err(err("missing top-level heading (# Title)"));
    }

    // Collect description: lines between title and first ## section
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
    let mut description = desc_lines.join(" ");

    // Parse sections
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

    // Extract steps from "Steps" or "steps" section
    let steps = extract_steps(sections.get("steps").map_or(&[][..], |v| v.as_slice()));

    // Extract tools from "Tools Used" section or frontmatter
    let tools_used = if fm_tools.is_empty() {
        extract_tools(sections.get("tools used").map_or(&[][..], |v| v.as_slice()))
    } else {
        fm_tools
    };

    // Domain tags from frontmatter, or derive from slug
    let domain_tags = if fm_domains.is_empty() {
        derive_domain_tags(slug)
    } else {
        fm_domains
    };

    // Use "When to Use" section as description if main description is empty
    if description.is_empty() {
        if let Some(when_lines) = sections.get("when to use") {
            description = when_lines.join(" ");
        }
    }

    if description.is_empty() {
        return Err(err("no description found"));
    }

    Ok(SkillContent {
        name: slug.to_owned(),
        description,
        steps,
        tools_used,
        domain_tags,
        origin: "seeded".to_owned(),
    })
}

/// Split optional YAML frontmatter from body.
fn split_frontmatter(source: &str) -> (Option<&str>, &str) {
    let trimmed = source.trim_start();
    if !trimmed.starts_with("---") {
        return (None, source);
    }
    // Find closing ---
    let after_open = &trimmed[3..];
    if let Some(close_pos) = after_open.find("\n---") {
        let fm = &after_open[..close_pos];
        let body = &after_open[close_pos + 4..];
        (Some(fm), body)
    } else {
        (None, source)
    }
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
            // Strip ordered list prefix (e.g. "1. ", "2. ")
            let stripped = if let Some(pos) = line.find(". ") {
                let prefix = &line[..pos];
                if prefix.chars().all(|c| c.is_ascii_digit()) {
                    line[pos + 2..].trim().to_owned()
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
            // Take everything before the colon as tool name
            let name = if let Some(colon_pos) = line.find(':') {
                line[..colon_pos].trim()
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

// ── CC-format exporter ──────────────────────────────────────────────────────

/// Convert a skill name into a filesystem-safe slug.
///
/// Lowercases, replaces whitespace/non-alphanumeric runs with `-`, and trims
/// leading/trailing dashes.
pub fn slugify(name: &str) -> String {
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

    // Collapse consecutive dashes and trim edges
    let mut result = String::with_capacity(slug.len());
    let mut prev_dash = true; // suppress leading dashes
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
    // Trim trailing dash
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
pub fn format_skill_md(skill: &SkillContent) -> String {
    use std::fmt::Write as _;
    let mut md = String::with_capacity(512);

    // YAML frontmatter
    md.push_str("---\n");
    let _ = writeln!(md, "name: {}", skill.name);
    // Escape description for YAML (wrap in quotes if it contains colons or special chars)
    let desc_needs_quoting = skill.description.contains(':')
        || skill.description.contains('#')
        || skill.description.contains('"');
    if desc_needs_quoting {
        let escaped = skill.description.replace('"', r#"\""#);
        let _ = writeln!(md, "description: \"{escaped}\"");
    } else {
        let _ = writeln!(md, "description: {}", skill.description);
    }
    if !skill.tools_used.is_empty() {
        // Write both CC-native and aletheia-native keys for interop:
        // `allowed-tools` is what CC reads; `tools` is what parse_skill_md reads.
        let _ = writeln!(md, "allowed-tools: {}", skill.tools_used.join(", "));
        let _ = writeln!(md, "tools: [{}]", skill.tools_used.join(", "));
    }
    if !skill.domain_tags.is_empty() {
        let _ = writeln!(md, "domains: [{}]", skill.domain_tags.join(", "));
    }
    md.push_str("---\n\n");

    // Title heading (required for parse_skill_md round-trip)
    let _ = writeln!(md, "# {}\n", skill.name);

    // When to Use
    md.push_str("## When to Use\n");
    let _ = writeln!(md, "{}\n", skill.description);

    // Steps
    if !skill.steps.is_empty() {
        md.push_str("## Steps\n");
        for (i, step) in skill.steps.iter().enumerate() {
            let _ = writeln!(md, "{}. {}", i + 1, step);
        }
        md.push('\n');
    }

    // Tools Used
    if !skill.tools_used.is_empty() {
        md.push_str("## Tools Used\n");
        for tool in &skill.tools_used {
            let _ = writeln!(md, "- {tool}");
        }
        md.push('\n');
    }

    // Domain Tags (informational, not CC-standard but useful)
    if !skill.domain_tags.is_empty() {
        md.push_str("## Tags\n");
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
/// This is a pure library function — no knowledge store dependency. Pass in
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
        // Apply domain filter if specified
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
        std::fs::write(&path, &md)?;

        exported.push(ExportedSkill {
            path,
            slug,
            name: skill.name.clone(),
        });
    }

    Ok(exported)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SKILL: &str = r"# Website and Review Intelligence Gathering
Systematically research a company by fetching official pages and aggregating third-party reviews.

## When to Use
When you need to comprehensively understand a company's offerings and reputation.

## Steps
1. Enable web_fetch tool
2. Fetch the company's homepage to identify main offerings
3. Search for independent reviews and discussions

## Tools Used
- web_fetch: to retrieve complete content from official company pages
- web_search: to locate independent reviews and discussions
";

    const SAMPLE_WITH_FRONTMATTER: &str = r"---
tools: [web_fetch, web_search]
domains: [research, writing]
---

# Website Intelligence
Research a company via web.

## When to Use
When you need company intelligence.

## Steps
1. Fetch homepage
2. Search reviews

## Tools Used
- web_fetch: fetch pages
- web_search: search web
";

    #[test]
    fn parse_basic_skill_md() {
        let skill = parse_skill_md(SAMPLE_SKILL, "web-research").expect("valid skill md");
        assert_eq!(skill.name, "web-research");
        assert!(skill.description.contains("Systematically research"));
        assert_eq!(skill.steps.len(), 3);
        assert_eq!(skill.steps[0], "Enable web_fetch tool");
        assert_eq!(skill.tools_used, vec!["web_fetch", "web_search"]);
        assert_eq!(skill.origin, "seeded");
    }

    #[test]
    fn parse_skill_with_frontmatter() {
        let skill = parse_skill_md(SAMPLE_WITH_FRONTMATTER, "web-intel").expect("valid frontmatter skill md");
        assert_eq!(skill.tools_used, vec!["web_fetch", "web_search"]);
        assert_eq!(skill.domain_tags, vec!["research", "writing"]);
        assert_eq!(skill.steps.len(), 2);
    }

    #[test]
    fn parse_skill_derives_domain_tags_from_slug() {
        let skill = parse_skill_md(SAMPLE_SKILL, "docker-network-diagnostics").expect("valid skill md for domain tag derivation");
        assert_eq!(skill.domain_tags, vec!["docker", "network", "diagnostics"]);
    }

    #[test]
    fn parse_skill_missing_heading_fails() {
        let bad = "No heading here\n\n## Steps\n1. Do stuff";
        let err = parse_skill_md(bad, "bad-skill").expect_err("bad skill md must fail");
        assert!(err.reason.contains("missing top-level heading"));
    }

    #[test]
    fn parse_skill_empty_doc_fails() {
        let err = parse_skill_md("", "empty").expect_err("empty skill md must fail");
        assert!(err.reason.contains("empty document"));
    }

    #[test]
    fn parse_skill_no_description_uses_when_to_use() {
        let md = "# Skill\n\n## When to Use\nWhen you need to do things.\n\n## Steps\n1. Do it\n";
        let skill = parse_skill_md(md, "fallback").expect("skill with when-to-use fallback description should parse");
        assert!(skill.description.contains("When you need to do things"));
    }

    #[test]
    fn parse_skill_no_description_at_all_fails() {
        let md = "# Skill\n\n## Steps\n1. Do it\n";
        let err = parse_skill_md(md, "no-desc").expect_err("skill without any description must fail to parse");
        assert!(err.reason.contains("no description"));
    }

    #[test]
    fn skill_content_serde_roundtrip() {
        let skill = SkillContent {
            name: "test-skill".to_owned(),
            description: "A test skill".to_owned(),
            steps: vec!["step 1".to_owned(), "step 2".to_owned()],
            tools_used: vec!["Read".to_owned(), "Edit".to_owned()],
            domain_tags: vec!["test".to_owned()],
            origin: "manual".to_owned(),
        };
        let json = serde_json::to_string(&skill).expect("SkillContent serializes to JSON");
        let back: SkillContent = serde_json::from_str(&json).expect("SkillContent deserializes from JSON");
        assert_eq!(skill, back);
    }

    #[test]
    fn parse_yaml_array_formats() {
        assert_eq!(parse_yaml_array("[a, b, c]"), vec!["a", "b", "c"]);
        assert_eq!(parse_yaml_array("[\"a\", 'b']"), vec!["a", "b"]);
        assert_eq!(parse_yaml_array("[]"), Vec::<String>::new());
    }

    #[test]
    fn split_frontmatter_present() {
        let (fm, body) = split_frontmatter("---\ntools: [a]\n---\n# Title\n");
        assert!(fm.is_some());
        assert!(fm.expect("frontmatter present").contains("tools:"));
        assert!(body.contains("# Title"));
    }

    #[test]
    fn split_frontmatter_absent() {
        let (fm, body) = split_frontmatter("# Title\nBody text");
        assert!(fm.is_none());
        assert!(body.contains("# Title"));
    }

    #[test]
    fn scan_skill_dir_with_tempdir() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir(&skill_dir).expect("create skill subdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "# My Skill\nDoes things.\n\n## When to Use\nAlways.\n\n## Steps\n1. Go\n",
        )
        .expect("write SKILL.md");

        let skills = scan_skill_dir(dir.path()).expect("scan skill dir");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].0, "my-skill");
    }

    #[test]
    fn scan_skill_dir_empty() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let skills = scan_skill_dir(dir.path()).expect("scan empty skill dir");
        assert!(skills.is_empty());
    }

    #[test]
    fn scan_skill_dir_ignores_non_skill_dirs() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let sub = dir.path().join("not-a-skill");
        std::fs::create_dir(&sub).expect("create non-skill subdir");
        std::fs::write(sub.join("README.md"), "not a skill").expect("write README.md");

        let skills = scan_skill_dir(dir.path()).expect("scan dir with non-skill subdirs");
        assert!(skills.is_empty());
    }

    #[test]
    fn extract_steps_mixed_format() {
        let lines = vec![
            "1. First step".to_owned(),
            "2. Second step".to_owned(),
            "- Third step".to_owned(),
        ];
        let steps = extract_steps(&lines);
        assert_eq!(steps, vec!["First step", "Second step", "Third step"]);
    }

    #[test]
    fn skill_parse_error_display() {
        let err = SkillParseError {
            path: "test-skill".to_owned(),
            reason: "missing heading".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "failed to parse test-skill: missing heading"
        );
    }

    // ── slugify ──────────────────────────────────────────────────────────────

    #[test]
    fn slugify_simple_name() {
        assert_eq!(slugify("rust-error-handling"), "rust-error-handling");
    }

    #[test]
    fn slugify_spaces_to_dashes() {
        assert_eq!(
            slugify("Docker Network Diagnostics"),
            "docker-network-diagnostics"
        );
    }

    #[test]
    fn slugify_special_chars() {
        assert_eq!(slugify("C++ Template (Meta)"), "c-template-meta");
    }

    #[test]
    fn slugify_consecutive_specials_collapsed() {
        assert_eq!(slugify("test---skill___name"), "test-skill-name");
    }

    #[test]
    fn slugify_empty_string() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_all_special() {
        assert_eq!(slugify("---"), "");
    }

    // ── format_skill_md ──────────────────────────────────────────────────────

    fn export_skill() -> SkillContent {
        SkillContent {
            name: "rust-error-handling".to_owned(),
            description: "Pattern for converting error types across crate boundaries".to_owned(),
            steps: vec![
                "Identify the source error type".to_owned(),
                "Create a snafu variant with #[snafu(source)]".to_owned(),
                "Add .context() at the call site".to_owned(),
            ],
            tools_used: vec!["Read".to_owned(), "Edit".to_owned(), "Bash".to_owned()],
            domain_tags: vec!["rust".to_owned(), "errors".to_owned()],
            origin: "manual".to_owned(),
        }
    }

    #[test]
    fn format_skill_md_has_yaml_frontmatter() {
        let md = format_skill_md(&export_skill());
        assert!(
            md.starts_with("---\n"),
            "should start with frontmatter delimiter"
        );
        // Count frontmatter delimiters
        let delimiters: Vec<_> = md.match_indices("---").collect();
        assert!(delimiters.len() >= 2, "should have opening and closing ---");
    }

    #[test]
    fn format_skill_md_frontmatter_has_name() {
        let md = format_skill_md(&export_skill());
        assert!(md.contains("name: rust-error-handling"));
    }

    #[test]
    fn format_skill_md_frontmatter_has_description() {
        let md = format_skill_md(&export_skill());
        assert!(md.contains("description: Pattern for converting error types"));
    }

    #[test]
    fn format_skill_md_frontmatter_has_allowed_tools() {
        let md = format_skill_md(&export_skill());
        assert!(md.contains("allowed-tools: Read, Edit, Bash"));
    }

    #[test]
    fn format_skill_md_has_when_to_use_section() {
        let md = format_skill_md(&export_skill());
        assert!(md.contains("## When to Use"));
    }

    #[test]
    fn format_skill_md_has_steps_section() {
        let md = format_skill_md(&export_skill());
        assert!(md.contains("## Steps"));
        assert!(md.contains("1. Identify the source error type"));
        assert!(md.contains("2. Create a snafu variant"));
        assert!(md.contains("3. Add .context() at the call site"));
    }

    #[test]
    fn format_skill_md_has_tools_section() {
        let md = format_skill_md(&export_skill());
        assert!(md.contains("## Tools Used"));
        assert!(md.contains("- Read"));
        assert!(md.contains("- Edit"));
        assert!(md.contains("- Bash"));
    }

    #[test]
    fn format_skill_md_has_tags_section() {
        let md = format_skill_md(&export_skill());
        assert!(md.contains("## Tags"));
        assert!(md.contains("rust, errors"));
    }

    #[test]
    fn format_skill_md_no_tools_omits_allowed_tools() {
        let mut skill = export_skill();
        skill.tools_used.clear();
        let md = format_skill_md(&skill);
        assert!(!md.contains("allowed-tools:"));
        assert!(!md.contains("## Tools Used"));
    }

    #[test]
    fn format_skill_md_no_steps_omits_steps_section() {
        let mut skill = export_skill();
        skill.steps.clear();
        let md = format_skill_md(&skill);
        assert!(!md.contains("## Steps"));
    }

    #[test]
    fn format_skill_md_description_with_colon_is_quoted() {
        let mut skill = export_skill();
        skill.description = "Error handling: a deep dive".to_owned();
        let md = format_skill_md(&skill);
        assert!(md.contains(r#"description: "Error handling: a deep dive""#));
    }

    // ── export_skills_to_cc ──────────────────────────────────────────────────

    #[test]
    fn export_creates_correct_directory_structure() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let skills = vec![export_skill()];
        let exported = export_skills_to_cc(&skills, dir.path(), None).expect("export skills to cc");

        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].slug, "rust-error-handling");

        let skill_md = dir.path().join("rust-error-handling").join("SKILL.md");
        assert!(
            skill_md.exists(),
            "SKILL.md should exist at {}",
            skill_md.display()
        );
    }

    #[test]
    fn export_skill_md_contains_valid_frontmatter() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let skills = vec![export_skill()];
        export_skills_to_cc(&skills, dir.path(), None).expect("export skills to cc");

        let content =
            std::fs::read_to_string(dir.path().join("rust-error-handling").join("SKILL.md"))
                .expect("read exported SKILL.md");
        assert!(content.starts_with("---\n"));
        assert!(content.contains("name: rust-error-handling"));
        assert!(content.contains("description:"));
        assert!(content.contains("allowed-tools:"));
    }

    #[test]
    fn export_domain_filtering_excludes_non_matching() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let rust_skill = export_skill();
        let mut python_skill = export_skill();
        python_skill.name = "python-testing".to_owned();
        python_skill.domain_tags = vec!["python".to_owned(), "testing".to_owned()];

        let skills = vec![rust_skill, python_skill];
        let exported = export_skills_to_cc(&skills, dir.path(), Some(&["rust"])).expect("export with domain filter");

        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].slug, "rust-error-handling");
    }

    #[test]
    fn export_no_skills_produces_empty_result() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let exported = export_skills_to_cc(&[], dir.path(), None).expect("export empty skills list");
        assert!(exported.is_empty());
    }

    #[test]
    fn export_multiple_skills_creates_separate_directories() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut docker_skill = export_skill();
        docker_skill.name = "docker-diagnostics".to_owned();
        docker_skill.domain_tags = vec!["docker".to_owned()];

        let skills = vec![export_skill(), docker_skill];
        let exported = export_skills_to_cc(&skills, dir.path(), None).expect("export multiple skills");

        assert_eq!(exported.len(), 2);
        assert!(
            dir.path()
                .join("rust-error-handling")
                .join("SKILL.md")
                .exists()
        );
        assert!(
            dir.path()
                .join("docker-diagnostics")
                .join("SKILL.md")
                .exists()
        );
    }

    #[test]
    fn export_roundtrip_content_preserved() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let original = export_skill();
        export_skills_to_cc(std::slice::from_ref(&original), dir.path(), None).expect("export skill for roundtrip");

        // Read back and parse
        let exported_md =
            std::fs::read_to_string(dir.path().join("rust-error-handling").join("SKILL.md"))
                .expect("read back exported SKILL.md");
        let parsed = parse_skill_md(&exported_md, "rust-error-handling").expect("re-parse exported skill md");

        assert_eq!(parsed.name, original.name);
        assert_eq!(parsed.description, original.description);
        assert_eq!(parsed.steps, original.steps);
        assert_eq!(parsed.tools_used, original.tools_used);
    }

    #[test]
    fn export_special_chars_in_name_slugified() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut skill = export_skill();
        skill.name = "C++ Template (Meta)".to_owned();
        let exported = export_skills_to_cc(&[skill], dir.path(), None).expect("export skill with special chars in name");

        assert_eq!(exported[0].slug, "c-template-meta");
        assert!(dir.path().join("c-template-meta").join("SKILL.md").exists());
    }

    #[test]
    fn export_overwrites_existing_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let skill_dir = dir.path().join("rust-error-handling");
        std::fs::create_dir_all(&skill_dir).expect("create pre-existing skill dir");
        std::fs::write(skill_dir.join("SKILL.md"), "old content").expect("write pre-existing SKILL.md");

        let skills = vec![export_skill()];
        export_skills_to_cc(&skills, dir.path(), None).expect("overwrite existing skill");

        let content = std::fs::read_to_string(skill_dir.join("SKILL.md")).expect("read overwritten SKILL.md");
        assert!(
            content.contains("## When to Use"),
            "should have new content"
        );
        assert!(!content.contains("old content"));
    }

    #[test]
    fn export_domain_filter_with_no_matches_returns_empty() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let skills = vec![export_skill()]; // domain_tags: ["rust", "errors"]
        let exported = export_skills_to_cc(&skills, dir.path(), Some(&["python"])).expect("export with non-matching domain filter");
        assert!(exported.is_empty());
    }
}
