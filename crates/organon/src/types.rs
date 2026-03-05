//! Core types for tool definitions, input/output, and execution context.

use std::path::PathBuf;

use aletheia_koina::id::{NousId, SessionId, ToolName};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub use aletheia_hermeneus::types::{
    DocumentSource, ImageSource, ToolResultBlock, ToolResultContent,
};

/// Tool definition — the rich metadata that organon tracks internally.
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
    pub fn to_json_schema(&self) -> serde_json::Value {
        let mut props = serde_json::Map::new();
        for (name, def) in &self.properties {
            let mut prop = serde_json::Map::new();
            prop.insert(
                "type".to_owned(),
                serde_json::to_value(def.property_type)
                    .unwrap_or(serde_json::Value::String("string".to_owned())),
            );
            prop.insert(
                "description".to_owned(),
                serde_json::Value::String(def.description.clone()),
            );
            if let Some(ref enum_vals) = def.enum_values {
                prop.insert(
                    "enum".to_owned(),
                    serde_json::Value::Array(
                        enum_vals
                            .iter()
                            .map(|v| serde_json::Value::String(v.clone()))
                            .collect(),
                    ),
                );
            }
            if let Some(ref default) = def.default {
                prop.insert("default".to_owned(), default.clone());
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
    String,
    Number,
    Integer,
    Boolean,
    Array,
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

/// Semantic tool category — classifies tool purpose.
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
            Self::Domain => f.write_str("domain"),
        }
    }
}

/// What the tool executor returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Result content — text or rich content blocks.
    pub content: ToolResultContent,
    /// Whether this result represents an error.
    pub is_error: bool,
}

impl ToolResult {
    /// Create a text-only success result.
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: false,
        }
    }

    /// Create a text-only error result.
    #[must_use]
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: true,
        }
    }

    /// Create a result with rich content blocks.
    #[must_use]
    pub fn blocks(blocks: Vec<ToolResultBlock>) -> Self {
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
pub struct ToolStats {
    pub total_calls: u32,
    pub total_duration_ms: u64,
    pub error_count: u32,
    pub calls_by_tool: IndexMap<String, u32>,
}

impl ToolStats {
    /// Record a tool execution.
    pub fn record(&mut self, name: &str, duration_ms: u64, is_error: bool) {
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

/// Execution context passed to every tool invocation.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// The agent executing this tool.
    pub nous_id: NousId,
    /// Current session.
    pub session_id: SessionId,
    /// Agent workspace root.
    pub workspace: PathBuf,
    /// Allowed filesystem roots for sandboxing.
    pub allowed_roots: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_def_serde_roundtrip() {
        let def = ToolDef {
            name: ToolName::new("test_tool").expect("valid"),
            description: "A test tool".to_owned(),
            extended_description: Some("Detailed description".to_owned()),
            input_schema: InputSchema {
                properties: IndexMap::from([(
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                )]),
                required: vec!["path".to_owned()],
            },
            category: ToolCategory::Workspace,
            auto_activate: false,
        };
        let json = serde_json::to_string(&def).expect("serialize");
        let back: ToolDef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name.as_str(), "test_tool");
        assert_eq!(back.category, ToolCategory::Workspace);
    }

    #[test]
    fn input_schema_to_json_schema() {
        let schema = InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "max_lines".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum lines".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(100)),
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        };
        let json_schema = schema.to_json_schema();
        assert_eq!(json_schema["type"], "object");
        assert_eq!(json_schema["properties"]["path"]["type"], "string");
        assert_eq!(json_schema["properties"]["max_lines"]["default"], 100);
        assert_eq!(json_schema["required"][0], "path");
    }

    #[test]
    fn property_type_serde() {
        let json = serde_json::to_string(&PropertyType::Integer).expect("serialize");
        assert_eq!(json, "\"integer\"");
        let back: PropertyType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, PropertyType::Integer);
    }

    #[test]
    fn tool_category_display() {
        assert_eq!(ToolCategory::Workspace.to_string(), "workspace");
        assert_eq!(ToolCategory::Communication.to_string(), "communication");
    }

    #[test]
    fn tool_stats_record_accumulates() {
        let mut stats = ToolStats::default();
        stats.record("read", 10, false);
        stats.record("write", 20, false);
        stats.record("read", 15, true);
        assert_eq!(stats.total_calls, 3);
        assert_eq!(stats.total_duration_ms, 45);
        assert_eq!(stats.error_count, 1);
        assert_eq!(stats.calls_by_tool["read"], 2);
        assert_eq!(stats.calls_by_tool["write"], 1);
    }

    #[test]
    fn tool_stats_top_tools() {
        let mut stats = ToolStats::default();
        stats.record("a", 1, false);
        stats.record("b", 1, false);
        stats.record("b", 1, false);
        stats.record("c", 1, false);
        stats.record("c", 1, false);
        stats.record("c", 1, false);
        let top = stats.top_tools(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0], ("c", 3));
        assert_eq!(top[1], ("b", 2));
    }

    #[test]
    fn tool_input_serde_roundtrip() {
        let input = ToolInput {
            name: ToolName::new("read").expect("valid"),
            tool_use_id: "toolu_123".to_owned(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        let json = serde_json::to_string(&input).expect("serialize");
        let back: ToolInput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name.as_str(), "read");
        assert_eq!(back.tool_use_id, "toolu_123");
    }

    #[test]
    fn tool_result_text_constructor() {
        let r = ToolResult::text("hello");
        assert_eq!(r.content.text_summary(), "hello");
        assert!(!r.is_error);
    }

    #[test]
    fn tool_result_error_constructor() {
        let r = ToolResult::error("bad input");
        assert_eq!(r.content.text_summary(), "bad input");
        assert!(r.is_error);
    }

    #[test]
    fn tool_result_blocks_constructor() {
        let r = ToolResult::blocks(vec![ToolResultBlock::Text {
            text: "desc".to_owned(),
        }]);
        assert_eq!(r.content.text_summary(), "desc");
        assert!(!r.is_error);
    }
}
