# L3 API Index: skene

Crate path: `crates/theatron/skene`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/api/client.rs`

```rust
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<SecretString>,
}
```

```rust
impl ApiClient {
    pub fn new (base_url: &str, token: Option<String>) -> Result<Self>;
    pub fn token (&self) -> Option<&str>;
    pub async fn health (&self) -> Result<bool>;
    pub async fn auth_mode (&self) -> Result<AuthMode>;
    pub async fn login (&self, username: &str, password: &str) -> Result<LoginResponse>;
    pub async fn agents (&self) -> Result<Vec<Agent>>;
    pub async fn sessions (&self, nous_id: &str) -> Result<Vec<Session>>;
    pub async fn history (&self, session_id: &str) -> Result<Vec<HistoryMessage>>;
    pub async fn create_session (&self, nous_id: &str, session_key: &str) -> Result<Session>;
    pub async fn archive_session (&self, session_id: &str) -> Result<()>;
    pub async fn unarchive_session (&self, session_id: &str) -> Result<()>;
    pub async fn rename_session (&self, session_id: &str, name: &str) -> Result<()>;
    pub async fn abort_turn (&self, turn_id: &str) -> Result<()>;
    pub async fn approve_tool (&self, turn_id: &str, tool_id: &str) -> Result<()>;
    pub async fn deny_tool (&self, turn_id: &str, tool_id: &str) -> Result<()>;
    pub async fn approve_plan (&self, plan_id: &str) -> Result<()>;
    pub async fn cancel_plan (&self, plan_id: &str) -> Result<()>;
    pub async fn today_cost_cents (&self) -> Result<u32>;
    pub async fn compact (&self, session_id: &str) -> Result<()>;
    pub async fn tools (&self, nous_id: &str) -> Result<Vec<NousTool>>;
    pub async fn recall (&self, nous_id: &str, query: &str) -> Result<String>;
    pub async fn config (&self) -> Result<serde_json::Value>;
    pub async fn update_config_section (
        &self,
        section: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    pub async fn knowledge_facts (
        &self,
        sort: &str,
        order: &str,
        limit: u32,
    ) -> Result<serde_json::Value>;
    pub async fn knowledge_fact_detail (&self, fact_id: &str) -> Result<serde_json::Value>;
    pub async fn knowledge_forget (&self, fact_id: &str) -> Result<()>;
    pub async fn knowledge_restore (&self, fact_id: &str) -> Result<()>;
    pub async fn knowledge_entities (&self) -> Result<serde_json::Value>;
    pub async fn knowledge_entity_relationships (
        &self,
        entity_id: &str,
    ) -> Result<serde_json::Value>;
    pub async fn knowledge_timeline (&self) -> Result<serde_json::Value>;
    pub async fn knowledge_update_confidence (&self, fact_id: &str, confidence: f64) -> Result<()>;
    pub async fn queue_message (&self, session_id: &str, text: &str) -> Result<()>;
    pub fn raw_client (&self) -> &Client;
}
```

## `src/api/error.rs`

```rust
pub enum ApiError {
    /// HTTP transport or connection error (no response received).
    #[snafu(display("{operation}: {source}"))]
    Http {
        /// Which API call failed.
        operation: &'static str,
        /// Underlying reqwest error.
        source: reqwest::Error,
    },

    /// Non-2xx HTTP response. Message is extracted from the server body when possible.
    #[snafu(display("{operation}: {status} {message}"))]
    Server {
        /// Which API call failed.
        operation: &'static str,
        /// HTTP status code from the response.
        status: u16,
        /// Human-readable error from the server.
        message: String,
    },

    /// Credentials rejected by the gateway.
    #[snafu(display("authentication failed: token expired or invalid"))]
    Auth,

