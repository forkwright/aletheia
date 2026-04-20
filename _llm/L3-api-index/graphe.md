# L3 API Index: graphe

Crate path: `crates/graphe`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// Session not found.
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session creation failed.
    #[snafu(display("failed to create session for nous {nous_id}"))]
    SessionCreate {
        nous_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage backend error (fjall LSM-tree).
    #[snafu(display("storage error: {message}"))]
    Storage {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization error within stored data.
    #[snafu(display("stored data JSON error: {source}"))]
    StoredJson {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Filesystem I/O error (archive, backup, or store open).
    #[snafu(display("I/O error at {}: {source}", path.display()))]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Engine initialization failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("engine initialization failed: {message}"))]
    EngineInit {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Engine query failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("engine query failed: {message}"))]
    EngineQuery {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Query exceeded the configured timeout duration.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("query timed out after {secs:.1}s"))]
    QueryTimeout {
        secs: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Schema version mismatch.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("schema version mismatch: expected {expected}, found {found}"))]
    SchemaVersion {
        expected: i64,
        found: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Spawned blocking task failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("spawned task failed: {source}"))]
    Join {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// `DataValue` type conversion failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("DataValue conversion failed: {message}"))]
    Conversion {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact content was empty.
    #[snafu(display("fact content must not be empty"))]
    EmptyContent {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact content exceeded maximum length.
    #[snafu(display("fact content too long: {actual} bytes (max {max})"))]
    ContentTooLong {
        max: usize,
        actual: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Confidence score was outside the valid [0.0, 1.0] range.
    #[snafu(display("confidence must be in [0.0, 1.0], got {value}"))]
    InvalidConfidence {
        value: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact rejected by admission control policy.
    #[snafu(display("admission rejected: {reason}"))]
    AdmissionRejected {
        /// Human-readable reason from the admission policy.
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A timestamp string could not be parsed.
    #[snafu(display("invalid timestamp: {source}"))]
    InvalidTimestamp {
        source: jiff::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Entity name was empty.
    #[snafu(display("entity name must not be empty"))]
    EmptyEntityName {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Relationship weight was outside the valid [0.0, 1.0] range.
    #[snafu(display("relationship weight must be in [0.0, 1.0], got {value}"))]
    InvalidWeight {
        value: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding vector was empty.
    #[snafu(display("embedding vector must not be empty"))]
    EmptyEmbedding {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding content was empty.
    #[snafu(display("embedding content must not be empty"))]
    EmptyEmbeddingContent {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Attempted to operate on a fact that does not exist.
    #[snafu(display("fact not found: {id}"))]
    FactNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding vector dimension does not match the store's configured dimension.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("embedding dimension mismatch: expected {expected}, got {actual}"))]
    EmbeddingDimensionMismatch {
        expected: usize,
        actual: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Knowledge-domain identifier validation failed.
    #[snafu(display("invalid identifier: {source}"))]
    InvalidId {
        source: eidos::id::IdValidationError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
impl Error {
    pub fn is_unique_constraint_violation (&self) -> bool;
}
```

> Result alias using graphe's [`Error`] type.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/portability.rs`

```rust
pub struct AgentFile {
    pub version: u32,
    pub exported_at: String,
    pub generator: String,
    pub nous: NousInfo,
    pub workspace: WorkspaceData,
    pub sessions: Vec<ExportedSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryData>,
    /// Knowledge graph export (facts, entities, relationships).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<KnowledgeExport>,
}
```

```rust
pub struct NousInfo {
    pub id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub config: serde_json::Value,
}
```

```rust
pub struct WorkspaceData {
    pub files: HashMap<String, String>,
    pub binary_files: Vec<String>,
}
```

```rust
pub struct ExportedSession {
    pub id: String,
    pub session_key: String,
    pub status: String,
    pub session_type: String,
    pub message_count: i64,
    pub token_count_estimate: i64,
    pub distillation_count: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_state: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distillation_priming: Option<serde_json::Value>,
    pub notes: Vec<ExportedNote>,
    pub messages: Vec<ExportedMessage>,
}
```

```rust
pub struct ExportedMessage {
    pub role: String,
    pub content: String,
    pub seq: i64,
    pub token_estimate: i64,
    pub is_distilled: bool,
    pub created_at: String,
}
```

```rust
pub struct ExportedNote {
    pub category: String,
    pub content: String,
    pub created_at: String,
}
```

```rust
pub struct MemoryData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<Vec<ExportedVector>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphData>,
}
```

```rust
pub struct ExportedVector {
    pub id: String,
    pub text: String,
    pub metadata: serde_json::Value,
}
```

```rust
pub struct GraphData {
    pub nodes: Vec<serde_json::Value>,
    pub edges: Vec<serde_json::Value>,
}
```

```rust
pub struct KnowledgeExport {
    /// All facts from the knowledge graph.
    pub facts: Vec<crate::knowledge::Fact>,
    /// All entities from the knowledge graph.
    pub entities: Vec<crate::knowledge::Entity>,
    /// All relationships between entities.
    pub relationships: Vec<crate::knowledge::Relationship>,
}
```

## `src/store/fjall_store.rs`

> Fjall-backed session store.
> 
> Open with [`SessionStore::open`] for persistent storage or
> [`SessionStore::open_in_memory`] for ephemeral storage (test-only; uses a
> `TempDir` that is cleaned up on drop).
```rust
pub struct SessionStore {
    db: Arc<SingleWriterTxDatabase>,
    /// Shared write mutex — see [`koina::fjall::FjallDb::write_lock`].
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    _temp_dir: Option<tempfile::TempDir>,
}
```

```rust
impl SessionStore {
    pub fn open (path: &Path) -> Result<Self>;
    pub fn open_in_memory () -> Result<Self>;
    pub fn ping (&self) -> Result<()>;
    pub fn find_session (&self, nous_id: &str, session_key: &str) -> Result<Option<Session>>;
    pub fn find_session_by_id (&self, id: &str) -> Result<Option<Session>>;
    pub fn create_session (
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        parent_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<Session>;
    pub fn find_or_create_session (
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        model: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<Session>;
    pub fn list_sessions (&self, nous_id: Option<&str>) -> Result<Vec<Session>>;
    pub fn update_session_status (&self, id: &str, status: SessionStatus) -> Result<()>;
    pub fn update_display_name (&self, id: &str, display_name: &str) -> Result<()>;
    pub fn delete_session (&self, id: &str) -> Result<bool>;
    pub fn append_message (
        &self,
        session_id: &str,
        role: Role,
        content: &str,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        token_estimate: i64,
    ) -> Result<i64>;
    pub fn get_history (&self, session_id: &str, limit: Option<i64>) -> Result<Vec<Message>>;
    pub fn get_history_filtered (
        &self,
        session_id: &str,
        limit: Option<i64>,
        before_seq: Option<i64>,
    ) -> Result<Vec<Message>>;
    pub fn get_history_with_budget (
        &self,
        session_id: &str,
        max_tokens: i64,
    ) -> Result<Vec<Message>>;
    pub fn get_distillation_summary (&self, session_id: &str) -> Result<Option<String>>;
    pub fn mark_messages_distilled (&self, session_id: &str, seqs: &[i64]) -> Result<()>;
    pub fn insert_distillation_summary (&self, session_id: &str, content: &str) -> Result<()>;
    pub fn record_distillation (
        &self,
        session_id: &str,
        messages_before: i64,
        messages_after: i64,
        tokens_before: i64,
        tokens_after: i64,
        model: Option<&str>,
    ) -> Result<()>;
    pub fn usage_exists_for_turn (&self, session_id: &str, turn_seq: i64) -> Result<bool>;
    pub fn record_usage (&self, record: &UsageRecord) -> Result<()>;
    pub fn add_note (
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> Result<i64>;
    pub fn get_notes (&self, session_id: &str) -> Result<Vec<AgentNote>>;
    pub fn delete_note (&self, note_id: i64) -> Result<bool>;
    pub fn blackboard_write (
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_secs: i64,
    ) -> Result<()>;
    pub fn blackboard_read (&self, key: &str) -> Result<Option<BlackboardRow>>;
    pub fn blackboard_list (&self) -> Result<Vec<BlackboardRow>>;
    pub fn blackboard_delete (&self, key: &str, author: &str) -> Result<bool>;
}
```

## `src/types.rs`

```rust
pub enum SessionStatus {
    /// Session is live and accepting new messages.
    Active,
    /// Session has been closed and is retained for history.
    Archived,
    /// Session has been distilled into a summary and may be pruned.
    Distilled,
}
```

```rust
impl SessionStatus {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub enum SessionType {
    /// Long-lived conversational session (the default).
    Primary,
    /// Background task session (e.g. prosoche attention loops).
    Background,
    /// Short-lived session for one-shot tasks (`ask:`, `spawn:`, `dispatch:`).
    Ephemeral,
}
```

```rust
pub enum Role {
    /// System-injected context (bootstrap, instructions).
    System,
    /// Human operator input.
    User,
    /// LLM-generated response.
    Assistant,
    /// Output returned from a tool invocation.
    ToolResult,
}
```

```rust
impl Role {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub struct SessionMetrics {
    /// Approximate total tokens consumed across all messages.
    pub token_count_estimate: i64,
    /// Number of messages in this session.
    pub message_count: i64,
    /// Token count from the most recent input.
    pub last_input_tokens: i64,
    /// Hash of the bootstrap payload to detect config changes.
    pub bootstrap_hash: Option<String>,
    /// Number of times this session has been distilled.
    pub distillation_count: i64,
    /// ISO 8601 timestamp of the last distillation, if any.
    pub last_distilled_at: Option<String>,
    /// Estimated context window token usage.
    pub computed_context_tokens: i64,
}
```

```rust
pub struct SessionOrigin {
    /// Parent session for sub-task lineage tracking.
    pub parent_session_id: Option<String>,
    /// External thread identifier (e.g. Signal group thread).
    pub thread_id: Option<String>,
    /// Transport layer that originated this session.
    pub transport: Option<String>,
    /// Human-readable display name set by the user.
    pub display_name: Option<String>,
}
```

```rust
pub struct Session {
    /// Unique session identifier (UUID v4).
    pub id: String,
    /// Owning agent identifier.
    pub nous_id: String,
    /// Logical key used to look up or resume this session.
    pub session_key: String,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// LLM model used for this session's turns.
    pub model: Option<String>,
    /// Classification of the session's lifecycle behavior.
    pub session_type: SessionType,
    /// ISO 8601 timestamp when the session was created.
    pub created_at: String,
    /// ISO 8601 timestamp of the last update.
    pub updated_at: String,
    /// Token and message count metrics.
    #[serde(flatten)]
    pub metrics: SessionMetrics,
    /// External origin and identity metadata.
    #[serde(flatten)]
    pub origin: SessionOrigin,
}
```

```rust
pub struct Message {
    /// Database-assigned row identifier.
    pub id: i64,
    /// Session this message belongs to.
    pub session_id: String,
    /// Sequence number within the session (monotonically increasing).
    pub seq: i64,
    /// Author role (system, user, assistant, or `tool_result`).
    pub role: Role,
    /// Message body text.
    pub content: String,
    /// Tool call identifier if this message is a tool result.
    pub tool_call_id: Option<String>,
    /// Tool name if this message is a tool result.
    pub tool_name: Option<String>,
    /// Estimated token count for this message.
    pub token_estimate: i64,
    /// Whether this message was produced by distillation.
    pub is_distilled: bool,
    /// ISO 8601 timestamp when the message was created.
    pub created_at: String,
}
```

```rust
pub struct UsageRecord {
    /// Session this usage belongs to.
    pub session_id: String,
    /// Turn sequence number within the session.
    pub turn_seq: i64,
    /// Tokens consumed from the input (prompt).
    pub input_tokens: i64,
    /// Tokens generated in the output (completion).
    pub output_tokens: i64,
    /// Tokens read from prompt cache.
    pub cache_read_tokens: i64,
    /// Tokens written to prompt cache.
    pub cache_write_tokens: i64,
    /// Model used for this turn, if known.
    pub model: Option<String>,
}
```

```rust
pub struct BlackboardRow {
    pub key: String,
    pub value: String,
    pub author_nous_id: String,
    pub ttl_seconds: i64,
    pub created_at: String,
    pub expires_at: Option<String>,
}
```

```rust
pub struct AgentNote {
    /// Database-assigned row identifier.
    pub id: i64,
    /// Session this note is attached to.
    pub session_id: String,
    /// Agent that wrote the note.
    pub nous_id: String,
    /// Freeform category tag for filtering (e.g. "insight", "task").
    pub category: String,
    /// Note body text.
    pub content: String,
    /// ISO 8601 timestamp when the note was created.
    pub created_at: String,
}
```
