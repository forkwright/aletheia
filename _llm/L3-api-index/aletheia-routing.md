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

> A static router that records after-action outcomes into a shared store.
> 
> This is the interactive-runtime counterpart to the richer dispatch
> empirical routers: it does not change provider selection, but it prevents
> completed turns from being discarded when the binary has not enabled an
> empirical selection policy.
```rust
pub struct RecordingRouter {
    /// Shared empirical outcome store.
    store: Arc<AfterActionStore>,
    /// Static provider/model returned for route calls.
    provider: Arc<str>,
}
```

```rust
impl RecordingRouter {
    pub fn new (store: Arc<AfterActionStore>, provider: impl Into<Arc<str>>) -> Self;
}
```

> A router combinator that falls through to a secondary router when the
> primary router's confidence is below a threshold.
> 
> WHY(#3969): the Q-learner (and any learned router) needs a way to defer to
> a static or rule-based fallback when it has insufficient data to make a
> high-confidence decision. `FallthroughRouter` is that combinator: it runs
> the primary router first, and if `confidence < threshold` (or the primary
> returns `None` confidence), delegates to the secondary.
> 
> Both `after_action` calls are forwarded to the primary router only. The
> secondary is a read-only fallback; recording against it would corrupt the
> primary's training signal.
> 
> # Example
> 
> ```rust
> # use aletheia_routing::{FallthroughRouter, NoOpRouter};
> # use std::sync::Arc;
> let primary = Arc::new(NoOpRouter { provider: Arc::from("learned") });
> let fallback = Arc::new(NoOpRouter { provider: Arc::from("static") });
> // Fall through to `fallback` when primary confidence < 0.5.
> let router = FallthroughRouter::new(primary, fallback, 0.5);
> ```
```rust
pub struct FallthroughRouter {
    /// Primary router — queried first on every `route` call.
    primary: Arc<dyn Router>,
    /// Fallback router — used when primary confidence is below threshold.
    fallback: Arc<dyn Router>,
    /// Minimum confidence required to accept the primary decision.
    ///
    /// Must be in `[0.0, 1.0]`. A value of `0.0` means always accept the
    /// primary decision; `1.0` means always fall through.
    threshold: f64,
}
```

```rust
impl FallthroughRouter {
    pub fn new (primary: Arc<dyn Router>, fallback: Arc<dyn Router>, threshold: f64) -> Self;
    pub fn threshold (&self) -> f64;
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
pub enum RoutingBoundary {
    /// External cloud provider allowed. Widest boundary; permits all providers.
    ///
    /// This is the default so routers that have not been updated to pass a
    /// boundary never accidentally restrict routing.
    #[default]
    Cloud,
    /// Only local-hosted or embedded providers (no external API calls).
    LocalHosted,
    /// Only in-process providers (fully air-gapped).
    Embedded,
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

    /// Maximum allowed deployment boundary for this request.
    ///
    /// Routers that respect sovereignty must not select providers whose
    /// deployment target exceeds this boundary. Defaults to
    /// [`RoutingBoundary::Cloud`] so existing call-sites are not broken.
    ///
    /// WHY(#3969): the Q-learner and fallthrough router need this in context
    /// so they can filter candidates by sovereignty without out-of-band state.
    #[doc(hidden)]
    pub deployment_target: RoutingBoundary,
}
```

```rust
impl RequestFeatures {
    pub fn new (
        candidates: Vec<ProviderId>,
        task_category: Option<TaskCategory>,
        prompt_text: Option<Arc<str>>,
    ) -> Self;
    pub fn with_deployment_target (mut self, boundary: RoutingBoundary) -> Self;
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
