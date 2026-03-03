//! Tool description tiers for token-efficient tool injection.
//!
//! Two tiers:
//! - **Summary**: name + one-liner, included in bootstrap for all tools
//! - **Expanded**: full description + parameters, loaded on demand

use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolDef;

/// Compact tool summary for bootstrap inclusion.
///
/// One-liner is the first sentence of the tool description, capped at 80 characters.
#[derive(Debug, Clone)]
pub(crate) struct ToolSummary {
    /// Tool name.
    pub name: String,
    /// One-line description (max 80 chars).
    pub one_liner: String,
}

/// Expanded tool description for on-demand loading.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "tool bootstrap injection not yet wired into pipeline")]
pub(crate) struct ToolExpanded {
    /// Tool name.
    pub name: String,
    /// Full description text.
    pub description: String,
    /// Parameter names and their descriptions.
    pub parameters: Vec<(String, String)>,
}

/// Generate compact summaries for all registered tools.
#[must_use]
#[cfg_attr(not(test), expect(dead_code, reason = "tool bootstrap injection not yet wired into pipeline"))]
pub(crate) fn summarize_tools(registry: &ToolRegistry) -> Vec<ToolSummary> {
    registry
        .definitions()
        .iter()
        .map(|def| ToolSummary {
            name: def.name.as_str().to_owned(),
            one_liner: extract_one_liner(&def.description),
        })
        .collect()
}

/// Generate expanded descriptions for selected tool definitions.
#[must_use]
#[expect(dead_code, reason = "tool bootstrap injection not yet wired into pipeline")]
pub(crate) fn expand_tools(defs: &[&ToolDef]) -> Vec<ToolExpanded> {
    defs.iter()
        .map(|def| ToolExpanded {
            name: def.name.as_str().to_owned(),
            description: def
                .extended_description
                .as_deref()
                .unwrap_or(&def.description)
                .to_owned(),
            parameters: def
                .input_schema
                .properties
                .iter()
                .map(|(name, prop)| (name.clone(), prop.description.clone()))
                .collect(),
        })
        .collect()
}

/// Format tool summaries as a markdown section for the system prompt.
#[must_use]
#[cfg_attr(not(test), expect(dead_code, reason = "tool bootstrap injection not yet wired into pipeline"))]
pub(crate) fn format_tool_summary_section(summaries: &[ToolSummary]) -> String {
    if summaries.is_empty() {
        return String::new();
    }

    let mut lines = Vec::with_capacity(summaries.len() + 2);
    lines.push("## Available Tools\n".to_owned());
    for summary in summaries {
        lines.push(format!("- **{}**: {}", summary.name, summary.one_liner));
    }
    lines.join("\n")
}

/// Extract a one-line summary from a description string.
///
/// Takes the first sentence (up to first `. ` or newline), capped at 80 characters.
fn extract_one_liner(description: &str) -> String {
    let first_line = description.lines().next().unwrap_or(description);

    // Find first sentence boundary
    let end = first_line.find(". ").map_or(first_line.len(), |i| i + 1);

    let sentence = &first_line[..end];

    if sentence.len() <= 80 {
        sentence.to_owned()
    } else {
        let truncated = &sentence[..80];
        match truncated.rfind(' ') {
            Some(i) if i > 40 => format!("{}...", &truncated[..i]),
            _ => format!("{truncated}..."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_one_liner_short_description() {
        assert_eq!(extract_one_liner("Read a file."), "Read a file.");
    }

    #[test]
    fn extract_one_liner_truncated() {
        let long = "This is a very long tool description that goes on and on and explains every detail about what the tool does in excruciating detail.";
        let result = extract_one_liner(long);
        assert!(result.len() <= 83); // 80 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn extract_one_liner_sentence_boundary() {
        let desc = "Read a file from disk. Supports all text encodings and binary files.";
        assert_eq!(extract_one_liner(desc), "Read a file from disk.");
    }

    #[test]
    fn summarize_empty_registry() {
        let registry = ToolRegistry::new();
        let summaries = summarize_tools(&registry);
        assert!(summaries.is_empty());
    }

    #[test]
    fn format_produces_markdown() {
        let summaries = vec![
            ToolSummary {
                name: "read".to_owned(),
                one_liner: "Read a file from disk.".to_owned(),
            },
            ToolSummary {
                name: "write".to_owned(),
                one_liner: "Write content to a file.".to_owned(),
            },
        ];
        let section = format_tool_summary_section(&summaries);
        assert!(section.contains("## Available Tools"));
        assert!(section.contains("- **read**: Read a file from disk."));
        assert!(section.contains("- **write**: Write content to a file."));
    }

    #[test]
    fn format_empty_summaries() {
        assert_eq!(format_tool_summary_section(&[]), "");
    }
}