    /// Token contains characters that are not valid in an HTTP header value.
    #[snafu(display("invalid token: contains characters not valid in HTTP headers"))]
    InvalidToken,
}
```

## `src/api/sse.rs`

> Manages the global SSE connection to /api/v1/events.
> Runs in a background task, sends parsed events through a channel.
```rust
pub struct SseConnection {
    // kanon:ignore RUST/pub-visibility
    rx: mpsc::Receiver<SseEvent>,
    _handle: tokio::task::JoinHandle<()>,
}
```

```rust
impl SseConnection {
    pub fn connect (client: Client, base_url: &str) -> Self;
    pub async fn next (&mut self) -> Option<SseEvent>;
}
```

## `src/api/streaming.rs`

```rust
pub fn stream_message (
    // kanon:ignore RUST/pub-visibility
    client: Client,
    base_url: &str,
    nous_id: &str,
    session_key: &str,
    text: &str,
) -> mpsc::Receiver<StreamEvent>
```

## `src/api/types/mod.rs`

```rust
pub struct Agent {
    /// Agent identifier.
    pub id: NousId,
    /// Display name: falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
    /// Model backing this agent.
    #[serde(default)]
    pub model: Option<String>,
    /// Emoji icon for the agent.
    #[serde(default)]
    pub emoji: Option<String>,
}
```

```rust
impl Agent {
    pub fn display_name (&self) -> &str;
}
```

```rust
pub struct Session {
    /// Session identifier.
    pub id: SessionId,
    /// Agent this session belongs to.
    pub nous_id: NousId,
    /// Session key (human-readable slug, not a secret).
    #[serde(rename = "session_key")]
    pub key: String, // kanon:ignore RUST/plain-string-secret
    /// Session status (e.g. "active", "archived").
    #[serde(default)]
    pub status: Option<String>,
    /// Number of messages in the session.
    #[serde(default)]
    pub message_count: u32,
    /// Session type (e.g. "background").
    #[serde(default)]
    pub session_type: Option<String>,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// User-assigned display name.
    #[serde(default, alias = "name")]
    pub display_name: Option<String>,
}
```

```rust
impl Session {
    pub fn label (&self) -> &str;
    pub fn is_archived (&self) -> bool;
    pub fn is_interactive (&self) -> bool;
}
```

```rust
pub struct HistoryMessage {
    /// Role: "user", "assistant", or "tool".
    pub role: String,
    /// Message content (text or structured).
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    /// When the message was created.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Model that generated this message (assistant messages only).
    #[serde(default)]
    pub model: Option<String>,
    /// Tool name if this is a tool-result message.
    #[serde(default)]
    pub tool_name: Option<String>,
}
```

```rust
pub struct HistoryResponse {
    /// Messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}
```

```rust
pub struct TurnOutcome {
    /// Final text output.
    pub text: String,
    /// Agent that processed this turn.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Model used for this turn.
    pub model: String,
    /// Number of tool calls made.
    #[serde(rename = "toolCalls", default)]
    pub tool_calls: u32,
    /// Input tokens consumed.
    #[serde(rename = "inputTokens", default)]
    pub input_tokens: u32,
    /// Output tokens generated.
    #[serde(rename = "outputTokens", default)]
    pub output_tokens: u32,
    /// Tokens read from cache.
    #[serde(rename = "cacheReadTokens", default)]
    pub cache_read_tokens: u32,
    /// Tokens written to cache.
    #[serde(rename = "cacheWriteTokens", default)]
    pub cache_write_tokens: u32,
    /// Error message, if the turn errored.
    #[serde(default)]
    pub error: Option<String>,
}
```

```rust
pub struct PlanStep {
    /// Step index.
    pub id: u32,
    /// Human-readable label.
    pub label: String,
    /// Role responsible for this step.
    pub role: String,
    /// Steps that can run in parallel with this one.
    #[serde(default)]
    pub parallel: Option<Vec<u32>>,
    /// Current status of this step.
    pub status: String,
    /// Result summary after completion.
    #[serde(default)]
    pub result: Option<String>,
}
```

```rust
pub struct Plan {
    /// Plan identifier.
    pub id: PlanId,
    /// Session this plan was proposed in.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Agent that proposed the plan.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Ordered list of plan steps.
    pub steps: Vec<PlanStep>,
    /// Estimated total cost in cents.
    #[serde(rename = "totalEstimatedCostCents", default)]
    pub total_estimated_cost_cents: u32,
    /// Plan status.
    pub status: String,
}
```

```rust
pub enum SseEvent {
    /// SSE connection established.
    Connected,
    /// SSE connection lost (will auto-reconnect).
    Disconnected,
    /// Initial state dump with currently active turns.
    Init {
        /// Turns that are currently in progress.
        active_turns: Vec<ActiveTurn>,
    },
    /// A turn is about to start.
    TurnBefore {
        /// Agent processing the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
        /// Turn identifier.
        turn_id: TurnId,
    },
    /// A turn has completed.
    TurnAfter {
        /// Agent that processed the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
    },
    /// A tool was invoked during a turn.
    ToolCalled {
        /// Agent invoking the tool.
        nous_id: NousId,
        /// Name of the tool.
        tool_name: String,
    },
    /// A tool invocation failed.
    ToolFailed {
        /// Agent whose tool failed.
        nous_id: NousId,
        /// Name of the failed tool.
        tool_name: String,
        /// Error description.
        error: String,
    },
    /// Agent status changed.
    StatusUpdate {
        /// Agent whose status changed.
        nous_id: NousId,
        /// New status value.
        status: String,
    },
    /// A new session was created.
    SessionCreated {
        /// Agent the session was created for.
        nous_id: NousId,
        /// New session identifier.
        session_id: SessionId,
    },
    /// A session was archived.
    SessionArchived {
        /// Agent the session belongs to.
        nous_id: NousId,
        /// Archived session identifier.
        session_id: SessionId,
    },
    /// Memory distillation is about to start.
    DistillBefore {
        /// Agent undergoing distillation.
        nous_id: NousId,
    },
    /// Memory distillation progressed to a new stage.
    DistillStage {
        /// Agent undergoing distillation.
        nous_id: NousId,
        /// Current distillation stage.
        stage: String,
    },
    /// Memory distillation completed.
    DistillAfter {
        /// Agent that completed distillation.
        nous_id: NousId,
    },
    /// A new checkpoint was created in a planning project.
    CheckpointCreated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the created checkpoint.
        checkpoint_id: String,
    },
    /// A checkpoint's status changed (approved, skipped, overridden).
    CheckpointUpdated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the updated checkpoint.
        checkpoint_id: String,
        /// New status value (e.g. "approved", "skipped", "overridden").
        status: String,
    },
    /// Server heartbeat.
    Ping,
    /// Error event from the server.
    Error {
        /// Error message.
        message: String,
    },
}
```

```rust
pub struct ActiveTurn {
    /// Agent processing this turn.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Turn identifier.
    #[serde(rename = "turnId")]
    pub turn_id: TurnId,
}
```

```rust
pub struct AuthMode {
    /// Authentication mode (e.g. "token", "none").
    pub mode: String,
}
```

```rust
pub struct LoginResponse {
    /// Authentication token.
    pub token: SecretString,
}
```

```rust
pub struct CostSummary {
    /// Total cost across all agents.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Per-agent cost breakdown.
    #[serde(default)]
    pub agents: Vec<AgentCost>,
}
```

```rust
pub struct AgentCost {
    /// Agent identifier.
    #[serde(rename = "agentId")]
    pub agent_id: NousId,
    /// Total cost for this agent.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Number of turns processed.
    #[serde(default)]
    pub turns: u32,
}
```

```rust
pub struct DailyResponse {
    /// Daily cost entries.
    pub daily: Vec<DailyEntry>,
}
```

```rust
pub struct DailyEntry {
    /// Date string (YYYY-MM-DD).
    pub date: String,
    /// Cost in dollars.
    pub cost: f64,
    /// Total tokens consumed.
    #[serde(default)]
    pub tokens: u64,
    /// Number of turns.
    #[serde(default)]
    pub turns: u32,
}
```

```rust
pub struct AgentsResponse {
    /// Server returns `{"nous": [...]}`: accept both keys for resilience.
    #[serde(alias = "agents")]
    pub nous: Vec<Agent>,
}
```

```rust
pub struct SessionsResponse {
    /// List of sessions.
    pub sessions: Vec<Session>,
}
```

```rust
pub struct NousTool {
    /// Tool name.
    pub name: String,
    /// Whether the tool is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}
```

```rust
pub struct NousToolsResponse {
    /// List of tools.
    pub tools: Vec<NousTool>,
}
```

## `src/api/types/verification.rs`

```rust
pub enum VerificationStatus {
    /// Requirement fully demonstrated.
    Verified,
    /// Some but not all criteria demonstrated.
    PartiallyVerified,
    /// No verification evidence found.
    Unverified,
    /// Verification attempted but explicitly failed.
    Failed,
}
```

```rust
pub enum RequirementPriority {
    /// Blocking — must be verified before release.
    P0,
    /// High priority.
    P1,
    /// Medium priority.
    P2,
    /// Low or nice-to-have.
    P3,
}
```

```rust
pub struct VerificationEvidence {
    /// Human-readable label for this evidence.
    pub label: String,
    /// Path or reference to the evidence artifact.
    pub artifact: String,
}
```

```rust
pub struct VerificationGap {
    /// Description of the missing criteria.
    pub missing_criteria: String,
    /// Suggested action to close the gap.
    pub suggested_action: String,
}
```

```rust
pub struct RequirementVerification {
    /// Requirement identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Version tier (e.g., `"v1"`, `"v2"`).
    pub tier: String,
    /// Priority level.
    pub priority: RequirementPriority,
    /// Current verification status.
    pub status: VerificationStatus,
    /// Coverage percentage 0–100.
    pub coverage_pct: u8,
    /// Evidence supporting this requirement.
    pub evidence: Vec<VerificationEvidence>,
    /// Gaps remaining for this requirement.
    pub gaps: Vec<VerificationGap>,
}
```

```rust
pub struct ProjectVerificationResult {
    /// Project identifier.
    pub project_id: String,
    /// Per-requirement verification results.
    pub requirements: Vec<RequirementVerification>,
    /// ISO 8601 timestamp of the last verification run.
    pub last_verified_at: String,
}
```

## `src/discovery.rs`

```rust
pub async fn discover_server () -> Option<String>
```

## `src/events.rs`

```rust
pub enum StreamEvent {
    /// Turn started: carries session, agent, and turn identifiers.
    TurnStart {
        /// Session this turn belongs to.
        session_id: SessionId,
        /// Agent processing this turn.
        nous_id: NousId,
        /// Unique identifier for this turn.
        turn_id: TurnId,
    },
    /// Incremental text output from the model.
    TextDelta(String),
    /// Incremental extended-thinking output from the model.
    ThinkingDelta(String),
    /// A tool invocation has started.
    ToolStart {
        /// Name of the tool being invoked.
        tool_name: String,
        /// Unique identifier for this tool call.
        tool_id: ToolId,
        /// Tool input parameters, if available.
        input: Option<serde_json::Value>,
    },
    /// A tool invocation has completed.
    ToolResult {
        /// Name of the tool that completed.
        tool_name: String,
        /// Unique identifier for this tool call.
        tool_id: ToolId,
        /// Whether the tool returned an error.
        is_error: bool,
        /// Wall-clock duration of the tool call in milliseconds.
        duration_ms: u64,
        /// Tool output text, if available.
        result: Option<String>,
    },
    /// The server is waiting for user approval of a tool call.
    ToolApprovalRequired {
        /// Turn that owns this tool call.
        turn_id: TurnId,
        /// Name of the tool awaiting approval.
        tool_name: String,
        /// Unique identifier for this tool call.
        tool_id: ToolId,
        /// Tool input parameters.
        input: serde_json::Value,
        /// Risk level assigned by the server.
        risk: String,
        /// Human-readable reason for requiring approval.
        reason: String,
    },
    /// A tool approval decision has been resolved.
    ToolApprovalResolved {
        /// Tool call that was resolved.
        tool_id: ToolId,
        /// Decision: "approved" or "denied".
        decision: String,
    },
    /// The server has proposed a multi-step plan for approval.
    PlanProposed {
        /// The proposed plan.
        plan: Plan,
    },
    /// A plan step has started executing.
    PlanStepStart {
        /// Plan this step belongs to.
        plan_id: PlanId,
        /// Step index within the plan.
        step_id: u32,
    },
    /// A plan step has completed.
    PlanStepComplete {
        /// Plan this step belongs to.
        plan_id: PlanId,
        /// Step index within the plan.
        step_id: u32,
        /// Completion status of the step.
        status: String,
    },
    /// The entire plan has completed.
    PlanComplete {
        /// Plan that completed.
        plan_id: PlanId,
        /// Overall completion status.
        status: String,
    },
    /// The turn has completed successfully.
    TurnComplete {
        /// Summary of the completed turn.
        outcome: TurnOutcome,
    },
    /// The turn was aborted (by user or server).
    TurnAbort {
        /// Reason the turn was aborted.
        reason: String,
    },
    /// An error occurred during streaming.
    Error(String),
}
```

## `src/id.rs`

```rust
pub struct TurnId(CompactString);
```

```rust
pub struct PlanId(String);
```

## `src/sse.rs`

```rust
pub struct SseEvent {
    /// The `event:` field. Defaults to `"message"` per the SSE spec.
    pub event: String,
    /// The `data:` field(s), concatenated with newlines for multi-line data.
    pub data: String,
    /// The `id:` field, if present.
    pub id: Option<String>,
    /// The `retry:` field in milliseconds, if present.
    pub retry: Option<u64>,
}
```

> Transforms a byte stream into a stream of parsed SSE events.
> 
> Handles the full SSE wire protocol: `data:`, `event:`, `id:`, `retry:`,
> comment lines (`:` prefix), multi-line `data:` fields (concatenated with
> newlines), and blank-line event delimiters.
```rust
pub struct SseStream<S> {
    stream: S,
    buf: String,
    done: bool,

    current_event: Option<String>,
    current_data: String,
    current_id: Option<String>,
    current_retry: Option<u64>,
    has_data: bool,
}
```

```rust
impl <S, E> SseStream<S> where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display, {
    pub fn new (stream: S) -> Self;
}
```
