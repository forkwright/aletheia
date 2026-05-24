# L3 API Index: aletheia-routing

Crate path: `crates/aletheia-routing`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/lib.rs`

> A provider/model router that supports empirical feedback.
> 
> Implementors select a provider or model based on [`RequestFeatures`] and
> accept [`TurnOutcome`] records after each interaction so the router can
> improve over time.
> 
> # Dyn compatibility
> 
> `route` returns a [`BoxFuture`] rather than using `async fn` so the trait
> is dyn-compatible and can be stored as `Arc<dyn Router>`. Implementors
> return `Box::pin(async move { ... })` from `route`.
```rust
pub trait Router : Send + Sync {
    fn route <'a> (&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision>;
    fn after_action (
        &self,
        decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError>;
}
```

> A no-op router used when no empirical router is configured.
> 
> Always returns the configured static provider and discards after-action
> records. Satisfies `Arc<dyn Router>` without requiring fjall.
```rust
pub struct NoOpRouter {
    /// The static provider returned for all requests.
    pub provider: Arc<str>,
}
```

## `src/store.rs`

```rust
pub enum AfterActionStoreError {
    /// Could not read a JSONL log directory.
    #[snafu(display("I/O error reading after-action log '{}': {source}", path.display()))]
    Io {
        /// Path that triggered the error.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}
```

```rust
pub struct RollingStats {
    /// Sessions that completed successfully.
    pub successes: u64,
    /// Sessions that ended in any failure state.
    pub failures: u64,
    /// Total sessions (`successes + failures`).
    pub total: u64,
    /// Timestamp of the most recent successful session, if any.
    pub last_success_at: Option<Timestamp>,
}
```

```rust
impl RollingStats {
    pub fn success_rate (&self) -> Option<f64>;
}
```

```rust
pub struct AfterActionStore {
    /// Directory containing per-day JSONL files (`YYYY-MM-DD.jsonl`).
    ///
    /// `None` when the store is used in memory-only mode (interactive path
    /// without a configured log directory).
    dir: Option<PathBuf>,
    /// Latest per-day JSONL files to include during refresh.
    window: Duration,
    /// In-memory cache: `(provider_id, task_category)` → [`RollingStats`].
    cache: RwLock<HashMap<(ProviderId, TaskCategory), RollingStats>>,
    /// Direct interactive writes since the most recent disk refresh.
    interactive: RwLock<HashMap<(ProviderId, TaskCategory), RollingStats>>,
}
```

```rust
impl AfterActionStore {
    pub fn new (dir: PathBuf) -> Self;
    pub fn new_with_window (dir: PathBuf, window: Duration) -> Self;
    pub fn in_memory () -> Self;
    pub async fn rolling_stats (
        &self,
        provider: &ProviderId,
        cat: &TaskCategory,
        window: std::time::Duration,
    ) -> Option<RollingStats>;
    pub async fn record_outcome (&self, outcome: &TurnOutcome);
    pub async fn refresh (&self) -> Result<(), AfterActionStoreError>;
    pub async fn refresh_window (&self, window: Duration) -> Result<(), AfterActionStoreError>;
}
```

> Parse a category string from a JSONL record.
> 
> Returns [`TaskCategory::Feature`] for unrecognised strings so that new
> categories added in future PRs degrade gracefully on old store data.
```rust
pub fn parse_category (s: &str) -> TaskCategory
```

## `src/types.rs`

```rust
pub enum TaskCategory {
    /// Code restructuring without behaviour change.
    Refactor,
    /// New product feature.
    Feature,
    /// Defect correction.
    Bug,
    /// Documentation or comment changes.
    Docs,
    /// Tests and test infrastructure.
    Test,
    /// Housekeeping, dependency updates, CI.
    Chore,
}
```

```rust
impl TaskCategory {
    pub fn from_prompt (text: &str) -> Self;
}
```

```rust
pub struct ProviderId(pub Arc<str>);
```

```rust
impl ProviderId {
    pub fn new (id: impl Into<Arc<str>>) -> Self;
}
```

```rust
pub struct RequestFeatures {
    /// Candidate provider IDs eligible for selection.
    ///
    /// An empty slice causes the router to return its configured static
    /// fallback. Dispatch paths supply all configured providers; interactive
    /// paths supply the currently-active provider from the agent config.
    pub candidates: Vec<ProviderId>,

    /// High-level category for aggregation in the success-rate store.
    ///
    /// When `None`, the store key falls back to [`TaskCategory::Feature`].
    pub task_category: Option<TaskCategory>,

    /// Free-text prompt or message that drove this request.
    ///
    /// Used by category-inference helpers when `task_category` is absent.
    pub prompt_text: Option<Arc<str>>,
}
```

```rust
impl RequestFeatures {
    pub fn new (
        candidates: Vec<ProviderId>,
        task_category: Option<TaskCategory>,
        prompt_text: Option<Arc<str>>,
    ) -> Self;
    pub fn effective_category (&self) -> TaskCategory;
}
```

```rust
pub struct RoutingDecision {
    /// The selected provider identifier.
    pub provider: Arc<str>,

    /// Empirical confidence in the selection (0.0–1.0), if the router has
    /// enough historical data to compute one. `None` for static/fallback
    /// decisions.
    pub confidence: Option<f64>,
}
```

```rust
impl RoutingDecision {
    pub fn new (provider: impl Into<Arc<str>>, confidence: Option<f64>) -> Self;
}
```

```rust
pub struct TurnOutcome {
    /// The provider identifier that handled this turn.
    pub provider: ProviderId,

    /// Task category for the aggregation key.
    pub task_category: TaskCategory,

    /// Whether the turn completed successfully.
    pub success: bool,

    /// Whether the response path was the interactive (nous) path.
    ///
    /// `false` means dispatch (energeia). Used for observability; the storage
    /// backend is the same regardless of path.
    pub is_interactive: bool,
}
```

```rust
impl TurnOutcome {
    pub fn new (
        provider: ProviderId,
        task_category: TaskCategory,
        success: bool,
        is_interactive: bool,
    ) -> Self;
}
```

```rust
pub enum RouterError {
    /// After-action record could not be written to the store.
    #[snafu(display("router after-action write failed: {message}"))]
    AfterActionWrite {
        /// Human-readable error description.
        message: String,
    },
}
```
