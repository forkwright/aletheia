//! Memory flush types for amnesia prevention across distillation boundaries.

use std::fmt::Write;

/// Items to flush to persistent storage before distillation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryFlush {
    /// Key decisions that must survive distillation.
    pub decisions: Vec<FlushItem>,
    /// Corrections that prevent repeating mistakes.
    pub corrections: Vec<FlushItem>,
    /// Facts learned in this session.
    pub facts: Vec<FlushItem>,
    /// Current task state.
    pub task_state: Option<String>,
}

/// A single item to flush to persistent storage.
///
/// Collected inside a [`MemoryFlush`] payload. The [`source`](FlushItem::source)
/// field identifies how the item was detected (see [`FlushSource`]).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FlushItem {
    /// Content of the item to persist.
    pub content: String,
    /// ISO 8601 timestamp when the item was recorded.
    pub timestamp: String,
    /// How the item was identified.
    pub source: FlushSource,
}

/// How a flush item was identified.
///
/// Recorded on each [`FlushItem`] so consumers can distinguish LLM-extracted
/// items from agent-noted or pattern-detected ones.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum FlushSource {
    /// Extracted from conversation by LLM.
    Extracted,
    /// Explicitly noted by the agent.
    AgentNote,
    /// Detected from tool usage patterns.
    ToolPattern,
}

impl FlushSource {
    fn label(&self) -> &str {
        match self {
            Self::Extracted => "extracted",
            Self::AgentNote => "agent_note",
            Self::ToolPattern => "tool_pattern",
        }
    }
}

impl MemoryFlush {
    /// Create an empty flush.
    pub fn empty() -> Self {
        Self {
            decisions: vec![],
            corrections: vec![],
            facts: vec![],
            task_state: None,
        }
    }

    /// Check if there's anything to flush.
    pub fn is_empty(&self) -> bool {
        self.decisions.is_empty()
            && self.corrections.is_empty()
            && self.facts.is_empty()
            && self.task_state.is_none()
    }

    /// Render as markdown for writing to a memory file.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        write_section(&mut out, "Decisions", &self.decisions);
        write_section(&mut out, "Corrections", &self.corrections);
        write_section(&mut out, "Facts", &self.facts);

        if let Some(state) = &self.task_state {
            let _ = writeln!(out, "## Task State\n{state}\n");
        }

        out.trim_end().to_owned()
    }
}

fn write_section(out: &mut String, heading: &str, items: &[FlushItem]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(out, "## {heading}");
    for item in items {
        let _ = writeln!(
            out,
            "- [{}] {} (source: {})",
            item.timestamp,
            item.content,
            item.source.label()
        );
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_flush_empty_is_empty() {
        assert!(MemoryFlush::empty().is_empty());
    }

    #[test]
    fn memory_flush_with_items_not_empty() {
        let flush = MemoryFlush {
            decisions: vec![FlushItem {
                content: "Use snafu for errors".to_owned(),
                timestamp: "2026-03-05T19:00:00Z".to_owned(),
                source: FlushSource::Extracted,
            }],
            corrections: vec![],
            facts: vec![],
            task_state: None,
        };
        assert!(!flush.is_empty());
    }

    #[test]
    fn memory_flush_with_task_state_not_empty() {
        let flush = MemoryFlush {
            decisions: vec![],
            corrections: vec![],
            facts: vec![],
            task_state: Some("Working on auth module".to_owned()),
        };
        assert!(!flush.is_empty());
    }

    #[test]
    fn memory_flush_to_markdown_full() {
        let flush = MemoryFlush {
            decisions: vec![FlushItem {
                content: "Use snafu for errors".to_owned(),
                timestamp: "2026-03-05T19:00:00Z".to_owned(),
                source: FlushSource::Extracted,
            }],
            corrections: vec![FlushItem {
                content: "Wrong file path corrected".to_owned(),
                timestamp: "2026-03-05T19:01:00Z".to_owned(),
                source: FlushSource::AgentNote,
            }],
            facts: vec![FlushItem {
                content: "Config lives in taxis crate".to_owned(),
                timestamp: "2026-03-05T19:02:00Z".to_owned(),
                source: FlushSource::ToolPattern,
            }],
            task_state: Some("Implementing distillation pipeline".to_owned()),
        };

        let md = flush.to_markdown();
        assert!(md.contains("## Decisions"));
        assert!(md.contains("Use snafu for errors"));
        assert!(md.contains("(source: extracted)"));
        assert!(md.contains("## Corrections"));
        assert!(md.contains("(source: agent_note)"));
        assert!(md.contains("## Facts"));
        assert!(md.contains("(source: tool_pattern)"));
        assert!(md.contains("## Task State"));
        assert!(md.contains("Implementing distillation pipeline"));
    }

    #[test]
    fn memory_flush_to_markdown_omits_empty_sections() {
        let flush = MemoryFlush {
            decisions: vec![FlushItem {
                content: "Use actor model".to_owned(),
                timestamp: "2026-03-05T19:00:00Z".to_owned(),
                source: FlushSource::Extracted,
            }],
            corrections: vec![],
            facts: vec![],
            task_state: None,
        };

        let md = flush.to_markdown();
        assert!(md.contains("## Decisions"));
        assert!(!md.contains("## Corrections"));
        assert!(!md.contains("## Facts"));
        assert!(!md.contains("## Task State"));
    }
}
