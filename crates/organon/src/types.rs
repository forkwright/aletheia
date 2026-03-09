//! Core types for tool definitions, input/output, and execution context.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

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

/// Cross-nous message routing for tool executors.
pub trait CrossNousService: Send + Sync {
    /// Fire-and-forget send to another agent.
    fn send(
        &self,
        from: &str,
        to: &str,
        session_key: &str,
        content: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    /// Send and wait for a reply from another agent.
    fn ask(
        &self,
        from: &str,
        to: &str,
        session_key: &str,
        content: &str,
        timeout_secs: u64,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

/// Outbound message delivery (Signal, etc.) for tool executors.
pub trait MessageService: Send + Sync {
    /// Send a text message to a recipient.
    fn send_message(
        &self,
        to: &str,
        text: &str,
        from_nous: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;
}

/// Planning project management for tool executors.
pub trait PlanningService: Send + Sync {
    /// Create a new project. Returns JSON representation of the created project.
    fn create_project(
        &self,
        name: &str,
        description: &str,
        scope: Option<&str>,
        mode: &str,
        appetite_minutes: Option<u32>,
        owner: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// Load a project by ID. Returns JSON representation.
    fn load_project(
        &self,
        project_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// Apply a named state transition to a project. Returns updated project JSON.
    ///
    /// Valid transition names:
    /// - `start_questioning`, `start_research`, `skip_research`, `skip_to_research`
    /// - `start_scoping`, `start_planning`, `start_discussion`, `start_execution`
    /// - `start_verification`, `complete`, `abandon`, `pause`, `resume`
    /// - `revert_to_scoping`, `revert_to_planning`, `revert_to_executing`
    fn transition_project(
        &self,
        project_id: &str,
        transition: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// Add a phase to a project. Returns updated project JSON.
    fn add_phase(
        &self,
        project_id: &str,
        name: &str,
        goal: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// Mark a plan as complete within a phase. Returns updated project JSON.
    fn complete_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        achievement: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// Mark a plan as failed within a phase. Returns updated project JSON.
    fn fail_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    /// List all projects. Returns JSON array of project summaries.
    fn list_projects(&self) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}

/// A result from knowledge store search.
pub struct MemoryResult {
    pub id: String,
    pub content: String,
    pub score: f64,
    pub source_type: String,
}

/// Summary of a stored fact for audit display.
pub struct FactSummary {
    pub id: String,
    pub content: String,
    pub confidence: f64,
    pub tier: String,
    pub recorded_at: String,
    pub is_forgotten: bool,
    pub forgotten_at: Option<String>,
    pub forget_reason: Option<String>,
}

/// Abstracts knowledge store operations for memory tools.
///
/// Implemented by an adapter in the binary crate wrapping `KnowledgeStore` + `EmbeddingProvider`.
pub trait KnowledgeSearchService: Send + Sync {
    fn search(
        &self,
        query: &str,
        nous_id: &str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryResult>, String>> + Send + '_>>;

    fn correct_fact(
        &self,
        fact_id: &str,
        new_content: &str,
        nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;

    fn retract_fact(
        &self,
        fact_id: &str,
        reason: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    fn audit_facts(
        &self,
        nous_id: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FactSummary>, String>> + Send + '_>>;

    fn forget_fact(
        &self,
        fact_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    fn unforget_fact(
        &self,
        fact_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    fn datalog_query(
        &self,
        query: &str,
        params: Option<serde_json::Value>,
        timeout_secs: Option<f64>,
        row_limit: Option<usize>,
    ) -> Pin<Box<dyn Future<Output = Result<DatalogResult, String>> + Send + '_>>;
}

/// Result from a read-only Datalog query.
pub struct DatalogResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}

/// Service locator for tool executors needing access to runtime services.
pub struct ToolServices {
    pub cross_nous: Option<Arc<dyn CrossNousService>>,
    pub messenger: Option<Arc<dyn MessageService>>,
    pub note_store: Option<Arc<dyn NoteStore>>,
    pub blackboard_store: Option<Arc<dyn BlackboardStore>>,
    pub spawn: Option<Arc<dyn SpawnService>>,
    pub planning: Option<Arc<dyn PlanningService>>,
    pub knowledge: Option<Arc<dyn KnowledgeSearchService>>,
    pub http_client: reqwest::Client,
    /// Catalog of lazy tools available for activation via `enable_tool`.
    pub lazy_tool_catalog: Vec<(ToolName, String)>,
}

impl std::fmt::Debug for ToolServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolServices")
            .field("cross_nous", &self.cross_nous.is_some())
            .field("messenger", &self.messenger.is_some())
            .field("note_store", &self.note_store.is_some())
            .field("blackboard_store", &self.blackboard_store.is_some())
            .field("spawn", &self.spawn.is_some())
            .field("planning", &self.planning.is_some())
            .field("knowledge", &self.knowledge.is_some())
            .field("lazy_tool_catalog_len", &self.lazy_tool_catalog.len())
            .finish_non_exhaustive()
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
    /// Optional runtime services for tools that need cross-cutting capabilities.
    pub services: Option<Arc<ToolServices>>,
    /// Per-session set of dynamically activated tools (via `enable_tool`).
    pub active_tools: Arc<RwLock<HashSet<ToolName>>>,
}

/// Persistent session notes storage.
pub trait NoteStore: Send + Sync {
    fn add_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> std::result::Result<i64, Box<dyn std::error::Error + Send + Sync>>;

    fn get_notes(
        &self,
        session_id: &str,
    ) -> std::result::Result<Vec<NoteEntry>, Box<dyn std::error::Error + Send + Sync>>;

    fn delete_note(
        &self,
        note_id: i64,
    ) -> std::result::Result<bool, Box<dyn std::error::Error + Send + Sync>>;
}

/// Shared blackboard state with TTL.
pub trait BlackboardStore: Send + Sync {
    fn write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_seconds: i64,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

    fn read(
        &self,
        key: &str,
    ) -> std::result::Result<Option<BlackboardEntry>, Box<dyn std::error::Error + Send + Sync>>;

    fn list(
        &self,
    ) -> std::result::Result<Vec<BlackboardEntry>, Box<dyn std::error::Error + Send + Sync>>;

    fn delete(
        &self,
        key: &str,
        author: &str,
    ) -> std::result::Result<bool, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Debug, Clone)]
pub struct NoteEntry {
    pub id: i64,
    pub category: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct BlackboardEntry {
    pub key: String,
    pub value: String,
    pub author_nous_id: String,
    pub ttl_seconds: i64,
    pub created_at: String,
    pub expires_at: Option<String>,
}

/// Request to spawn an ephemeral sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnRequest {
    /// Role identifier (coder, reviewer, researcher, explorer, runner).
    pub role: String,
    /// Task prompt sent as the single turn.
    pub task: String,
    /// Model override (None = role-based default).
    pub model: Option<String>,
    /// Tool name whitelist (None = role-based defaults).
    pub allowed_tools: Option<Vec<String>>,
    /// Maximum seconds before the sub-agent is killed.
    pub timeout_secs: u64,
}

/// Result from an ephemeral sub-agent's single turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResult {
    /// The sub-agent's text response.
    pub content: String,
    /// Whether the sub-agent encountered an error.
    pub is_error: bool,
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens produced.
    pub output_tokens: u64,
}

/// Ephemeral sub-agent spawning for tool executors.
pub trait SpawnService: Send + Sync {
    /// Spawn an ephemeral actor, run one turn, collect the result, shut down.
    fn spawn_and_run(
        &self,
        request: SpawnRequest,
        parent_nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>>;
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
        assert_eq!(ToolCategory::Research.to_string(), "research");
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
