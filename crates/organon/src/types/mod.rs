//! Core types for tool definitions, input/output, and execution context.

mod context;
mod services;

pub use context::*;
pub use services::*;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub use aletheia_hermeneus::types::{
    DocumentSource, ImageSource, ToolResultBlock, ToolResultContent,
};
use aletheia_koina::id::ToolName;

/// Tool definition: the rich metadata that organon tracks internally.
///
/// Converted to `hermeneus::types::ToolDefinition` (the lean LLM wire format)
/// via `ToolRegistry::to_hermeneus_tools`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    /// Validated tool name.
    pub name: ToolName,
    /// Short description sent to the LLM (token-budget friendly).
    pub description: String,
    /// Detailed description for extended-thinking mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: InputSchema,
    /// Semantic category.
    pub category: ToolCategory,
    /// How reversible this tool's effects are.
    pub reversibility: Reversibility,
    /// Whether the tool activates automatically by domain without explicit config.
    pub auto_activate: bool,
}

/// JSON Schema for tool input parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSchema {
    /// Property definitions, insertion-ordered.
    pub properties: IndexMap<String, PropertyDef>,
    /// Names of required properties.
    pub required: Vec<String>,
}

impl InputSchema {
    /// Serialize to a JSON Schema `{"type": "object", ...}` value
    /// suitable for `hermeneus::types::ToolDefinition::input_schema`.
    #[must_use]
    pub fn to_json_schema(&self) -> serde_json::Value {
        // kanon:ignore RUST/pub-visibility
        let mut props = serde_json::Map::new();
        for (name, def) in &self.properties {
            let mut prop = serde_json::Map::new();
            prop.insert(
                String::from("type"),
                serde_json::to_value(def.property_type)
                    .unwrap_or(serde_json::Value::String("string".to_owned())),
            );
            prop.insert(
                String::from("description"),
                serde_json::Value::String(def.description.clone()),
            );
            if let Some(ref enum_vals) = def.enum_values {
                prop.insert(
                    String::from("enum"),
                    serde_json::Value::Array(
                        enum_vals
                            .iter()
                            .map(|v| serde_json::Value::String(v.clone()))
                            .collect(),
                    ),
                );
            }
            if let Some(ref default) = def.default {
                prop.insert(String::from("default"), default.clone());
            }
            props.insert(name.clone(), serde_json::Value::Object(prop));
        }

        serde_json::json!({
            "type": "object",
            "properties": props,
            "required": self.required,
        })
    }
}

/// A single property in the input schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDef {
    /// The JSON Schema type.
    #[serde(rename = "type")]
    pub property_type: PropertyType,
    /// Human-readable description.
    pub description: String,
    /// Allowed enum values, if constrained.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

/// JSON Schema primitive types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PropertyType {
    /// JSON string type.
    String,
    /// JSON number type (float or integer).
    Number,
    /// JSON integer type.
    Integer,
    /// JSON boolean type.
    Boolean,
    /// JSON array type.
    Array,
    /// JSON object type.
    Object,
}

impl std::fmt::Display for PropertyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => f.write_str("string"),
            Self::Number => f.write_str("number"),
            Self::Integer => f.write_str("integer"),
            Self::Boolean => f.write_str("boolean"),
            Self::Array => f.write_str("array"),
            Self::Object => f.write_str("object"),
        }
    }
}

/// How reversible a tool's effects are.
///
/// Used by the approval system to determine which tool calls need confirmation
/// and whether counterfactual dry-run simulation is meaningful.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reversibility {
    /// Read-only operations with no side effects (ls, read, search).
    FullyReversible,
    /// Can be undone (write file with backup, git commit can revert).
    Reversible,
    /// Partial undo possible (delete file can restore from backup if exists).
    PartiallyReversible,
    /// Cannot be undone (exec with external side effects, API calls, messages).
    #[default]
    Irreversible,
}

impl std::fmt::Display for Reversibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FullyReversible => f.write_str("fully_reversible"),
            Self::Reversible => f.write_str("reversible"),
            Self::PartiallyReversible => f.write_str("partially_reversible"),
            Self::Irreversible => f.write_str("irreversible"),
        }
    }
}

impl Reversibility {
    /// Whether this tool's effects can be simulated in a dry run.
    #[must_use]
    pub fn supports_dry_run(self) -> bool {
        matches!(self, Self::FullyReversible | Self::Reversible)
    }
}

/// What level of approval a tool call requires before execution.
///
/// Derived from [`Reversibility`] but can be overridden per-tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalRequirement {
    /// No approval needed (read-only, fully reversible).
    None,
    /// Approval recommended but not enforced.
    Advisory,
    /// Approval required before execution.
    Required,
    /// Approval required with explicit confirmation prompt.
    Mandatory,
}

