//! Tool description tiers for token-efficient tool injection.
//!
//! Two tiers:
//! - **Summary**: name + one-liner, included in bootstrap for all tools
//! - **Expanded**: full description + parameters, loaded on demand

use organon::registry::ToolRegistry;
use organon::types::ToolDef;

use super::{BootstrapSection, BootstrapSlot, SectionPriority};
use crate::budget::CharEstimator;

/// Compact tool summary for bootstrap inclusion.
///
/// One-liner is the first sentence of the tool description, capped at 80 characters.
#[derive(Debug, Clone)]
pub struct ToolSummary {
    /// Tool name.
    pub name: String,
    /// One-line description (max 80 chars).
    pub one_liner: String,
}

/// Expanded tool description for on-demand loading.
#[derive(Debug, Clone)]
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
pub(crate) fn format_tool_summary_section(
    summaries: &[ToolSummary],
    expanded: &[ToolExpanded],
) -> String {
    if summaries.is_empty() {
        return String::new();
    }

    let mut lines = Vec::with_capacity(summaries.len() + expanded.len() + 4);
    lines.push("## Available Tools\n".to_owned());
    for summary in summaries {
        lines.push(format!("- **{}**: {}", summary.name, summary.one_liner));
    }
    let parameter_lines: Vec<String> = expanded
        .iter()
        .filter(|tool| !tool.parameters.is_empty())
        .map(|tool| {
            let names = tool
                .parameters
                .iter()
                .map(|(name, _description)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let one_liner = extract_one_liner(&tool.description);
            format!("- **{}**: {one_liner} Parameters: {names}", tool.name)
        })
        .collect();
    if !parameter_lines.is_empty() {
        lines.push("\n### Tool Parameters".to_owned());
        lines.extend(parameter_lines);
    }
    lines.join("\n")
}

/// Build a budgeted bootstrap section from the live tool registry.
#[must_use]
pub(crate) fn tool_summary_bootstrap_section(
    registry: &ToolRegistry,
    estimator: &CharEstimator,
) -> Option<BootstrapSection> {
    let summaries = summarize_tools(registry);
    let definitions = registry.definitions();
    let expanded = expand_tools(&definitions);
    let content = format_tool_summary_section(&summaries, &expanded);
    if content.is_empty() {
        return None;
    }

    Some(BootstrapSection {
        name: "tools-summary".to_owned(),
        priority: SectionPriority::Important,
        tokens: estimator.estimate(&content),
        content,
        truncatable: true,
        slot: BootstrapSlot::Tools,
    })
}

/// Extract a one-line summary from a description string.
///
/// Takes the first sentence (up to first `. ` or newline), capped at 80 characters.
fn extract_one_liner(description: &str) -> String {
    let first_line = description.lines().next().unwrap_or(description);

    let end = first_line.find(". ").map_or(first_line.len(), |i| i + 1);

    let sentence = first_line.get(..end).unwrap_or(first_line);

    if sentence.len() <= 80 {
        sentence.to_owned()
    } else {
        let truncated = sentence.get(..80).unwrap_or(sentence);
        match truncated.rfind(' ') {
            Some(i) if i > 40 => format!("{}...", truncated.get(..i).unwrap_or(truncated)),
            _ => format!("{truncated}..."),
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
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
        let section = format_tool_summary_section(&summaries, &[]);
        assert!(section.contains("## Available Tools"));
        assert!(section.contains("- **read**: Read a file from disk."));
        assert!(section.contains("- **write**: Write content to a file."));
    }

    #[test]
    fn format_empty_summaries() {
        assert_eq!(format_tool_summary_section(&[], &[]), "");
    }

    #[test]
    fn registry_summary_becomes_bootstrap_section() {
        let mut registry = ToolRegistry::new();
        registry
            .register(
                organon::types::ToolDef {
                    name: koina::id::ToolName::new("read_file").expect("valid tool name"),
                    description: "Read a file from disk. Extra details.".to_owned(),
                    extended_description: None,
                    input_schema: organon::types::InputSchema {
                        properties: indexmap::IndexMap::new(),
                        required: Vec::new(),
                    },
                    category: organon::types::ToolCategory::Workspace,
                    reversibility: organon::types::Reversibility::FullyReversible,
                    auto_activate: false,
                    groups: vec![organon::types::ToolGroupId::Read],
                    tags: Vec::new(),
                },
                Box::new(NoopExecutor),
            )
            .expect("register tool");

        let section = tool_summary_bootstrap_section(&registry, &CharEstimator::default())
            .expect("summary section");
        assert_eq!(section.name, "tools-summary");
        assert_eq!(section.slot, BootstrapSlot::Tools);
        assert!(
            section
                .content
                .contains("- **read_file**: Read a file from disk.")
        );
        assert!(section.tokens > 0);
    }

    struct NoopExecutor;

    impl organon::registry::ToolExecutor for NoopExecutor {
        fn execute<'a>(
            &'a self,
            _input: &'a organon::types::ToolInput,
            _ctx: &'a organon::types::ToolContext,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = organon::error::Result<organon::types::ToolResult>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(organon::types::ToolResult::text("ok")) })
        }
    }
}
