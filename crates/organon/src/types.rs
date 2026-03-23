//! Core types for tool definitions, input/output, and execution context.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub use aletheia_hermeneus::types::{
    DocumentSource, ImageSource, ToolResultBlock, ToolResultContent,
};
use aletheia_koina::id::{NousId, SessionId, ToolName};

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

use crate::error::{KnowledgeAdapterError, PlanningAdapterError};

/// Cross-nous message routing for tool executors.
pub trait CrossNousService: Send + Sync {
    // kanon:ignore RUST/pub-visibility
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
    // kanon:ignore RUST/pub-visibility
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
    // kanon:ignore RUST/pub-visibility
    /// Create a new project. Returns JSON representation of the created project.
    fn create_project(
        &self,
        name: &str,
        description: &str,
        scope: Option<&str>,
        mode: &str,
        appetite_minutes: Option<u32>,
        owner: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;

    /// Load a project by ID. Returns JSON representation.
    fn load_project(
        &self,
        project_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;

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
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;

    /// Add a phase to a project. Returns updated project JSON.
    fn add_phase(
        &self,
        project_id: &str,
        name: &str,
        goal: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;

    /// Mark a plan as complete within a phase. Returns updated project JSON.
    fn complete_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        achievement: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;

    /// Mark a plan as failed within a phase. Returns updated project JSON.
    fn fail_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;

    /// List all projects. Returns JSON array of project summaries.
    fn list_projects(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;

    /// Verify phase criteria against provided evidence.
    ///
    /// `criteria_json` is a JSON array of criterion evaluations, each with:
    /// `criterion` (string), `status` ("met"/"partially-met"/"not-met"),
    /// `evidence` (array of `{kind, content}`), `detail` (string),
    /// and optional `proposed_fix` (string).
    ///
    /// Returns a JSON verification result with overall status, per-criterion
    /// results, gaps, and goal-backward traces.
    fn verify_criteria(
        &self,
        project_id: &str,
        phase_id: &str,
        criteria_json: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
}

/// A result from knowledge store search.
#[expect(
    missing_docs,
    reason = "result struct fields are self-documenting by name"
)]
pub struct MemoryResult {
    // kanon:ignore RUST/pub-visibility
    pub id: String,
    pub content: String,
    pub score: f64,
    pub source_type: String,
}

/// Summary of a stored fact for audit display.
#[expect(
    missing_docs,
    reason = "summary struct fields are self-documenting by name"
)]
pub struct FactSummary {
    // kanon:ignore RUST/pub-visibility
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
    // kanon:ignore RUST/pub-visibility
    /// Semantic search over the knowledge store.
    fn search(
        &self,
        query: &str,
        nous_id: &str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryResult>, KnowledgeAdapterError>> + Send + '_>>;

    /// Update the content of an existing fact.
    fn correct_fact(
        &self,
        fact_id: &str,
        new_content: &str,
        nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, KnowledgeAdapterError>> + Send + '_>>;

    /// Mark a fact as retracted (soft-delete).
    fn retract_fact(
        &self,
        fact_id: &str,
        reason: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), KnowledgeAdapterError>> + Send + '_>>;

    /// Return a list of recent facts for audit review.
    fn audit_facts(
        &self,
        nous_id: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FactSummary>, KnowledgeAdapterError>> + Send + '_>>;

    /// Mark a fact as forgotten with a given reason.
    fn forget_fact(
        &self,
        fact_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>>;

    /// Restore a previously forgotten fact.
    fn unforget_fact(
        &self,
        fact_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>>;

    /// Execute a read-only Datalog query against the knowledge graph.
    fn datalog_query(
        &self,
        query: &str,
        params: Option<serde_json::Value>,
        timeout_secs: Option<f64>,
        row_limit: Option<usize>,
    ) -> Pin<Box<dyn Future<Output = Result<DatalogResult, KnowledgeAdapterError>> + Send + '_>>;
}

/// Result from a read-only Datalog query.
#[expect(
    missing_docs,
    reason = "result struct fields are self-documenting by name"
)]
pub struct DatalogResult {
    // kanon:ignore RUST/pub-visibility
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}

/// Configuration for server-side tools that execute on the API provider's infrastructure.
///
/// Controls which server tools are available for per-session activation via `enable_tool`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerToolConfig {
    /// Whether web search is available for activation.
    #[serde(default)]
    pub web_search: bool,
    /// Maximum web search uses per turn (None = provider default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search_max_uses: Option<u32>,
    /// Whether code execution is available for activation.
    #[serde(default)]
    pub code_execution: bool,
}

impl ServerToolConfig {
    /// Generate catalog entries for server tools available via `enable_tool`.
    #[must_use]
    pub fn catalog_entries(&self) -> Vec<(ToolName, String)> {
        // kanon:ignore RUST/pub-visibility
        let mut entries = Vec::new();
        if self.web_search {
            entries.push((
                ToolName::from_static("web_search"), // kanon:ignore RUST/expect
                "Search the web using Anthropic's server-side web search".to_owned(),
            ));
        }
        if self.code_execution {
            entries.push((
                ToolName::from_static("code_execution"), // kanon:ignore RUST/expect
                "Execute Python code in a sandboxed server-side environment".to_owned(),
            ));
        }
        entries
    }

    /// Produce server tool definitions for tools that are currently active.
    #[must_use]
    pub fn active_definitions(
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<aletheia_hermeneus::types::ServerToolDefinition> {
        let mut defs = Vec::new();
        let web_search_name = ToolName::from_static("web_search"); // kanon:ignore RUST/expect
        let code_exec_name = ToolName::from_static("code_execution"); // kanon:ignore RUST/expect

        if self.web_search && active.contains(&web_search_name) {
            defs.push(aletheia_hermeneus::types::ServerToolDefinition {
                tool_type: "web_search_20250305".to_owned(),
                name: "web_search".to_owned(),
                max_uses: self.web_search_max_uses,
                allowed_domains: None,
                blocked_domains: None,
                user_location: None,
            });
        }
        if self.code_execution && active.contains(&code_exec_name) {
            defs.push(aletheia_hermeneus::types::ServerToolDefinition {
                tool_type: "code_execution_20250522".to_owned(),
                name: "code_execution".to_owned(),
                max_uses: None,
                allowed_domains: None,
                blocked_domains: None,
                user_location: None,
            });
        }
        defs
    }
}

/// Service locator for tool executors needing access to runtime services.
#[expect(
    missing_docs,
    reason = "service locator fields are self-documenting by name"
)]
pub struct ToolServices {
    // kanon:ignore RUST/pub-visibility
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
    /// Server tool configuration for provider-side tools (web search, code execution).
    pub server_tool_config: ServerToolConfig,
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
            .field("server_tool_config", &self.server_tool_config)
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
    // kanon:ignore RUST/pub-visibility
    /// Add a note to a session.
    fn add_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> std::result::Result<i64, crate::error::StoreError>;