impl std::fmt::Display for ApprovalRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Advisory => f.write_str("advisory"),
            Self::Required => f.write_str("required"),
            Self::Mandatory => f.write_str("mandatory"),
        }
    }
}

impl From<Reversibility> for ApprovalRequirement {
    fn from(rev: Reversibility) -> Self {
        match rev {
            Reversibility::FullyReversible => Self::None,
            Reversibility::Reversible => Self::Advisory,
            Reversibility::PartiallyReversible => Self::Required,
            Reversibility::Irreversible => Self::Mandatory,
        }
    }
}

/// Metadata recorded per tool call for session audit trails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallMetadata {
    /// The tool's reversibility classification at call time.
    pub reversibility: Reversibility,
    /// The approval requirement that was applied.
    pub approval: ApprovalRequirement,
    /// Whether the call was a dry-run simulation.
    pub dry_run: bool,
}

/// Semantic tool category: classifies tool purpose.
///
/// This is a semantic classification, not a loading strategy.
/// The loading strategy (essential vs available) is orthogonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolCategory {
    /// Filesystem operations.
    Workspace,
    /// Memory operations.
    Memory,
    /// Messaging and cross-agent communication.
    Communication,
    /// Planning and deliberation.
    Planning,
    /// System and configuration.
    System,
    /// Agent coordination and spawning.
    Agent,
    /// Web research and information retrieval.
    Research,
    /// External domain pack tools.
    Domain,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workspace => f.write_str("workspace"),
            Self::Memory => f.write_str("memory"),
            Self::Communication => f.write_str("communication"),
            Self::Planning => f.write_str("planning"),
            Self::System => f.write_str("system"),
            Self::Agent => f.write_str("agent"),
            Self::Research => f.write_str("research"),
            Self::Domain => f.write_str("domain"),
        }
    }
}

impl ToolCategory {
    /// Unicode icon for TUI display.
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Workspace => "≡",
            Self::Memory => "⊙",
            Self::Communication => "↔",
            Self::Planning => "◈",
            Self::System => "⚙",
            Self::Agent => "⊛",
            Self::Research => "⊕",
            Self::Domain => "○",
        }
    }

    /// Human-readable display name.
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::Memory => "Memory",
            Self::Communication => "Communication",
            Self::Planning => "Planning",
            Self::System => "System",
            Self::Agent => "Agent",
            Self::Research => "Research",
            Self::Domain => "Domain",
        }
    }

    /// Whether this category is read-only (non-destructive).
    #[must_use]
    pub fn is_read_only(self) -> bool {
        matches!(self, Self::Research | Self::Planning)
    }

    /// Whether this category performs destructive or irreversible operations.
    #[must_use]
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::System | Self::Agent | Self::Communication)
    }
}

/// What the tool executor returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Result content: text or rich content blocks.
    pub content: ToolResultContent,
    /// Whether this result represents an error.
    pub is_error: bool,
}

impl ToolResult {
    /// Create a text-only success result.
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: false,
        }
    }

    /// Create a text-only error result.
    #[must_use]
    pub fn error(content: impl Into<String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: true,
        }
    }

    /// Create a result with rich content blocks.
    #[must_use]
    pub fn blocks(blocks: Vec<ToolResultBlock>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            content: ToolResultContent::Blocks(blocks),
            is_error: false,
        }
    }
}

/// Input to a tool execution (from the LLM's `tool_use` block).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    /// Which tool to invoke.
    pub name: ToolName,
    /// The `tool_use` block ID from the LLM response.
    pub tool_use_id: String,
    /// The arguments the LLM provided.
    pub arguments: serde_json::Value,
}

/// Cumulative tool execution statistics for a pipeline turn.
#[derive(Debug, Clone, Default, Serialize)]
#[expect(
    missing_docs,
    reason = "stats struct fields are self-documenting by name"
)]
pub struct ToolStats {
    pub total_calls: u32,
    pub total_duration_ms: u64,
    pub error_count: u32,
    pub calls_by_tool: IndexMap<String, u32>,
}

impl ToolStats {
    /// Record a tool execution.
    pub fn record(&mut self, name: &str, duration_ms: u64, is_error: bool) {
        // kanon:ignore RUST/pub-visibility
        self.total_calls += 1;
        self.total_duration_ms += duration_ms;
        if is_error {
            self.error_count += 1;
        }
        *self.calls_by_tool.entry(name.to_owned()).or_insert(0) += 1;
    }

    /// Top N tools by call count.
    #[must_use]
    pub fn top_tools(&self, n: usize) -> Vec<(&str, u32)> {
        // kanon:ignore RUST/pub-visibility
        let mut sorted: Vec<_> = self
            .calls_by_tool
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(n);
        sorted
    }
}

#[cfg(test)]
#[path = "../types_tests.rs"]
mod tests;
