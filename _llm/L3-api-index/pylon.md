# L3 API Index: pylon

Crate path: `crates/pylon`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub struct ErrorResponse {
    /// The error body.
    pub error: ErrorBody,
}
```

```rust
pub struct ErrorBody {
    /// Machine-readable error code (e.g. `"session_not_found"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Per-request correlation ID for tracing errors across logs and client reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Optional structured details (e.g. retry timing, validation errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
```

```rust
pub struct FieldError {
    /// Request body or query parameter field name (e.g. `"nous_id"`).
    pub field: String,
    /// Stable machine-readable error code (e.g. `"required"`, `"range"`,
    /// `"format"`, `"too_long"`).
    pub code: String,
    /// Human-readable description of the error.
    pub message: String,
}
```

```rust
pub enum ApiError {
    /// Requested session does not exist (404).
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Requested nous agent does not exist (404).
    #[snafu(display("nous not found: {id}"))]
    NousNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Client sent an invalid request (400).
    #[snafu(display("bad request: {message}"))]
    BadRequest {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unrecoverable server-side failure (500).
    #[snafu(display("internal error: {message}"))]
    Internal {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Missing or invalid authentication credentials (401).
    #[snafu(display("unauthorized"))]
    Unauthorized {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("not found: {path}"))]
    NotFound {
        path: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("rate limited, retry after {retry_after_secs}s"))]
    RateLimited {
        retry_after_secs: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("forbidden: {message}"))]
    Forbidden {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("service unavailable: {message}"))]
    ServiceUnavailable {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Idempotency conflict: a request with this key is already in flight (409).
    #[snafu(display("conflict: {message}"))]
    Conflict {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Validation failed with field-level errors (422).
    #[snafu(display("validation failed"))]
    ValidationFailed {
        errors: Vec<FieldError>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Feature not yet implemented (501).
    #[snafu(display("not implemented: {message}"))]
    NotImplemented {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Client-visible error already classified by a lower-layer presentation contract.
    #[snafu(display("user-facing error ({status} {code}): {message}"))]
    UserFacing {
        status: StatusCode,
        code: String,
        message: String,
        retry_after_secs: Option<u64>,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/event_bus.rs`

```rust
pub struct DomainEvent {
    /// Event topic (e.g. `fact.created`, `turn.complete`).
    pub topic: String,
    /// Structured event payload.
    pub payload: serde_json::Value,
    /// ISO-8601 timestamp of emission.
    pub at: String,
}
```

```rust
impl DomainEvent {
    pub fn new (topic: impl Into<String>, payload: serde_json::Value) -> Self;
}
```

> In-process broadcast bus for domain events.
> 
> Holds a [`tokio::sync::broadcast::Sender`] and provides typed publish /
> subscribe methods.  The channel capacity is fixed at creation time;
> slow subscribers lag behind and are dropped gracefully.
```rust
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<DomainEvent>,
}
```

```rust
impl EventBus {
    pub fn new (capacity: usize) -> Self;
    pub fn publish (&self, event: DomainEvent);
    pub fn subscribe (&self) -> tokio::sync::broadcast::Receiver<DomainEvent>;
}
```

## `src/extract.rs`

```rust
pub struct Claims {
    /// Subject identifier (user or service principal).
    pub sub: String,
    /// Authorization role governing API access.
    pub role: Role,
    /// Optional nous scope: when set, restricts access to a single agent.
    pub nous_id: Option<String>,
}
```

## `src/handlers/config.rs`

```rust
pub struct ConfigUpdateResponse {
    /// Name of the config section that was updated.
    pub section: String,
    /// The updated config section value.
    pub config: Value,
    /// Field paths that require a restart to take effect.
    pub restart_required: Vec<String>,
}
```

```rust
pub struct ConfigReloadResponse {
    /// Number of hot-reloadable values that were updated.
    pub hot_reloaded: usize,
    /// Field paths that changed but require a restart to take effect.
    pub restart_required: Vec<String>,
    /// All changed field paths (both hot and cold).
    pub changed: Vec<String>,
}
```

```rust
pub struct AgentsConfig {
        #[schema(value_type = Object)]
        pub defaults: Option<Value>,
        pub list: Option<Vec<Value>>,
    }
```

```rust
pub struct GatewayConfig {
        pub port: Option<u16>,
        pub bind: Option<String>,
        #[schema(value_type = Object)]
        pub auth: Option<Value>,
        #[schema(value_type = Object)]
        pub tls: Option<Value>,
        #[schema(value_type = Object)]
        pub cors: Option<Value>,
        #[schema(value_type = Object)]
        pub body_limit: Option<Value>,
        #[schema(value_type = Object)]
        pub csrf: Option<Value>,
        #[schema(value_type = Object)]
        pub rate_limit: Option<Value>,
    }
```

```rust
pub struct ChannelsConfig {
        #[schema(value_type = Object)]
        pub signal: Option<Value>,
        #[schema(value_type = Object)]
        pub matrix: Option<Value>,
    }
```

```rust
pub struct ChannelBinding {
        pub channel: Option<String>,
        pub source: Option<String>,
        pub nous_id: Option<String>,
        pub session_key: Option<String>,
    }
```

```rust
pub struct EmbeddingSettings {
        pub provider: Option<String>,
        pub model: Option<String>,
        pub dimension: Option<usize>,
    }
```

```rust
pub struct DataConfig {
        #[schema(value_type = Object)]
        pub retention: Option<Value>,
    }
```

```rust
pub struct MaintenanceConfig {
        #[schema(value_type = Object)]
        pub trace_rotation: Option<Value>,
        #[schema(value_type = Object)]
        pub drift_detection: Option<Value>,
        #[schema(value_type = Object)]
        pub db_monitoring: Option<Value>,
        #[schema(value_type = Object)]
        pub disk_space: Option<Value>,
        #[schema(value_type = Object)]
        pub retention: Option<Value>,
        pub knowledge_maintenance_enabled: Option<bool>,
        #[schema(value_type = Object)]
        pub watchdog: Option<Value>,
        #[schema(value_type = Object)]
        pub cron_tasks: Option<Value>,
        #[schema(value_type = Object)]
        pub backup: Option<Value>,
    }
```

```rust
pub struct ModelPricing {
        pub input_cost_per_mtok: Option<f64>,
        pub output_cost_per_mtok: Option<f64>,
    }
```

```rust
pub enum ConfigSectionPayload {
        Agents(Value),
        Gateway(Value),
        Channels(Value),
        Bindings(Value),
        Embedding(Value),
        Data(Value),
        Packs(Value),
        Maintenance(Value),
        Pricing(Value),
    }
```

```rust
pub async fn get_config (
    State(state): State<ConfigState>,
    _claims: Claims,
) -> Result<Json<Value>, ApiError>
```

```rust
pub async fn get_section (
    State(state): State<ConfigState>,
    _claims: Claims,
    Path(section): Path<String>,
) -> Result<Json<Value>, ApiError>
```

```rust
pub async fn reload_config (
    State(state): State<ConfigState>,
    claims: Claims,
) -> Result<impl IntoResponse, ApiError>
```

```rust
pub async fn update_section (
    State(state): State<ConfigState>,
    claims: Claims,
    Path(section): Path<String>,
    body: ConfigSectionPayload,
) -> Result<impl IntoResponse, ApiError>
```

## `src/handlers/events.rs`

```rust
pub struct SubscribeParams {
    /// Comma-separated list of topics to subscribe to (e.g. `fact.created,turn.complete`).
    pub topics: String,
}
```

```rust
pub async fn subscribe (
    State(state): State<EventBusState>,
    _claims: Claims,
    Query(params): Query<SubscribeParams>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError>
```

```rust
pub async fn discovery (_claims: Claims) -> impl IntoResponse
```

## `src/handlers/health.rs`

```rust
pub async fn check (State(state): State<HealthState>) -> impl IntoResponse
```

```rust
pub async fn deprecated_health_check (State(state): State<HealthState>) -> impl IntoResponse
```

```rust
pub struct HealthResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Crate version from `Cargo.toml`.
    #[schema(value_type = String)]
    pub version: &'static str,
    /// Build git SHA when available from the build environment.
    #[schema(value_type = String)]
    pub git_sha: &'static str,
    /// Seconds since server start.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
    /// Absolute path to the instance data directory.
    pub data_dir: String,
}
```

```rust
pub struct HealthCheck {
    /// Subsystem name (e.g. `"session_store"`, `"providers"`).
    #[schema(value_type = String)]
    pub name: &'static str,
    /// Check outcome: `"pass"`, `"warn"`, `"fail"`, or `"timeout"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Diagnostic message when status is not `"pass"`.
    pub message: Option<String>,
}
```

## `src/handlers/insights.rs`

```rust
pub async fn get_agent_perf (
    State(state): State<InsightsState>,
) -> Json<AgentPerformanceListResponse>
```

```rust
pub async fn get_agent_perf_one (
    State(state): State<InsightsState>,
    Path(id): Path<String>,
) -> Result<Json<AgentPerformance>, ApiError>
```

```rust
pub async fn get_quality_metrics (
    State(state): State<InsightsState>,
) -> Json<QualityMetricsResponse>
```

```rust
pub async fn get_token_metrics (
    State(state): State<InsightsState>,
    Query(query): Query<MetricsQuery>,
) -> Json<TokenMetricsResponse>
```

```rust
pub async fn get_cost_metrics (
    State(state): State<InsightsState>,
    Query(query): Query<MetricsQuery>,
) -> Json<CostMetricsResponse>
```

```rust
pub async fn get_journal (Query(query): Query<JournalQuery>) -> Json<Vec<JournalEvent>>
```

## `src/handlers/knowledge/bulk_import.rs`

```rust
pub struct BulkImportRequest {
    pub facts: Vec<mneme::knowledge::Fact>,
}
```

```rust
pub struct BulkImportResponse {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<ImportFactError>,
}
```

```rust
pub struct ImportFactError {
    pub index: usize,
    pub id: String,
    pub message: String,
}
```

```rust
pub async fn import_facts (
    State(state): State<KnowledgeState>,
    claims: Claims,
    body: axum::body::Bytes,
) -> Result<Json<BulkImportResponse>, ApiError>
```

## `src/handlers/knowledge/ingest.rs`

```rust
pub struct IngestRequest {
    /// Raw content to ingest.
    pub content: String,
    /// Format: markdown, text, json, jsonl.
    #[serde(default)]
    pub format: String,
    /// Nous agent ID that will own the extracted facts.
    pub nous_id: String,
}
```

```rust
pub struct IngestFactError {
    /// Index of the fact in the batch.
    pub index: usize,
    /// Fact ID if available.
    pub id: Option<String>,
    /// Error message.
    pub message: String,
}
```

```rust
pub struct IngestResponse {
    /// Number of facts successfully inserted.
    pub inserted: usize,
    /// Number of facts skipped due to errors.
    pub skipped: usize,
    /// Per-fact error details.
    pub errors: Vec<IngestFactError>,
}
```

```rust
pub async fn ingest (
    State(state): State<KnowledgeState>,
    claims: Claims,
    Json(body): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError>
```

## `src/handlers/knowledge/mod.rs`

```rust
pub struct FactsQuery {
    /// Filter by nous agent ID.
    #[serde(default)]
    pub nous_id: Option<String>,
    /// Sort field: confidence, recency, created, `access_count`, `fsrs_review`.
    #[serde(default = "default_sort")]
    pub sort: String,
    /// Sort direction: asc or desc.
    #[serde(default = "default_order")]
    pub order: String,
    /// Text filter.
    #[serde(default)]
    pub filter: Option<String>,
    /// Fact type filter (knowledge, preference, skill, observation, etc.).
    #[serde(default)]
    pub fact_type: Option<String>,
    /// Epistemic tier filter (verified, inferred, assumed).
    #[serde(default)]
    pub tier: Option<String>,
    /// Maximum results to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
    /// Include forgotten facts.
    #[serde(default)]
    pub include_forgotten: bool,
}
```

```rust
pub struct FactsResponse {
    pub facts: Vec<mneme::knowledge::Fact>,
    pub total: usize,
}
```

```rust
pub struct EntitiesQuery {
    /// Maximum results to return (default: 100, max: 1000).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
}
```

```rust
pub struct EntitiesResponse {
    pub entities: Vec<mneme::knowledge::Entity>,
    pub total: usize,
}
```

```rust
pub struct RelationshipsResponse {
    pub relationships: Vec<mneme::knowledge::Relationship>,
}
```

```rust
pub struct ForgetRequest {
    #[serde(default = "default_forget_reason")]
    pub reason: String,
}
```

```rust
pub struct UpdateConfidenceRequest {
    pub confidence: f64,
}
```

```rust
pub struct UpdateSensitivityRequest {
    pub sensitivity: String,
}
```

```rust
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub nous_id: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}
```

```rust
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub confidence: f64,
    pub tier: String,
    pub fact_type: String,
    pub score: f64,
}
```

```rust
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
}
```

```rust
pub struct SimilarFact {
    pub id: String,
    pub content: String,
    pub similarity: f64,
}
```

```rust
pub struct FactDetailResponse {
    pub fact: mneme::knowledge::Fact,
    pub relationships: Vec<mneme::knowledge::Relationship>,
    pub similar: Vec<SimilarFact>,
}
```

```rust
pub struct TimelineEvent {
    pub timestamp: String,
    pub event_type: String,
    pub description: String,
    pub fact_id: String,
    pub confidence: Option<f64>,
}
```

```rust
pub struct TimelineQuery {
    /// Filter by nous agent ID.
    #[serde(default)]
    pub nous_id: Option<String>,
    /// Maximum events to return (default: 100, max: 1000).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
}
```

```rust
pub struct TimelineResponse {
    pub events: Vec<TimelineEvent>,
    pub total: usize,
}
```

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn list_facts (
    State(state): State<KnowledgeState>,
    Query(mut query): Query<FactsQuery>,
) -> Result<Json<FactsResponse>, ApiError>
```

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn get_fact (
    #[cfg_attr(
        not(feature = "knowledge-store"),
        expect(
            unused_variables,
            reason = "state only used when knowledge-store feature is enabled"
        )
    )]
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
) -> Result<Json<FactDetailResponse>, ApiError>
```

```rust
pub async fn list_entities (
    State(state): State<KnowledgeState>,
    Query(mut query): Query<EntitiesQuery>,
) -> Result<Json<EntitiesResponse>, ApiError>
```

```rust
pub async fn entity_relationships (
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
) -> Result<Json<RelationshipsResponse>, ApiError>
```

```rust
pub struct GraphCheckReport {
    /// Total number of facts stored.
    pub fact_count: usize,
    /// Total number of entities stored.
    pub entity_count: usize,
    /// Total number of relationships stored.
    pub relationship_count: usize,
    /// Entities with no facts or relationships (potential orphans).
    pub orphaned_entity_count: usize,
    /// Edges that reference missing endpoint entities.
    pub dangling_edge_count: usize,
    /// Overall health: `"healthy"` or `"issues_found"`.
    pub status: &'static str,
}
```

```rust
pub async fn check_graph_health (
    State(state): State<KnowledgeState>,
) -> impl axum::response::IntoResponse
```

## `src/handlers/knowledge/mutation.rs`

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn forget_fact (
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<ForgetRequest>,
) -> Result<StatusCode, ApiError>
```

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn restore_fact (
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError>
```

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn update_confidence (
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<UpdateConfidenceRequest>,
) -> Result<Json<serde_json::Value>, ApiError>
```

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn update_sensitivity (
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<UpdateSensitivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError>
```

## `src/handlers/knowledge/search.rs`

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn search (
    State(state): State<KnowledgeState>,
    Query(mut query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError>
```

> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn timeline (
    State(state): State<KnowledgeState>,
    Query(mut query): Query<TimelineQuery>,
) -> Result<Json<TimelineResponse>, ApiError>
```

## `src/handlers/knowledge/webhook.rs`

```rust
pub struct WebhookIngestRequest {
    /// Nous agent ID that will own the facts.
    pub nous_id: String,
    /// Facts to insert.
    pub facts: Vec<mneme::knowledge::Fact>,
    /// Optional source identifier for provenance.
    #[serde(default)]
    pub source: Option<String>,
}
```

```rust
pub struct WebhookIngestResponse {
    /// Number of facts successfully inserted.
    pub inserted: usize,
    /// Number of facts skipped due to errors.
    pub skipped: usize,
    /// Per-fact error details.
    pub errors: Vec<crate::handlers::knowledge::ingest::IngestFactError>,
}
```

```rust
pub async fn webhook_ingest (
    State(state): State<KnowledgeState>,
    claims: Claims,
    Json(body): Json<WebhookIngestRequest>,
) -> Result<Json<WebhookIngestResponse>, ApiError>
```

## `src/handlers/metrics.rs`

```rust
pub async fn expose (State(state): State<MetricsState>) -> impl IntoResponse
```

## `src/handlers/nous.rs`

```rust
pub async fn list (State(state): State<NousState>, claims: Claims) -> Json<NousListResponse>
```

```rust
pub async fn get_status (
    State(state): State<NousState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<NousStatus>, ApiError>
```

```rust
pub async fn tools (
    State(state): State<NousState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<ToolsResponse>, ApiError>
```

```rust
pub async fn recover (
    State(state): State<NousState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<RecoverResponse>, ApiError>
```

```rust
pub struct AgentDefinition {
    /// Agent identifier (alphanumeric and hyphens only).
    pub id: String,
    /// Human-readable display name. Falls back to a capitalized `id`.
    #[serde(default)]
    pub name: Option<String>,
    /// LLM model identifier. Falls back to the workspace default.
    #[serde(default)]
    pub model: Option<String>,
}
```

```rust
pub struct CreateAgentResponse {
    /// Agent identifier.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// LLM model assigned to this agent.
    pub model: String,
    /// Whether the agent requires a server restart to become active.
    pub restart_required: bool,
}
```

```rust
pub async fn create (
    State(state): State<NousState>,
    claims: Claims,
    Json(body): Json<AgentDefinition>,
) -> Result<impl IntoResponse, ApiError>
```

```rust
pub struct RecoverResponse {
    /// Agent identifier.
    pub id: String,
    /// Whether recovery was performed (false if agent was not degraded).
    pub recovered: bool,
}
```

```rust
pub struct NousListResponse {
    /// Agent summaries.
    pub nous: Vec<NousSummary>,
}
```

```rust
pub struct NousSummary {
    /// Agent identifier.
    pub id: String,
    /// Human-readable display name (falls back to `id`).
    pub name: String,
    /// LLM model assigned to this agent.
    pub model: String,
    /// Lifecycle status (e.g. `"active"`).
    pub status: String,
}
```

```rust
pub struct NousStatus {
    /// Agent identifier.
    pub id: String,
    /// LLM model assigned to this agent.
    pub model: String,
    /// Maximum context window in tokens.
    pub context_window: u32,
    /// Maximum output tokens per turn.
    pub max_output_tokens: u32,
    /// Whether extended thinking is enabled.
    pub thinking_enabled: bool,
    /// Token budget for extended thinking.
    pub thinking_budget: u32,
    /// Maximum tool iterations per turn.
    pub max_tool_iterations: u32,
    /// Actor lifecycle status.
    pub status: String,
}
```

```rust
pub struct ToolsResponse {
    /// Tool summaries.
    pub tools: Vec<ToolSummary>,
}
```

```rust
pub struct ToolSummary {
    /// Tool name as sent to the LLM.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Tool category (e.g. `"Builtin"`, `"Pack"`).
    pub category: String,
    /// Whether the tool activates automatically without explicit configuration.
    ///
    /// When `false` the tool is lazy and must be activated via `enable_tool`
    /// before the agent can use it.
    pub auto_activate: bool,
}
```

## `src/handlers/sessions/mod.rs`

```rust
pub async fn create (
    State(state): State<SessionsState>,
    claims: Claims,
    Json(body): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, ApiError>
```

```rust
pub async fn list_sessions (
    State(state): State<SessionsState>,
    _claims: Claims,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<ListSessionsResponse>, ApiError>
```

```rust
pub async fn get_session (
    State(state): State<SessionsState>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, ApiError>
```

```rust
pub async fn close (
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError>
```

```rust
pub async fn purge (
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError>
```

```rust
pub async fn archive (
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError>
```

```rust
pub async fn unarchive (
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError>
```

```rust
pub async fn rename (
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<RenameSessionRequest>,
) -> Result<StatusCode, ApiError>
```

```rust
pub async fn history (
    State(state): State<SessionsState>,
    _claims: Claims,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiError>
```

## `src/handlers/sessions/streaming.rs`

```rust
pub async fn send_message (
    State(state): State<SessionsState>,
    claims: Claims,
    headers: axum::http::HeaderMap,
    axum::extract::Extension(request_id): axum::extract::Extension<RequestId>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError>
```

```rust
pub async fn stream_turn (
    State(state): State<SessionsState>,
    claims: Claims,
    axum::extract::Extension(request_id): axum::extract::Extension<RequestId>,
    Json(body): Json<StreamTurnRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError>
```

```rust
pub async fn events (
    _claims: Claims,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>
```

```rust
pub async fn reconnect_turn (
    State(state): State<SessionsState>,
    _claims: Claims,
    headers: axum::http::HeaderMap,
    axum::extract::Path((session_id, turn_id)): axum::extract::Path<(String, String)>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError>
```

## `src/handlers/sessions/types.rs`

```rust
pub struct CreateSessionRequest {
    /// Target nous agent to bind the session to.
    pub nous_id: String,
    /// Client-chosen key for session deduplication.
    pub session_key: String,
}
```

```rust
pub struct RenameSessionRequest {
    /// New display name for the session.
    pub name: String,
}
```

```rust
pub struct SendMessageRequest {
    /// User message text.
    pub content: String,
}
```

```rust
pub struct StreamTurnRequest {
    /// Target nous agent ID.
    pub nous_id: String,
    /// User message text.
    pub message: String,
    /// Session key for deduplication (defaults to "main").
    #[serde(default = "default_session_key")]
    pub session_key: String,
}
```

```rust
pub struct ListSessionsParams {
    /// Filter sessions by agent ID.
    pub nous_id: Option<String>,
    /// Maximum number of sessions to return (default 50, max 1000).
    pub limit: Option<u32>,
    /// Cursor token from a previous response's `next_cursor` field.
    #[serde(default)]
    pub after: Option<String>,
}
```

```rust
pub struct HistoryParams {
    /// Maximum number of messages to return.
    pub limit: Option<u32>,
    /// Return messages with `seq` strictly less than this value.
    pub before: Option<i64>,
}
```

> Response for `GET /api/v1/sessions` (list).
> 
> Uses the standard paginated envelope. The `items` field contains
> `SessionListItem` values; `has_more` and `next_cursor` enable paging.
```rust
pub type ListSessionsResponse = crate::pagination::PaginatedResponse<SessionListItem>;
```

```rust
pub struct SessionListItem {
    /// Session identifier.
    pub id: String,
    /// Nous agent that owns this session.
    pub nous_id: String,
    /// Client-chosen deduplication key.
    pub session_key: String,
    /// Lifecycle status (e.g. `"active"`, `"archived"`).
    pub status: String,
    /// Total messages stored in this session.
    pub message_count: i64,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
    /// Human-readable name, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
```

```rust
pub struct SessionResponse {
    /// Session identifier.
    pub id: String,
    /// Nous agent owning this session.
    pub nous_id: String,
    /// Client-chosen deduplication key.
    pub session_key: String,
    /// Lifecycle status (e.g. `"active"`, `"archived"`).
    pub status: String,
    /// LLM model used for this session, if set.
    pub model: Option<String>,
    /// Human-readable display name, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Total messages stored in this session.
    pub message_count: i64,
    /// Estimated total tokens across all messages.
    pub token_count_estimate: i64,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
}
```

```rust
pub struct HistoryResponse {
    /// Conversation messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}
```

```rust
pub struct HistoryMessage {
    /// Database row ID.
    pub id: i64,
    /// Sequence number within the session.
    pub seq: i64,
    /// Message role (`"user"`, `"assistant"`, `"tool"`).
    pub role: String,
    /// Message text content.
    pub content: String,
    /// Tool call ID if this is a tool result message.
    pub tool_call_id: Option<String>,
    /// Tool name if this is a tool result message.
    pub tool_name: Option<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}
```

## `src/idempotency.rs`

> Thread-safe idempotency cache with LRU eviction and TTL expiry.
```rust
pub struct IdempotencyCache {
    inner: Mutex<CacheInner>,
    /// Maximum key length for idempotency keys.
    pub(crate) max_key_length: usize,
}
```

```rust
impl IdempotencyCache {
    pub fn new () -> Self;
    pub fn with_config (ttl: Duration, capacity: usize, max_key_length: usize) -> Self;
}
```

## `src/insights/anomaly.rs`

```rust
pub fn detect_anomalies (
    agent_id: &str,
    agent_name: &str,
    metric_name: &str,
    series: &[TimeSeriesPoint],
) -> Vec<AnomalyAlert>
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
> 
> Called once at startup from the binary crate's `register_all_metrics`.
```rust
pub fn register (registry: &mut Registry)
```

## `src/middleware/deprecation.rs`

```rust
pub struct DeprecationInfo {
    /// Unix timestamp when the endpoint was deprecated.
    pub deprecated_at: Timestamp,
    /// Unix timestamp when the endpoint will be removed.
    pub sunset_at: Timestamp,
    /// Optional URL to a migration guide.
    pub link: Option<String>,
}
```

```rust
impl DeprecationInfo {
    pub fn new (deprecated_at: Timestamp, sunset_at: Timestamp, link: Option<String>) -> Self;
}
```

```rust
pub struct DeprecationMap {
    inner: HashMap<String, DeprecationInfo>,
}
```

```rust
pub fn deprecate (
    pattern: impl Into<String>,
    deprecated_at: Timestamp,
    sunset_at: Timestamp,
    link: Option<String>,
) -> (String, DeprecationInfo)
```

```rust
pub struct DeprecationLayer {
    deprecations: Arc<DeprecationMap>,
}
```

```rust
impl DeprecationLayer {
    pub fn new (deprecations: impl IntoIterator<Item = (String, DeprecationInfo)>) -> Self;
}
```

```rust
pub struct DeprecationService<S> {
    inner: S,
    deprecations: Arc<DeprecationMap>,
}
```

## `src/middleware/etag.rs`

```rust
pub struct ETagLayer;
```

```rust
impl ETagLayer {
    pub fn new () -> Self;
}
```

```rust
pub struct ETagService<S> {
    inner: S,
}
```

## `src/middleware/mod.rs`

```rust
pub struct CsrfState {
    /// HTTP header name to check (e.g. `"x-requested-with"`).
    pub header_name: String,
    /// Expected header value (e.g. `"aletheia"`).
    pub header_value: String,
}
```

> Middleware that validates bearer auth for an entire router subtree.
> 
> The validated claims are cached in request extensions so handlers that also
> extract [`Claims`] do not re-validate the same token.
```rust
pub async fn require_bearer_auth (
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError>
```

> Middleware that requires a custom header on state-changing requests.
> 
> GET, HEAD, and OPTIONS are exempt. POST, PUT, DELETE, and PATCH must
> include the configured header with the expected value.
> 
> # Cancel safety
> 
> Cancel-safe. Axum middleware; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn require_csrf_header (request: Request, next: Next) -> Response
```

```rust
pub struct RequestId(pub String);
```

> Middleware that generates a ULID request ID and stores it in request extensions.
> 
> If the client sends an `X-Request-ID` header, the server echoes it for
> client-initiated correlation. Otherwise a new ULID is generated.
> 
> # Cancel safety
> 
> Cancel-safe. Axum middleware; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn inject_request_id (mut request: Request, next: Next) -> Response
```

> Middleware that normalizes error responses into the `ErrorResponse` JSON
> envelope and injects `request_id`.
> 
> WHY: Some error paths (e.g. Axum's built-in `Json` extractor rejection)
> produce plain-text error bodies instead of the `ErrorResponse` envelope.
> This middleware catches those responses and wraps them so all API errors
> have a consistent `{error: {code, message}}` shape (#3160).
> 
> Must be placed inside the compression layer so the body is uncompressed.
> 
> # Cancel safety
> 
> Cancel-safe. Axum middleware; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn enrich_error_response (request: Request, next: Next) -> Response
```

> Middleware that records HTTP request metrics (count + duration).
> 
> # Cancel safety
> 
> Cancel-safe. Axum middleware; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn record_http_metrics (request: Request, next: Next) -> Response
```

## `src/middleware/rate_limiter.rs`

> Per-IP sliding-window rate limiter for anonymous HTTP requests.
```rust
pub struct RateLimiter {
    // kanon:ignore RUST/pub-visibility
    max_requests: u32,
    window: Duration,
    state: Mutex<HashMap<String, (Instant, u32)>>,
    /// When true, `X-Forwarded-For` / `X-Real-IP` headers are trusted for
    /// client IP resolution. Enable only when pylon sits behind a trusted
    /// reverse proxy that strips these headers from untrusted clients.
    pub(super) trust_proxy: bool,
}
```

> Middleware that enforces per-IP rate limiting.
> 
> Reads the `Arc<RateLimiter>` from request extensions (installed by
> `build_router`). Returns 429 Too Many Requests with a `Retry-After` header
> when the client has exceeded the configured limit. On success, injects
> standard rate limit headers (#3268).
> 
> # Cancel safety
> 
> Cancel-safe. Axum middleware; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn rate_limit (request: Request, next: Next) -> Response
```

## `src/middleware/user_rate_limiter.rs`

```rust
pub enum EndpointCategory {
    /// LLM/chat endpoints (most expensive).
    Llm,
    /// Tool execution endpoints.
    Tool,
    /// All other API endpoints.
    General,
}
```

> Per-user token-bucket rate limiter with endpoint-category differentiation.
> 
> Each authenticated user gets separate token buckets for general, LLM, and
> tool endpoints. Additionally enforces a per-IP ceiling so that a single
> IP address cannot bypass limits by creating multiple bearer tokens (#3228).
> The per-IP ceiling uses the same RPM but a higher burst allowance
> ([`IP_CEILING_BURST_MULTIPLIER`] x per-user burst) to accommodate
> multiple legitimate users behind a shared IP.
> 
> Uses `std::sync::Mutex` (not tokio): the critical section
> is short and contains no `.await` points.
```rust
pub struct UserRateLimiter {
    config: PerUserRateLimitConfig,
    /// Per-user (token-keyed) rate limit state.
    state: Mutex<HashMap<String, UserBuckets>>,
    /// Per-IP rate limit ceiling. Checked alongside the per-user bucket so
    /// that a single IP cannot exceed the configured limit regardless of how
    /// many bearer tokens it presents (#3228).
    ip_state: Mutex<HashMap<String, UserBuckets>>,
}
```

> Middleware that enforces per-user rate limiting with endpoint categories.
> 
> Reads the `Arc<UserRateLimiter>` from request extensions. Keys on both
> bearer token hash (per-user) and client IP (per-IP ceiling). The per-IP
> check prevents a single IP from bypassing limits by creating multiple
> bearer tokens (#3228). Returns 429 with `Retry-After` header when the
> client has exceeded the configured limit for the endpoint category.
> 
> On successful responses, injects standard rate limit headers
> (`RateLimit-Limit`, `RateLimit-Remaining`, `RateLimit-Reset`) so
> consumers can self-throttle (#3268).
> 
> # Cancel safety
> 
> Cancel-safe. Axum middleware; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn per_user_rate_limit (request: Request, next: Next) -> Response
```

> Spawn a background task that periodically cleans up stale user rate limit
> entries to prevent unbounded memory growth.
```rust
pub fn spawn_stale_cleanup (
    limiter: Arc<UserRateLimiter>,
    shutdown: tokio_util::sync::CancellationToken,
)
```

## `src/openapi.rs`

> Serve the generated `OpenAPI` specification as JSON.
> 
> # Cancel safety
> 
> Cancel-safe. Axum handler; cancellation drops the future with no
> side effects beyond not returning a response.
```rust
pub async fn openapi_json (State(state): State<Arc<AppState>>) -> impl IntoResponse
```

## `src/pagination.rs`

```rust
pub struct PaginatedResponse<T> {
    /// The items in this page.
    pub items: Vec<T>,
    /// Whether more items exist beyond this page.
    pub has_more: bool,
    /// Cursor to pass as `after` to fetch the next page.
    /// `None` when `has_more` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Total count of matching items, when cheap to compute.
    /// `None` when the total is unknown or expensive to calculate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}
```

## `src/router.rs`

> Build the Axum router with all routes and middleware.
```rust
pub fn build_router (state: Arc<AppState>, security: &SecurityConfig) -> Router
```

```rust
pub fn build_router_with (
    state: Arc<AppState>,
    security: &SecurityConfig,
    extra: Option<Router>,
) -> Router
```

## `src/security.rs`

```rust
pub struct CorsConfig {
    /// Origins allowed by the CORS layer.
    pub allowed_origins: Vec<String>,
    /// CORS preflight cache duration.
    pub max_age_secs: u64,
}
```

```rust
pub struct CsrfConfig {
    /// Whether the CSRF header check is active.
    pub enabled: bool,
    /// HTTP header name for CSRF validation.
    pub header_name: String,
    /// Expected CSRF header value (per-instance CSPRNG token).
    pub header_value: String,
}
```

```rust
pub struct TlsConfig {
    /// Whether TLS termination is handled by pylon.
    pub enabled: bool,
    /// Path to PEM certificate file.
    pub cert_path: Option<PathBuf>,
    /// Path to PEM private key file.
    pub key_path: Option<PathBuf>,
}
```

```rust
pub struct RateLimitConfig {
    /// Whether per-IP rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute per client IP.
    pub requests_per_minute: u32,
    /// Trust X-Forwarded-For and X-Real-IP headers for client IP resolution.
    ///
    /// Enable only when pylon is behind a trusted reverse proxy that strips
    /// or overwrites these headers from untrusted clients. When false, the
    /// peer TCP address is used for rate limiting and logging.
    pub trust_proxy: bool,
    /// Per-user rate limiting configuration.
    pub per_user: PerUserRateLimitConfig,
}
```

```rust
pub struct SecurityConfig {
    /// Maximum request body size.
    pub body_limit_bytes: usize,
    /// Cross-Origin Resource Sharing settings.
    pub cors: CorsConfig,
    /// Cross-Site Request Forgery protection settings.
    pub csrf: CsrfConfig,
    /// TLS termination settings.
    pub tls: TlsConfig,
    /// Rate limiting settings.
    pub rate_limit: RateLimitConfig,
}
```

```rust
impl SecurityConfig {
    pub fn from_gateway (gateway: &GatewayConfig) -> Self;
}
```

## `src/server.rs`

```rust
pub struct ServerConfig {
    /// Address to bind, e.g. `"0.0.0.0:3000"`.
    pub bind_addr: String,
    /// Path to the Aletheia instance directory.
    pub instance_path: PathBuf,
    /// Security configuration for middleware layers.
    pub security: SecurityConfig,
}
```

```rust
pub enum ServerError {
    /// Failed to open or initialize the session store.
    #[snafu(display("failed to open session store: {source}"))]
    SessionStore { source: mneme::error::Error },

    /// TCP listener failed to bind to the configured address.
    #[snafu(display("failed to bind to {addr}: {source}"))]
    Bind {
        addr: String,
        source: std::io::Error,
    },

    /// Error while serving HTTP requests.
    #[snafu(display("server error: {source}"))]
    Serve { source: std::io::Error },

    /// TLS certificate or key could not be loaded.
    #[snafu(display("TLS configuration error: {source}"))]
    TlsConfig { source: std::io::Error },

    /// Binary was compiled without the `tls` feature flag.
    #[snafu(display("TLS support not compiled — rebuild with --features tls"))]
    TlsNotCompiled,

    /// Instance directory layout validation failed.
    #[snafu(display("instance layout invalid: {source}"))]
    Validation { source: taxis::error::Error },

    /// Default nous agent failed to spawn during startup.
    #[snafu(display("default nous spawn failed: {source}"))]
    NousSpawn { source: nous::error::Error },

    /// Authentication setup failed during startup.
    #[snafu(display("authentication setup failed: {message}"))]
    Auth { message: String },
}
```

> Start the HTTP gateway and block until shutdown.
> 
> # Errors
> 
> Returns [`ServerError::Validation`] if the instance directory layout is invalid.
> Returns [`ServerError::SessionStore`] if the session database cannot be opened.
> Returns [`ServerError::Bind`] if the server cannot bind to the configured address.
> Returns [`ServerError::Serve`] if the HTTP server encounters a fatal I/O error.
> Returns [`ServerError::NousSpawn`] if the default nous agent fails to spawn.
> Returns [`ServerError::TlsConfig`] if TLS is enabled but certs cannot be loaded.
> Returns [`ServerError::TlsNotCompiled`] if TLS is enabled but the feature is absent.
> 
> # Cancel safety
> 
> Not cancel-safe. Cancellation during startup (before `serve_plain`/`serve_tls`
> returns) leaves partially initialised state. Once serving, the future blocks
> until the OS delivers a shutdown signal; dropping it at that point skips the
> SIGHUP-handler drain and `shutdown_readonly` call, which may leave actor tasks
> running until the runtime exits.
```rust
pub async fn run (config: ServerConfig) -> Result<(), ServerError>
```

```rust
pub fn spawn_sighup_handler (state: Arc<AppState>) -> Option<tokio::task::JoinHandle<()>>
```

## `src/state.rs`

> Shared state for all Axum handlers, held behind `Arc` in the router.
```rust
pub struct AppState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of available LLM providers.
    pub provider_registry: Arc<ProviderRegistry>,
    /// Registry of tools available to nous agents.
    pub tool_registry: Arc<ToolRegistry>,
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
    /// JWT token creation and validation.
    pub jwt_manager: Arc<JwtManager>,
    /// Revocation-aware authentication facade.
    pub auth_facade: Arc<AuthFacade>,
    /// Server start instant for uptime calculation.
    pub start_time: Instant,
    /// Runtime configuration, updatable via config API.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Broadcast channel for config change notifications.
    ///
    /// Actors and subsystems subscribe via `config_rx` to receive the latest
    /// config after each hot-reload.
    pub config_tx: tokio::sync::watch::Sender<AletheiaConfig>,
    /// Auth mode from gateway config (`"token"`, `"none"`, etc.).
    pub auth_mode: String,
    /// Role assigned to anonymous requests when auth mode is `"none"`.
    pub none_role: String,
    /// Root shutdown token. Cancel to initiate graceful shutdown of all subsystems.
    pub shutdown: CancellationToken,
    /// Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
    pub idempotency_cache: Arc<IdempotencyCache>,
    /// Shared knowledge store for fact/entity/relationship queries.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Active embedding provider. Used by health checks to report degraded
    /// mode when the real provider failed to load at startup (#3380).
    ///
    /// `None` only when the pylon-only standalone server builds state without
    /// a configured embedding pipeline.
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Per-turn event buffer registry for SSE client recovery (#3276).
    ///
    /// Shared between streaming handlers (which record events) and the
    /// reconnect handler (which replays them).
    pub turn_buffer_registry: Arc<TurnBufferRegistry>,
    /// Shared Prometheus metrics registry.
    ///
    /// Crates register their metric families here at startup; the `/metrics`
    /// handler holds this to encode scrapes.
    pub metrics_registry: MetricsRegistry,
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
}
```

```rust
impl AppState {
    pub fn config_rx (&self) -> tokio::sync::watch::Receiver<AletheiaConfig>;
}
```

```rust
pub struct HealthState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Registry of available LLM providers.
    pub provider_registry: Arc<ProviderRegistry>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Server start instant for uptime calculation.
    pub start_time: std::time::Instant,
    /// Instance directory layout for path reporting.
    pub oikos: Arc<Oikos>,
    /// Runtime configuration for config readability checks.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Active embedding provider (for degraded-mode reporting, #3380).
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}
```

```rust
pub struct MetricsState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Server start instant for uptime calculation.
    pub start_time: std::time::Instant,
    /// Shared Prometheus metrics registry for encoding scrapes.
    pub metrics_registry: MetricsRegistry,
}
```

```rust
pub struct NousState {
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of tools available to nous agents.
    pub tool_registry: Arc<ToolRegistry>,
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
}
```

```rust
pub struct ConfigState {
    /// Runtime configuration, updatable via config API.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Broadcast channel for config change notifications.
    pub config_tx: tokio::sync::watch::Sender<AletheiaConfig>,
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
}
```

```rust
pub struct SessionsState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of available LLM providers.
    pub provider_registry: Arc<ProviderRegistry>,
    /// Root shutdown token. Cancel to initiate graceful shutdown of all subsystems.
    pub shutdown: CancellationToken,
    /// Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
    pub idempotency_cache: Arc<IdempotencyCache>,
    /// Runtime configuration for API limit reads.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Per-turn event buffer registry for SSE client recovery (#3276).
    pub turn_buffer_registry: Arc<TurnBufferRegistry>,
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
}
```

```rust
pub struct KnowledgeState {
    /// Shared knowledge store for fact/entity/relationship queries.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Runtime configuration for API limit reads.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
}
```

```rust
pub struct PlanningState {
    /// Root directory containing dianoia project workspaces.
    pub planning_root: PathBuf,
}
```

```rust
pub struct InsightsState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
}
```

```rust
pub struct EventBusState {
    /// In-process broadcast bus for domain events.
    pub event_bus: Arc<EventBus>,
    /// Runtime configuration for heartbeat interval reads.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
}
```

## `src/turn_buffer.rs`

> Registry of active and recently-completed turn buffers.
> 
> Held in `SessionsState` as `Arc<TurnBufferRegistry>`.
```rust
pub struct TurnBufferRegistry {
    buffers: Mutex<HashMap<TurnKey, Arc<Mutex<TurnBuffer>>>>,
    completed_ttl: Duration,
}
```

```rust
impl TurnBufferRegistry {
    pub fn new () -> Self;
    pub async fn reap_expired (&self);
}
```

## `src/types/insights.rs`

```rust
pub struct TimeSeriesPoint {
    /// ISO 8601 date (`YYYY-MM-DD`).
    pub date: String,
    /// Numeric value for this date.
    pub value: f64,
}
```

```rust
pub struct AgentPerformance {
    /// Agent identifier.
    pub agent_id: String,
    /// Human-readable agent name.
    pub agent_name: String,
    /// Average tokens per response.
    pub avg_tokens_per_response: f64,
    /// Tool calls per session.
    pub tool_calls_per_session: f64,
    /// Fraction of tool calls that succeeded (0.0–1.0).
    pub tool_success_rate: f64,
    /// Distillations per session.
    pub distillation_frequency: f64,
    /// Average context tokens before distillation.
    pub avg_context_before_distill: f64,
    /// Messages per session.
    pub messages_per_session: f64,
    /// Sessions per active day.
    pub sessions_per_day: f64,
    /// Errors per session.
    pub errors_per_session: f64,
    /// Daily time series of tokens-per-response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens_per_response_series: Vec<TimeSeriesPoint>,
}
```

```rust
pub struct AnomalyAlert {
    /// Agent identifier.
    pub agent_id: String,
    /// Human-readable agent name.
    pub agent_name: String,
    /// Metric that triggered the alert.
    pub metric_name: String,
    /// Latest observed value.
    pub current_value: f64,
    /// Mean of the rolling window.
    pub baseline_mean: f64,
    /// Percentage deviation from baseline.
    pub deviation_pct: f64,
    /// Direction of deviation (`"up"` or `"down"`).
    pub direction: String,
}
```

```rust
pub struct AgentPerformanceListResponse {
    /// Per-agent performance data.
    pub agents: Vec<AgentPerformance>,
    /// Anomalies detected across all agents.
    pub anomalies: Vec<AnomalyAlert>,
}
```

```rust
pub struct QualitySeries {
    /// Average turns per session per day.
    pub avg_turn_length: Vec<TimeSeriesPoint>,
    /// Ratio of assistant responses to user questions per day.
    pub response_to_question_ratio: Vec<TimeSeriesPoint>,
    /// Tool result messages per total messages per day.
    pub tool_call_density: Vec<TimeSeriesPoint>,
    /// Fraction of time spent in thinking mode per day.
    pub thinking_time_ratio: Vec<TimeSeriesPoint>,
}
```

```rust
pub struct QualityMetricsResponse {
    /// Time series quality indicators.
    pub series: QualitySeries,
}
```

```rust
pub struct MetricsQuery {
    /// Series granularity: daily, weekly, or monthly.
    #[serde(default)]
    pub granularity: Option<String>,
    /// Inclusive start date (`YYYY-MM-DD`).
    #[serde(default)]
    pub from: Option<String>,
    /// Inclusive end date (`YYYY-MM-DD`).
    #[serde(default)]
    pub to: Option<String>,
}
```

```rust
pub struct TokenSeriesPoint {
    /// Bucket date (`YYYY-MM-DD`, ISO week, or `YYYY-MM`).
    pub date: String,
    /// Input tokens in this bucket.
    pub input_tokens: u64,
    /// Output tokens in this bucket.
    pub output_tokens: u64,
}
```

```rust
pub struct AgentTokenRow {
    /// Agent identifier.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Input tokens attributed to this agent.
    pub input_tokens: u64,
    /// Output tokens attributed to this agent.
    pub output_tokens: u64,
    /// Sessions attributed to this agent.
    pub session_count: u64,
}
```

```rust
pub struct ModelTokenRow {
    /// Model identifier.
    pub model: String,
    /// Input tokens attributed to this model.
    pub input_tokens: u64,
    /// Output tokens attributed to this model.
    pub output_tokens: u64,
    /// Sessions attributed to this model.
    pub session_count: u64,
}
```

```rust
pub struct TokenMetricsResponse {
    /// Token usage over time.
    pub series: Vec<TokenSeriesPoint>,
    /// Token usage grouped by agent.
    pub agents: Vec<AgentTokenRow>,
    /// Token usage grouped by model.
    pub models: Vec<ModelTokenRow>,
    /// Input tokens used today.
    pub today_input: u64,
    /// Output tokens used today.
    pub today_output: u64,
    /// Input tokens used this week.
    pub week_input: u64,
    /// Output tokens used this week.
    pub week_output: u64,
    /// Input tokens used this month.
    pub month_input: u64,
    /// Output tokens used this month.
    pub month_output: u64,
    /// Input tokens used in the previous equivalent day.
    pub prev_today_input: u64,
    /// Output tokens used in the previous equivalent day.
    pub prev_today_output: u64,
    /// Input tokens used in the previous equivalent week.
    pub prev_week_input: u64,
    /// Output tokens used in the previous equivalent week.
    pub prev_week_output: u64,
    /// Input tokens used in the previous equivalent month.
    pub prev_month_input: u64,
    /// Output tokens used in the previous equivalent month.
    pub prev_month_output: u64,
}
```

```rust
pub struct CostSeriesPoint {
    /// Bucket date (`YYYY-MM-DD`, ISO week, or `YYYY-MM`).
    pub date: String,
    /// Estimated cost in USD for this bucket.
    pub cost_usd: f64,
}
```

```rust
pub struct AgentCostRow {
    /// Agent identifier.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Estimated cost in USD.
    pub total_cost: f64,
    /// Message count attributed to this agent.
    pub message_count: u64,
    /// Sessions attributed to this agent.
    pub session_count: u64,
    /// Output tokens attributed to this agent.
    pub output_tokens: u64,
    /// Cost from the previous equivalent period.
    pub prev_period_cost: f64,
}
```

```rust
pub struct CostMetricsResponse {
    /// Estimated cost over time.
    pub series: Vec<CostSeriesPoint>,
    /// Estimated costs grouped by agent.
    pub agents: Vec<AgentCostRow>,
    /// Estimated cost today.
    pub today_cost: f64,
    /// Estimated cost this week.
    pub week_cost: f64,
    /// Estimated cost this month.
    pub month_cost: f64,
    /// Estimated cost for the previous equivalent day.
    pub prev_today_cost: f64,
    /// Estimated cost for the previous equivalent week.
    pub prev_week_cost: f64,
    /// Estimated cost for the previous equivalent month.
    pub prev_month_cost: f64,
}
```

```rust
pub struct JournalEvent {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Event category (`error`, `distillation`, `config`, `memory`).
    pub event_type: String,
    /// Human-readable description.
    pub message: String,
}
```

```rust
pub struct JournalQuery {
    /// Filter by source subsystem.
    #[serde(default)]
    pub source: Option<String>,
    /// Filter by severity level.
    #[serde(default)]
    pub level: Option<String>,
    /// Only events after this ISO 8601 timestamp.
    #[serde(default)]
    pub since: Option<String>,
    /// Maximum events to return (default 100, max 1000).
    #[serde(default = "default_journal_limit")]
    pub limit: u32,
}
```

## `tests/common/mod.rs`

> Minimal oikos tempdir with the directories and config files the
> health-check handlers expect to be readable.
```rust
pub struct TestEnv {
    pub state: Arc<AppState>,
    pub _tmp: tempfile::TempDir,
}
```

```rust
impl TestEnv {
    pub async fn new () -> Self;
    pub fn builder () -> TestEnvBuilder;
}
```

```rust
pub struct TestEnvBuilder {
    with_actor: bool,
    auth_mode: Option<String>,
    jwt_access_ttl: Option<Duration>,
}
```

```rust
impl TestEnvBuilder {
    pub fn with_actor (mut self, flag: bool) -> Self;
    pub fn auth_mode (mut self, mode: &str) -> Self;
    pub fn jwt_access_ttl (mut self, ttl: Duration) -> Self;
    pub async fn build (self) -> TestEnv;
}
```

> `SecurityConfig` with CSRF disabled: exercises the default route matrix
> without requiring the CSRF header on mutations.
```rust
pub fn permissive_security () -> SecurityConfig
```

```rust
pub fn issue_test_token (state: &AppState) -> String
```

```rust
pub fn issue_test_token_as (state: &AppState, role: Role) -> String
```

```rust
pub fn bearer (token: &str) -> String
```

```rust
pub async fn read_body_json (response: axum::response::Response) -> serde_json::Value
```
