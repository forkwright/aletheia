//! Skill storage helpers and SKILL.md parser.
//!
//! Skills are facts with `fact_type = "skill"`. This module provides:
//! - Structured content type for skill JSON
//! - Parser for SKILL.md markdown files
//! - Query helpers on [`KnowledgeStore`]

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
        let skill = parse_skill_md(SAMPLE_SKILL, "web-research").unwrap();
        assert_eq!(skill.name, "web-research");
        assert!(skill.description.contains("Systematically research"));
        assert_eq!(skill.steps.len(), 3);
        assert_eq!(skill.steps[0], "Enable web_fetch tool");
        assert_eq!(skill.tools_used, vec!["web_fetch", "web_search"]);
        assert_eq!(skill.origin, "seeded");
    }

    #[test]
    fn parse_skill_with_frontmatter() {
        let skill = parse_skill_md(SAMPLE_WITH_FRONTMATTER, "web-intel").unwrap();
        assert_eq!(skill.tools_used, vec!["web_fetch", "web_search"]);
        assert_eq!(skill.domain_tags, vec!["research", "writing"]);
        assert_eq!(skill.steps.len(), 2);
    }

    #[test]
    fn parse_skill_derives_domain_tags_from_slug() {
        let skill = parse_skill_md(SAMPLE_SKILL, "docker-network-diagnostics").unwrap();
        assert_eq!(skill.domain_tags, vec!["docker", "network", "diagnostics"]);
    }

    #[test]
    fn parse_skill_missing_heading_fails() {
        let bad = "No heading here\n\n## Steps\n1. Do stuff";
        let err = parse_skill_md(bad, "bad-skill").unwrap_err();
        assert!(err.reason.contains("missing top-level heading"));
    }

    #[test]
    fn parse_skill_empty_doc_fails() {
        let err = parse_skill_md("", "empty").unwrap_err();
        assert!(err.reason.contains("empty document"));
    }

    #[test]
    fn parse_skill_no_description_uses_when_to_use() {
        let md = "# Skill\n\n## When to Use\nWhen you need to do things.\n\n## Steps\n1. Do it\n";
        let skill = parse_skill_md(md, "fallback").unwrap();
        assert!(skill.description.contains("When you need to do things"));
    }

    #[test]
    fn parse_skill_no_description_at_all_fails() {
        let md = "# Skill\n\n## Steps\n1. Do it\n";
        let err = parse_skill_md(md, "no-desc").unwrap_err();
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
        let json = serde_json::to_string(&skill).unwrap();
        let back: SkillContent = serde_json::from_str(&json).unwrap();
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
        assert!(fm.unwrap().contains("tools:"));
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
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "# My Skill\nDoes things.\n\n## When to Use\nAlways.\n\n## Steps\n1. Go\n",
        )
        .unwrap();

        let skills = scan_skill_dir(dir.path()).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].0, "my-skill");
    }

    #[test]
    fn scan_skill_dir_empty() {
        let dir = tempfile::tempdir().unwrap();
        let skills = scan_skill_dir(dir.path()).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn scan_skill_dir_ignores_non_skill_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("not-a-skill");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("README.md"), "not a skill").unwrap();

        let skills = scan_skill_dir(dir.path()).unwrap();
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
}