    /// Retrieve all notes for a session.
    fn get_notes(
        &self,
        session_id: &str,
    ) -> std::result::Result<Vec<NoteEntry>, crate::error::StoreError>;

    /// Delete a note by ID. Returns `true` if it existed.
    fn delete_note(&self, note_id: i64) -> std::result::Result<bool, crate::error::StoreError>;
}

/// Shared blackboard state with TTL.
pub trait BlackboardStore: Send + Sync {
    // kanon:ignore RUST/pub-visibility
    /// Write a key-value entry with an author and TTL.
    fn write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_seconds: i64,
    ) -> std::result::Result<(), crate::error::StoreError>;

    /// Read a single entry by key.
    fn read(
        &self,
        key: &str,
    ) -> std::result::Result<Option<BlackboardEntry>, crate::error::StoreError>;

    /// List all current entries.
    fn list(&self) -> std::result::Result<Vec<BlackboardEntry>, crate::error::StoreError>;

    /// Delete an entry by key and author.
    fn delete(
        &self,
        key: &str,
        author: &str,
    ) -> std::result::Result<bool, crate::error::StoreError>;
}

/// A stored session note entry.
#[derive(Debug, Clone)]
#[expect(
    missing_docs,
    reason = "note entry fields are self-documenting by name"
)]
pub struct NoteEntry {
    pub id: i64,
    pub category: String,
    pub content: String,
    pub created_at: String,
}

/// A blackboard key-value entry with TTL.
#[derive(Debug, Clone)]
#[expect(
    missing_docs,
    reason = "blackboard entry fields are self-documenting by name"
)]
pub struct BlackboardEntry {
    pub key: String, // kanon:ignore RUST/plain-string-secret
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
    /// Tool name allowlist (None = role-based defaults).
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
    // kanon:ignore RUST/pub-visibility
    /// Spawn an ephemeral actor, run one turn, collect the result, shut down.
    fn spawn_and_run(
        &self,
        request: SpawnRequest,
        parent_nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>>;
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
