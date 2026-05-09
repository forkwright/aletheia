//! Service traits consumed by tool executors.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::{KnowledgeAdapterError, PlanningAdapterError};

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

    /// Add an executable plan to a phase. Returns updated project JSON.
    fn add_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan: PlanningPlanInput<'_>,
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

/// Input for creating an executable plan inside a phase.
pub struct PlanningPlanInput<'a> {
    /// Short title for the executable plan.
    pub title: &'a str,
    /// Concrete work description.
    pub description: &'a str,
    /// Execution wave; plans in the same wave may run in parallel.
    pub wave: u32,
    /// Plan IDs that must complete before this plan can run.
    pub depends_on: &'a [String],
    /// Optional maximum iterations before the plan is stuck.
    pub max_iterations: Option<u32>,
}

/// A result from knowledge store search.
#[expect(
    missing_docs,
    reason = "result struct fields are self-documenting by name"
)]
pub struct MemoryResult {
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

    /// Look up a skill by name for the given nous.
    ///
    /// Returns the skill's raw JSON content when found, or `None` when no
    /// matching skill exists.
    fn find_skill_by_name(
        &self,
        nous_id: &str,
        skill_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, KnowledgeAdapterError>> + Send + '_>>;
}

/// Result from a read-only Datalog query.
#[expect(
    missing_docs,
    reason = "result struct fields are self-documenting by name"
)]
pub struct DatalogResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}

/// Persistent session notes storage.
pub trait NoteStore: Send + Sync {
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

/// Working checkpoint storage for agent-curated session memory.
///
/// Agents call `update_working_checkpoint` to persist structured key-info
/// that survives context compaction and is reinjected on subsequent turns.
pub trait WorkingCheckpointStore: Send + Sync {
    /// Persist a checkpoint for the given session and turn.
    ///
    /// # Errors
    ///
    /// Returns `StoreError` on persistence failure.
    fn write_checkpoint(
        &self,
        session_id: &str,
        turn_number: u64,
        content: &str,
    ) -> std::result::Result<(), crate::error::StoreError>;

    /// Read the most recent checkpoint for a session.
    ///
    /// Returns `None` if no checkpoint exists.
    fn read_latest(
        &self,
        session_id: &str,
    ) -> std::result::Result<Option<WorkingCheckpoint>, crate::error::StoreError>;

    /// Read up to `limit` most recent checkpoints for a session, newest first.
    fn read_recent(
        &self,
        session_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<WorkingCheckpoint>, crate::error::StoreError>;
}

/// A single agent-curated working checkpoint.
#[derive(Debug, Clone)]
pub struct WorkingCheckpoint {
    /// Session identifier.
    pub session_id: String,
    /// Turn number when the checkpoint was written.
    pub turn_number: u64,
    /// Structured key-info content.
    pub content: String,
    /// ISO-8601 timestamp of the write.
    pub created_at: String,
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
