//! Service traits consumed by tool executors.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

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
