# L3 API Index: oikonomos

Crate path: `crates/daemon`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/bridge/bridge_impl.rs`

> No-op bridge for tests and configurations without nous integration.
```rust
pub struct NoopBridge;
```

## `src/bridge.rs`

> Allows daemon tasks to send prompts to a nous actor without the daemon
> crate depending on nous directly.
> 
> Implemented in the binary crate where both daemon and nous are available.
> Uses boxed futures for object safety (`Arc<dyn DaemonBridge>`).
```rust
pub trait DaemonBridge : Send + Sync {
    fn send_prompt (
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>>;
}
```

## `src/coordination.rs`

```rust
pub struct Coordinator {
    max_children: usize,
}
```

```rust
impl Coordinator {
    pub fn new (max_children: usize) -> Self;
    pub fn max_children (&self) -> usize;
}
```

## `src/cron/evolution.rs`

```rust
pub struct CronEvolutionConfig {
    /// Whether the evolution cron is enabled.
    pub enabled: bool,
    /// Interval between evolution runs.
    pub interval: Duration,
}
```

## `src/cron/graph_cleanup.rs`

```rust
pub struct CronGraphCleanupConfig {
    /// Whether the graph cleanup cron is enabled.
    pub enabled: bool,
    /// Interval between cleanup runs.
    pub interval: Duration,
}
```

## `src/cron/mod.rs`

```rust
pub struct CronConfig {
    /// Evolution: periodic configuration variant search.
    pub evolution: CronEvolutionConfig,
    /// Reflection: periodic self-reflection prompt.
    pub reflection: CronReflectionConfig,
    /// Graph cleanup: periodic knowledge graph orphan and stale entity removal.
    pub graph_cleanup: CronGraphCleanupConfig,
}
```

## `src/cron/reflection.rs`

```rust
pub struct CronReflectionConfig {
    /// Whether the reflection cron is enabled.
    pub enabled: bool,
    /// Interval between reflection runs.
    pub interval: Duration,
}
```

## `src/cron_expr.rs`

```rust
pub struct CronExpr {
    seconds: BTreeSet<u8>,
    minutes: BTreeSet<u8>,
    hours: BTreeSet<u8>,
    days_of_month: BTreeSet<u8>,
    months: BTreeSet<u8>,
    days_of_week: BTreeSet<u8>,
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// Invalid cron expression.
    #[snafu(display("invalid cron expression '{expression}': {reason}"))]
    CronParse {
        expression: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Task execution failed.
    #[snafu(display("task execution failed for {task_id}: {reason}"))]
    TaskFailed {
        task_id: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Shell command execution failed.
    #[snafu(display("command execution failed: {command}"))]
    CommandFailed {
        command: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Task disabled after consecutive failures.
    #[snafu(display("task {task_id} disabled after {failures} consecutive failures"))]
    TaskDisabled {
        task_id: String,
        failures: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Shutdown signal received.
    #[snafu(display("shutdown signal received"))]
    Shutdown {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage backend error (fjall task-state store).
    #[snafu(display("task-state storage error: {message}"))]
    Storage {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization error within stored task state.
    #[snafu(display("task-state JSON error: {source}"))]
    StoredJson {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// I/O error during maintenance operation.
    #[snafu(display("maintenance I/O error: {context}"))]
    MaintenanceIo {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Spawned blocking task failed.
    #[snafu(display("blocking task failed: {context}"))]
    BlockingJoin {
        context: String,
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invariant violation during maintenance (situation the caller believed impossible).
    #[snafu(display("maintenance invariant violated: {context}"))]
    MaintenanceInvariant {
        context: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Convenience alias for `Result` with daemon's [`Error`] type.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/maintenance/db_monitor.rs`

```rust
pub struct DbMonitoringConfig {
    /// Whether database monitoring is active.
    pub enabled: bool,
    /// Directory containing database files to monitor.
    pub data_dir: PathBuf,
    /// Size in MB above which a warning is reported.
    pub warn_threshold_mb: u64,
    /// Size in MB above which an alert is raised.
    pub alert_threshold_mb: u64,
}
```

```rust
pub struct DbSizeReport {
    /// Individual database entries with size and status.
    pub databases: Vec<DbInfo>,
    /// Sum of all database sizes in bytes.
    pub total_size_bytes: u64,
    /// Human-readable alert messages for databases exceeding thresholds.
    pub alerts: Vec<String>,
}
```

```rust
pub struct DbInfo {
    /// Database file or directory name (e.g., `"sessions.db"`, `"cozo/"`).
    pub name: String,
    /// Absolute path to the database file or directory.
    pub path: PathBuf,
    /// Total size in bytes.
    pub size_bytes: u64,
    /// Health classification based on configured thresholds.
    pub status: DbStatus,
}
```

```rust
pub enum DbStatus {
    /// Size is within normal bounds.
    Ok,
    /// Size exceeds the warning threshold but is below the alert threshold.
    Warning,
    /// Size exceeds the alert threshold and needs attention.
    Alert,
}
```

> Monitors database file sizes and reports against thresholds.
```rust
pub struct DbMonitor {
    config: DbMonitoringConfig,
}
```

```rust
impl DbMonitor {
    pub fn new (config: DbMonitoringConfig) -> Self;
    pub fn check (&self) -> error::Result<DbSizeReport>;
}
```

## `src/maintenance/drift_detection.rs`

```rust
pub struct DriftDetectionConfig {
    /// Whether drift detection is active.
    pub enabled: bool,
    /// Path to the live instance directory.
    pub instance_root: PathBuf,
    /// Path to the example/template directory to compare against.
    pub example_root: PathBuf,
    /// Whether to raise alerts for files present in the template but missing from the instance.
    pub alert_on_missing: bool,
    /// Glob-like patterns to exclude from comparison entirely (e.g., `"data/"`, `"*.db"`).
    pub ignore_patterns: Vec<String>,
    /// Glob-like patterns for files that are optional scaffolding. Missing files matching
    /// these patterns are reported at info level rather than warn level.
    pub optional_patterns: Vec<String>,
}
```

```rust
pub struct DriftReport {
    /// Required files present in the template but absent from the instance.
    pub missing_files: Vec<PathBuf>,
    /// Optional scaffolding files present in the template but absent from the instance.
    pub optional_missing_files: Vec<PathBuf>,
    /// Files present in the instance but absent from the template.
    pub extra_files: Vec<PathBuf>,
    /// Files with permission discrepancies (path, description).
    pub permission_issues: Vec<(PathBuf, String)>,
    /// When the check was performed.
    pub checked_at: Option<jiff::Timestamp>,
}
```

> Compares an instance directory against the example template.
```rust
pub struct DriftDetector {
    config: DriftDetectionConfig,
}
```

```rust
impl DriftDetector {
    pub fn new (config: DriftDetectionConfig) -> Self;
    pub fn check (&self) -> error::Result<DriftReport>;
}
```

## `src/maintenance/fjall_backup.rs`

```rust
pub struct FjallBackupConfig {
    /// Whether periodic fjall backups are enabled.
    pub enabled: bool,
    /// Path to the fjall knowledge store data directory.
    pub source_dir: PathBuf,
    /// Directory where timestamped backups are stored.
    pub backup_dir: PathBuf,
    /// Hours between automatic backups.
    pub interval_hours: u64,
    /// Maximum number of backup snapshots to retain.
    pub retention_count: usize,
}
```

```rust
pub struct FjallBackupReport {
    /// Path to the created backup directory.
    pub backup_path: Option<PathBuf>,
    /// Total bytes copied.
    pub bytes_copied: u64,
    /// Number of files copied.
    pub files_copied: u32,
    /// Number of old backups pruned.
    pub backups_pruned: u32,
}
```

```rust
pub struct BackupEntry {
    /// Directory name (timestamp-based).
    pub name: String,
    /// Full path to the backup directory.
    pub path: PathBuf,
    /// When the backup was created (from directory mtime).
    pub created: SystemTime,
    /// Total size of the backup in bytes.
    pub size_bytes: u64,
}
```

> Manages fjall knowledge store backups.
```rust
pub struct FjallBackup {
    config: FjallBackupConfig,
}
```

```rust
impl FjallBackup {
    pub fn new (config: FjallBackupConfig) -> Self;
    pub fn create_backup (&self) -> error::Result<FjallBackupReport>;
    pub fn list_backups (&self) -> error::Result<Vec<BackupEntry>>;
}
```

## `src/maintenance/knowledge.rs`

```rust
pub struct MaintenanceReport {
    /// Total items examined during the operation.
    pub items_processed: u64,
    /// Items that were actually changed.
    pub items_modified: u64,
    /// Number of non-fatal errors encountered.
    pub errors: u32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Optional human-readable detail string.
    pub detail: Option<String>,
}
```

> Bridge trait for knowledge graph maintenance operations.
> 
> Daemon crate defines it, binary crate implements it where `KnowledgeStore`
> is available. All methods are blocking: the runner wraps in `spawn_blocking`.
```rust
pub trait KnowledgeMaintenanceExecutor : Send + Sync {
    fn insert_fact (&self, fact: &episteme::knowledge::Fact) -> crate::error::Result<()>;
    fn refresh_decay_scores (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
    fn deduplicate_entities (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
    fn recompute_graph_scores (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
    fn refresh_embeddings (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
    fn garbage_collect (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
    fn maintain_indexes (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
    fn health_check (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
    fn run_skill_decay (&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
}
```

```rust
pub struct KnowledgeMaintenanceConfig {
    /// Whether knowledge maintenance tasks are enabled.
    pub enabled: bool,
    /// Auto-dream consolidation settings.
    pub auto_dream: AutoDreamConfig,
}
```

```rust
pub struct AutoDreamConfig {
    /// Whether auto-dream consolidation is enabled.
    pub enabled: bool,
    /// Minimum hours between consolidation runs.
    pub min_hours: u64,
    /// Minimum sessions required to trigger consolidation.
    pub min_sessions: usize,
    /// Session scan throttle interval in seconds.
    pub scan_interval_secs: i64,
    /// Stale lock threshold in seconds.
    pub stale_threshold_secs: i64,
}
```

## `src/maintenance/mod.rs`

```rust
pub struct ProposeRulesConfig {
    /// Whether the rule proposal task is enabled.
    pub enabled: bool,
    /// Directory where `rule_proposals.toml` is written.
    ///
    /// Defaults to `instance/data` resolved from `ALETHEIA_ROOT`.
    pub data_dir: std::path::PathBuf,
}
```

> Records backup observability metrics from the runtime crate.
```rust
pub trait BackupMetricsRecorder : std::fmt::Debug + Send + Sync {
    fn record_backup_duration (&self, duration_secs: f64, success: bool);
}
```

```rust
pub struct MaintenanceConfig {
    /// Trace file rotation and compression settings.
    pub trace_rotation: TraceRotationConfig,
    /// Instance drift detection settings.
    pub drift_detection: DriftDetectionConfig,
    /// Database size monitoring thresholds.
    pub db_monitoring: DbMonitoringConfig,
    /// Data retention policy settings.
    pub retention: RetentionConfig,
    /// Knowledge graph maintenance settings.
    pub knowledge_maintenance: KnowledgeMaintenanceConfig,
    /// Fjall knowledge store backup settings.
    pub fjall_backup: FjallBackupConfig,
    /// Runtime metrics hook for backup freshness alerting.
    pub backup_metrics: Option<Arc<dyn BackupMetricsRecorder>>,
    /// Directory where prosoche self-audit reports are written.
    pub prosoche_audit_dir: PathBuf,
    /// Cron task configuration (evolution, reflection, graph cleanup).
    pub cron: crate::cron::CronConfig,
    /// Rule proposal generation from observed patterns.
    pub propose_rules: ProposeRulesConfig,
    /// Prompt audit log retention pruning (#3411).
    pub prompt_audit: PromptAuditRetentionConfig,
    /// Runtime handle for refreshing empirical routing after-action stats.
    pub after_action_store: Option<Arc<aletheia_routing::AfterActionStore>>,
}
```

## `src/maintenance/prompt_audit_rotation.rs`

```rust
pub struct PromptAuditRetentionConfig {
    /// Whether pruning is active.
    pub enabled: bool,
    /// Directory holding daily JSONL files.
    pub log_dir: PathBuf,
    /// Files older than this many days are deleted.
    pub retention_days: u32,
}
```

```rust
pub struct PromptAuditRetentionReport {
    /// Number of daily files deleted.
    pub files_pruned: u32,
    /// Total bytes freed.
    pub bytes_freed: u64,
}
```

> Prunes prompt-audit daily files past the retention window.
```rust
pub struct PromptAuditRotator {
    config: PromptAuditRetentionConfig,
}
```

```rust
impl PromptAuditRotator {
    pub fn new (config: PromptAuditRetentionConfig) -> Self;
    pub fn prune (&self) -> error::Result<PromptAuditRetentionReport>;
}
```

## `src/maintenance/retention.rs`

```rust
pub struct RetentionConfig {
    /// Whether retention policy execution is active.
    pub enabled: bool,
}
```

> Trait for components that can execute data retention cleanup.
> 
> Implemented in the aletheia binary where `SessionStore` is available.
> The daemon crate defines the interface only.
```rust
pub trait RetentionExecutor : Send + Sync {
    fn execute_retention (&self) -> crate::error::Result<RetentionSummary>;
}
```

```rust
pub struct RetentionSummary {
    /// Number of expired sessions removed.
    pub sessions_cleaned: u32,
    /// Number of expired messages removed.
    pub messages_cleaned: u32,
    /// Number of expired blackboard entries removed.
    #[serde(default)]
    pub blackboard_entries_cleaned: u32,
    /// Total bytes reclaimed from storage.
    pub bytes_freed: u64,
}
```

## `src/maintenance/trace_rotation.rs`

```rust
pub struct TraceRotationConfig {
    /// Whether trace rotation is active.
    pub enabled: bool,
    /// Directory containing active trace files.
    pub trace_dir: PathBuf,
    /// Directory where rotated files are moved.
    pub archive_dir: PathBuf,
    /// Maximum age in days before a trace file is rotated.
    pub max_age_days: u32,
    /// Maximum total size of active trace files in MB before forcing rotation.
    pub max_total_size_mb: u64,
    /// Whether to gzip-compress rotated files.
    pub compress: bool,
    /// Maximum number of archived files to retain before pruning the oldest.
    pub max_archives: usize,
}
```

```rust
pub struct RotationReport {
    /// Number of trace files moved to the archive directory.
    pub files_rotated: u32,
    /// Number of old archive files deleted beyond the retention limit.
    pub files_pruned: u32,
    /// Total bytes freed from the active trace directory.
    pub bytes_freed: u64,
}
```

> Rotates old trace files to an archive directory with optional gzip compression.
```rust
pub struct TraceRotator {
    config: TraceRotationConfig,
    disk_monitor: Option<DiskSpaceMonitor>,
}
```

```rust
impl TraceRotator {
    pub fn new (config: TraceRotationConfig) -> Self;
    pub fn set_disk_monitor (&mut self, monitor: DiskSpaceMonitor);
    pub fn rotate (&self) -> error::Result<RotationReport>;
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/probe.rs`

```rust
pub struct ProbeAuditConfig {
    /// Whether the probe audit task is enabled.
    pub enabled: bool,
    /// Interval between probe audit runs.
    pub interval: Duration,
    /// Which probe categories to run.
    pub categories: Vec<ProbeCategory>,
}
```

```rust
pub enum ProbeCategory {
    /// Same question phrased differently should yield consistent answers.
    Consistency,
    /// Edge-case inputs that must be handled gracefully (refused or answered safely).
    Boundary,
    /// Questions about facts that should be retrievable from the knowledge graph.
    Recall,
}
```

```rust
pub struct Probe {
    /// Stable identifier used in result storage and log correlation.
    pub id: &'static str,
    /// Probe category.
    pub category: ProbeCategory,
    /// The prompt text to send to the nous.
    pub prompt: &'static str,
    /// Forbidden substrings (case-insensitive) that must not appear in the response.
    ///
    /// WHY: used for injection and boundary probes where the correct behavior is
    /// refusal — the response must not echo back sensitive content.
    pub forbidden_patterns: &'static [&'static str],
    /// Required substrings (case-insensitive) that must appear in the response.
    ///
    /// WHY: used for recall probes where the response must contain the known fact.
    pub required_patterns: &'static [&'static str],
    /// Human-readable description of what this probe tests.
    pub description: &'static str,
}
```

```rust
pub struct ProbeSet {
    probes: Vec<Probe>,
}
```

```rust
impl ProbeSet {
    pub fn new () -> Self;
    pub fn default_probes () -> Self;
    pub fn for_categories (categories: &[ProbeCategory]) -> Self;
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
    pub fn iter (&self) -> impl Iterator<Item = &Probe>;
    pub fn run_probe (probe: &Probe, response: &str) -> ProbeResult;
}
```

```rust
pub struct ProbeResult {
    /// Stable probe identifier.
    // kanon:ignore RUST/primitive-for-domain-id — ProbeResult is a serialization type used in probe audit summaries; probe_id is a stable string key from the static probe set, not a domain entity ID
    pub probe_id: String,
    /// Category of probe.
    pub category: ProbeCategory,
    /// Whether the probe passed (no violations, all required patterns present).
    pub passed: bool,
    /// Confidence in the pass/fail verdict, 0.0–1.0.
    ///
    /// 1.0 = clean pass. Degrades by 0.25 per issue (violation or missing required).
    pub confidence: f32,
    /// Forbidden patterns that appeared in the response.
    pub violations: Vec<String>,
    /// Required patterns that were absent from the response.
    pub missing_required: Vec<String>,
}
```

```rust
pub struct ProbeAuditSummary {
    /// Total probes evaluated.
    pub total: usize,
    /// Number of probes that passed.
    pub passed: usize,
    /// Number of probes that failed.
    pub failed: usize,
    /// Average confidence across all probes.
    pub avg_confidence: f32,
    /// Per-probe results.
    pub results: Vec<ProbeResult>,
}
```

```rust
impl ProbeAuditSummary {
    pub fn from_results (results: Vec<ProbeResult>) -> Self;
    pub fn one_line (&self) -> String;
}
```

```rust
pub fn build_probe_audit_prompt (probe_set: &ProbeSet) -> String
```

## `src/prosoche.rs`

> Prosoche attention check runner.
```rust
pub struct ProsocheCheck {
    nous_id: String,
    /// Instance data directory to check for disk usage.
    data_dir: Option<PathBuf>,
    /// Database file paths to check sizes.
    db_paths: Vec<PathBuf>,
    /// Optional knowledge store for memory consistency checks.
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<episteme::knowledge_store::KnowledgeStore>>,
    /// Number of recent facts sampled by anomaly detection.
    anomaly_sample_size: usize,
}
```

```rust
pub struct ProsocheResult {
    /// Items requiring the nous's attention.
    pub items: Vec<AttentionItem>,
    /// ISO 8601 timestamp when the check was performed.
    pub checked_at: String,
}
```

```rust
pub struct AttentionItem {
    /// What kind of attention is needed.
    pub category: AttentionCategory,
    /// Human-readable description of the item.
    pub summary: String,
    /// How urgently this needs attention.
    pub urgency: Urgency,
}
```

```rust
pub enum AttentionCategory {
    /// Calendar event or deadline.
    Calendar,
    /// Pending task or overdue item.
    Task,
    /// System health issue (disk, memory, service status).
    SystemHealth,
    /// Knowledge graph consistency anomaly detected via multi-path retrieval.
    MemoryAnomaly,
    /// Application-defined attention category.
    Custom(String),
}
```

```rust
pub enum Urgency {
    /// Informational, no action needed soon.
    Low,
    /// Should be addressed within the current session.
    Medium,
    /// Needs attention soon (within hours).
    High,
    /// Requires immediate action.
    Critical,
}
```

```rust
impl ProsocheCheck {
    pub async fn run (&self) -> crate::error::Result<ProsocheResult>;
}
```

## `src/prosoche_audit.rs`

```rust
pub enum ProsocheCheckKind {
    /// Detect contradictions between facts in the knowledge store (X and not-X).
    Consistency,
    /// Identify facts or sessions that haven't been touched in N days without rationale.
    Staleness,
    /// Verify recent session turns are advancing stated goals.
    GoalAlignment,
    /// Evaluate whether sessions produce actionable outcomes (error rate, completion rate).
    SessionQuality,
    /// Detect recurring patterns in agent behavior (loops, avoidance, over-confidence).
    ///
    /// v1: stub — defines the trait shape. Full semantics need gnomon weights.
    /// Tracked in follow-up issue.
    InstinctPatterns,
}
```

```rust
pub struct ProsocheState {
    /// The nous identity this audit is running for.
    // kanon:ignore RUST/primitive-for-domain-id — ProsocheState is an ephemeral audit input struct; nous_id is passed as &str from the runner and converted to String for serialization
    pub nous_id: String,
    /// Known stated goals for this nous (free-text lines).
    ///
    /// Used by [`GoalAlignmentCheck`] for keyword overlap.
    pub stated_goals: Vec<String>,
    /// Recent session summaries (id, turn count, error count, completed flag).
    ///
    /// Used by [`SessionQualityCheck`] and [`GoalAlignmentCheck`].
    pub sessions: Vec<SessionSnapshot>,
    /// Recent facts for consistency and staleness checks.
    ///
    /// Each entry is `(fact_id, content, last_touched_days_ago)`.
    pub facts: Vec<FactSnapshot>,
    /// Current UTC timestamp (ISO 8601), set at audit start.
    pub checked_at: String,
}
```

```rust
pub struct FactSnapshot {
    /// Stable fact identifier.
    // kanon:ignore RUST/primitive-for-domain-id — FactSnapshot is an ephemeral audit input; fact_id comes from external knowledge graph facts as raw strings
    pub fact_id: String,
    /// Full text content of the fact.
    pub content: String,
    /// How many days ago this fact was last accessed or updated.
    ///
    /// A value of `None` means last-access is unknown.
    pub days_since_touched: Option<f64>,
}
```

```rust
pub struct SessionSnapshot {
    /// Session identifier.
    // kanon:ignore RUST/primitive-for-domain-id — SessionSnapshot is an ephemeral audit input; session_id comes from external session metadata as raw strings
    pub session_id: String,
    /// Total turn count in the session.
    pub turn_count: u32,
    /// Number of turns that resulted in an error response.
    pub error_count: u32,
    /// Whether the session reached a natural completion (vs. abandoned).
    pub completed: bool,
    /// Combined text of all user turns in this session.
    ///
    /// Used for goal-alignment keyword matching.
    pub turn_text: String,
}
```

> A single prosoche self-audit check.
> 
> Implementations receive a [`ProsocheState`] snapshot and produce zero or
> more [`Finding`]s describing attention-quality issues. Each finding carries
> its own [`EvidenceLevel`] so consumers can weight the results appropriately.
> 
> # Implementation contract
> 
> - Checks MUST be stateless: all input is in [`ProsocheState`].
> - Checks MUST NOT panic. Return an empty `Vec` on errors and log via `tracing`.
> - Checks SHOULD log one `tracing::info!` per invocation with `findings_count`.
> - Checks SHOULD be fast (<100ms). Long-running analysis belongs in a separate
>   maintenance task, not a prosoche check.
> 
> # Object safety
> 
> `check` returns `Pin<Box<dyn Future>>` so the trait is object-safe and
> `Arc<dyn ProsocheCheck>` works without `async_trait`. Pattern mirrors
> [`crate::bridge::DaemonBridge`].
```rust
pub trait ProsocheCheck : Send + Sync {
    fn check <'a> (
        &'a self,
        state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>>;
    fn kind (&self) -> ProsocheCheckKind;
}
```

> Detect contradictions in the fact graph.
> 
> v1 heuristic: looks for fact pairs where one fact content contains a term
> and another contains "not <term>" or "never <term>". This catches obvious
> logical contradictions without requiring symbolic reasoning.
> 
> Future: integrate with episteme's A-MemGuard consensus layer for full
> multi-path contradiction detection.
```rust
pub struct ConsistencyCheck;
```

> Flag facts and sessions that haven't been touched in N days without a
> documented rationale.
> 
> v1 thresholds:
> - Facts untouched > 90 days: `Exploratory` finding.
> - Incomplete sessions with > 10 turns: `Interpretive` finding.
```rust
pub struct StalenessCheck {
    /// Number of days after which an unaccessed fact is considered stale.
    pub fact_stale_days: f64,
}
```

> Verify recent session turns are advancing stated goals.
> 
> v1: keyword-overlap heuristic. For each session, count how many of the
> stated-goal keywords appear in the session's turn text. Sessions with
> zero overlap with any goal produce an `Interpretive` finding.
> 
> Limitation: keyword matching does not capture semantic alignment. A session
> about "implement the authentication system" may advance the goal
> "ship secure login" without sharing keywords.
```rust
pub struct GoalAlignmentCheck;
```

> Flag sessions with high error rates or only abandoned turns.
> 
> v1 thresholds:
> - Error rate > 50% of turns: `Exploratory` finding.
> - Zero completed sessions in the snapshot AND ≥ 5 sessions: `Interpretive` finding.
```rust
pub struct SessionQualityCheck {
    /// Error rate threshold above which a session is flagged (0.0–1.0).
    pub error_rate_threshold: f64,
    /// Minimum session length (in turns) before quality check is applied.
    ///
    /// Very short sessions are excluded to avoid flagging legitimate 1-turn interactions.
    pub min_turns: u32,
}
```

> Detect recurring patterns in agent behavior.
> 
> v1: stub implementation. The trait shape and variant are correct; the
> pattern detection logic requires gnomon behavioral weights and a session
> history longer than what's available in `ProsocheState`.
> 
> # Follow-up
> 
> Full implementation tracked separately. When gnomon weights are available:
> 1. Sample the last N session summaries.
> 2. Run behavioural pattern detection (loop detection, avoidance bias,
>    over-confidence in tool selection).
> 3. Emit `Speculative` findings for patterns that exceed a threshold.
```rust
pub struct InstinctPatternsCheck;
```

> Persistence backend for audit reports.
> 
> v1: writes JSON files to a directory on disk. Each audit run produces
> a timestamped file: `prosoche-audit-<ISO8601>.json`.
```rust
pub struct AuditStorage {
    /// Directory where audit reports are written.
    pub dir: PathBuf,
}
```

```rust
impl AuditStorage {
    pub fn new (dir: impl Into<PathBuf>) -> Self;
    pub fn persist (&self, report: &AuditReport) -> std::io::Result<PathBuf>;
}
```

```rust
pub struct AuditReport {
    /// ISO 8601 timestamp when this audit ran.
    pub audited_at: String,
    /// The nous identity this audit covers.
    // kanon:ignore RUST/primitive-for-domain-id — AuditReport is a serialization envelope; nous_id is copied from the input state for provenance, not a cross-crate domain ID
    pub nous_id: String,
    /// All findings from all checks, in check order.
    pub findings: Vec<Finding>,
    /// Summary counts by check kind.
    pub check_summary: Vec<CheckSummary>,
    /// Provenance metadata (producer, schema version, counts).
    pub meta: ArtefactMeta,
}
```

```rust
pub struct CheckSummary {
    /// Which check produced these findings.
    pub kind: ProsocheCheckKind,
    /// How many findings this check produced.
    pub findings_count: usize,
}
```

> Runs all registered prosoche checks and persists the resulting findings.
> 
> # Usage
> 
> Build the runner with [`ProsocheAuditRunner::default_checks`], then call
> [`ProsocheAuditRunner::run_audit`] at the heartbeat cadence. The runner is
> wired into the existing 5-minute heartbeat via [`crate::execution`]'s
> `SelfAudit` builtin arm  -  it does NOT create a new timer.
> 
> ```text
> BuiltinTask::SelfAudit
>   └─ ProsocheAuditRunner::run_audit(&state)
>        ├─ ConsistencyCheck::check()
>        ├─ StalenessCheck::check()
>        ├─ GoalAlignmentCheck::check()
>        ├─ SessionQualityCheck::check()
>        └─ InstinctPatternsCheck::check()  (stub)
> ```
```rust
pub struct ProsocheAuditRunner {
    /// Ordered list of checks to run.
    checks: Vec<Arc<dyn ProsocheCheck>>,
    /// Persistence backend for audit reports.
    storage: AuditStorage,
}
```

```rust
impl ProsocheAuditRunner {
    pub fn default_checks (audit_dir: impl AsRef<Path>) -> Self;
    pub fn new (checks: Vec<Arc<dyn ProsocheCheck>>, storage: AuditStorage) -> Self;
    pub async fn run_audit (&self, state: &ProsocheState) -> AuditReport;
}
```

## `src/runner/inflight.rs`

```rust
impl TaskRunner {
    pub fn status (&self) -> Vec<TaskStatus>;
}
```

## `src/runner/lifecycle.rs`

```rust
impl TaskRunner {
    pub async fn run (&mut self);
}
```

## `src/runner/mod.rs`

```rust
pub enum DaemonOutputMode {
    /// Full output  -  all tool results and model responses logged verbatim.
    #[default]
    Full,
    /// Brief output  -  tool results truncated to first/last N lines, model
    /// responses logged at info level with truncation.
    Brief,
}
```

> Per-nous background task runner.
```rust
pub struct TaskRunner {
    nous_id: String,
    tasks: Vec<RegisteredTask>,
    shutdown: CancellationToken,
    bridge: Option<Arc<dyn DaemonBridge>>,
    maintenance: Option<MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
    /// In-flight tasks: `task_id` → [`InFlightTask`].
    in_flight: HashMap<String, InFlightTask>,
    /// Optional fjall-backed state store for cross-restart persistence.
    state_store: Option<crate::state::TaskStateStore>,
    /// Output mode: full or brief (truncated).
    output_mode: DaemonOutputMode,
    /// Deployment-tunable daemon behavior.
    daemon_behavior: DaemonBehaviorConfig,
    /// Self-prompt rate limiter (tracks per-agent dispatch counts).
    self_prompt_limiter: crate::self_prompt::SelfPromptLimiter,
    /// Self-prompt configuration (enabled, rate limits).
    self_prompt_config: crate::self_prompt::SelfPromptConfig,
}
```

```rust
pub struct ExecutionResult {
    /// Whether the task completed without error.
    pub success: bool,
    /// Task output or diagnostic message.
    pub output: Option<String>,
}
```

```rust
impl TaskRunner {
    pub fn new (nous_id: impl Into<String>, shutdown: CancellationToken) -> Self;
    pub fn with_bridge (
        nous_id: impl Into<String>,
        shutdown: CancellationToken,
        bridge: Arc<dyn DaemonBridge>,
    ) -> Self;
    pub fn with_maintenance (mut self, mut config: MaintenanceConfig) -> Self;
    pub fn with_retention (
        mut self,
        executor: Arc<dyn crate::maintenance::RetentionExecutor>,
    ) -> Self;
    pub fn with_knowledge_maintenance (
        mut self,
        executor: Arc<dyn KnowledgeMaintenanceExecutor>,
    ) -> Self;
    pub fn with_after_action_store (
        mut self,
        store: Arc<aletheia_routing::AfterActionStore>,
    ) -> Self;
    pub fn with_state_store (mut self, store: crate::state::TaskStateStore) -> Self;
    pub fn with_output_mode (mut self, mode: DaemonOutputMode) -> Self;
    pub fn with_daemon_behavior (mut self, behavior: DaemonBehaviorConfig) -> Self;
    pub fn with_self_prompt (mut self, config: crate::self_prompt::SelfPromptConfig) -> Self;
    pub fn register_top_issue_self_prompt (
        &mut self,
        issues: &[crate::self_prompt::OpenIssue],
        schedule: Schedule,
    ) -> Option<String>;
}
```

## `src/runner/registration.rs`

```rust
impl TaskRunner {
    pub fn register_maintenance_tasks (&mut self);
    pub fn register (&mut self, task: TaskDef);
}
```

## `src/runner/systemd.rs`

> Send `READY=1` to systemd via the `$NOTIFY_SOCKET`.
> 
> WHY: systemd `Type=notify` services need this to know initialization is
> complete. No-op if `$NOTIFY_SOCKET` is not SET.
```rust
pub fn sd_notify_ready ()
```

> Send `WATCHDOG=1` to systemd.
> 
> WHY: `WatchdogSec` integration enables automatic restart on hang.
```rust
pub fn sd_notify_watchdog ()
```

> Send `STOPPING=1` to systemd before shutdown cleanup.
```rust
pub fn sd_notify_stopping ()
```

> Parse `$WATCHDOG_USEC` to determine the systemd watchdog interval.
> 
> Returns `None` if the variable is not SET or unparseable. The recommended
> notification interval is half the watchdog timeout.
```rust
pub fn sd_watchdog_interval () -> Option<Duration>
```

## `src/schedule.rs`

```rust
pub enum Schedule {
    /// Cron expression (e.g., `"0 */45 8-23 * * *"` for every 45min 8am-11pm).
    Cron(String),
    /// Fixed interval.
    Interval(Duration),
    /// Run once at a specific time.
    Once(jiff::Timestamp),
    /// Run once at startup.
    Startup,
}
```

```rust
pub struct TaskDef {
    /// Unique task identifier.
    // kanon:ignore RUST/primitive-for-domain-id — TaskDef::id is a user-configured cron task identifier from TOML/JSON, not a typed domain entity ID
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Which nous this task belongs to.
    // kanon:ignore RUST/primitive-for-domain-id — TaskDef::nous_id is a string reference to a runtime nous actor name, configured externally
    pub nous_id: String,
    /// When to run.
    pub schedule: Schedule,
    /// What to run.
    pub action: TaskAction,
    /// Whether the task is currently enabled.
    pub enabled: bool,
    /// Active time window (optional): `(start_hour, end_hour)` in local time.
    pub active_window: Option<(u8, u8)>,
    /// Maximum duration a task may run before being considered hung.
    /// Default: 5 minutes.
    pub timeout: Duration,
    /// Whether to catch up missed cron windows on startup (within last 24h).
    /// Default: true for maintenance tasks, false for prosoche.
    pub catch_up: bool,
    /// Maximum jitter to add to computed next-fire times.
    ///
    /// WHY: jitter prevents thundering-herd when multiple tasks share the same
    /// cron expression. The actual jitter is deterministic, seeded FROM the task
    /// ID hash, so it is stable across restarts.
    pub jitter: Option<jiff::SignedDuration>,
}
```

```rust
pub enum TaskAction {
    /// Execute a shell command.
    Command(String),
    /// Run a built-in maintenance function.
    Builtin(BuiltinTask),
    /// Send a daemon-generated prompt to the nous.
    SelfPrompt(String),
}
```

```rust
pub enum BuiltinTask {
    /// Prosoche attention check.
    Prosoche,
    /// Rotate and compress old trace files.
    TraceRotation,
    /// Compare instance against template for configuration drift.
    DriftDetection,
    /// Monitor database file sizes against thresholds.
    DbSizeMonitor,
    /// Execute data retention policy cleanup.
    RetentionExecution,
    /// Refresh temporal decay scores for knowledge graph entities/edges.
    DecayRefresh,
    /// Find and merge duplicate entities in the knowledge graph.
    EntityDedup,
    /// Recompute graph-wide scores (`PageRank`, centrality, etc.).
    GraphRecompute,
    /// Re-embed entities whose embeddings are stale or missing.
    EmbeddingRefresh,
    /// Remove orphaned nodes, expired edges, and other detritus.
    KnowledgeGc,
    /// Rebuild or optimize knowledge graph indexes.
    IndexMaintenance,
    /// Run a diagnostic health check on the knowledge graph.
    GraphHealthCheck,
    /// Compute decay scores for skills and retire stale ones.
    SkillDecay,
    /// Run self-audit checks and store results in the knowledge graph.
    SelfAudit,
    /// Run adversarial self-probing: consistency, boundary, and recall probes.
    ///
    /// WHY: `SelfAudit` dispatches a generic introspection prompt to the nous;
    /// `ProbeAudit` dispatches a structured probe set that evaluates specific
    /// behavioral invariants (injection resistance, factual consistency, recall
    /// fidelity). Silent capability drift is detected before QA surfaces it.
    ProbeAudit,
    /// Periodic configuration variant search: mutate and benchmark agent pipeline configs.
    EvolutionSearch,
    /// Periodic self-reflection: agent evaluates recent performance.
    SelfReflection,
    /// Periodic knowledge graph cleanup: orphan removal and stale entity pruning.
    GraphCleanup,
    /// Extract operational metrics as knowledge graph facts.
    OpsFactExtraction,
    /// Extract lessons from training data (violations/lint JSONL) into knowledge graph.
    LessonExtraction,
    /// Self-prompt: daemon-initiated follow-up action from a prosoche check.
    SelfPrompt,
    /// Analyze recent session observations and write candidate lint rule proposals.
    ProposeRules,
    /// Periodic file-level backup of the fjall knowledge store.
    FjallBackup,
    /// Prune prompt audit log daily files past the retention window (#3411).
    PromptAuditRotation,
    /// Refresh empirical after-action routing statistics from JSONL logs.
    RoutingStoreRefresh,
}
```

```rust
pub struct TaskStatus {
    /// Unique task identifier.
    // kanon:ignore RUST/primitive-for-domain-id — TaskStatus::id mirrors TaskDef::id, a user-configured string identifier
    pub id: String,
    /// Human-readable task name.
    pub name: String,
    /// Whether the task is currently enabled (disabled after consecutive failures).
    pub enabled: bool,
    /// When the task is next scheduled to run (ISO 8601).
    pub next_run: Option<String>,
    /// When the task last ran (ISO 8601).
    pub last_run: Option<String>,
    /// Total successful executions.
    pub run_count: u64,
    /// Current streak of consecutive failures (resets on success).
    pub consecutive_failures: u32,
    /// Whether the task is currently in flight.
    pub in_flight: bool,
    /// Most recent error message, if the last execution failed. (#2212)
    pub last_error: Option<String>,
}
```

## `src/self_prompt.rs`

> Session key used for all self-prompt dispatches.
> 
> WHY: separate session key prevents self-prompts from interfering with user
> sessions. Users can check `daemon:self-prompt` when they want to review
> autonomous actions.
```rust
pub const SELF_PROMPT_SESSION_KEY: &str = "daemon:self-prompt";
```

```rust
pub struct OpenIssue {
    /// Issue number in the tracker.
    pub number: u64,
    /// Issue title.
    pub title: String,
    /// Issue body, if the tracker provided one.
    #[serde(default)]
    pub body: String,
}
```

```rust
pub struct IssuePromptTask {
    /// Stable daemon task id.
    // kanon:ignore RUST/primitive-for-domain-id — IssuePromptTask::id is a synthetic daemon task identifier derived from an issue number, not a domain entity ID
    pub id: String,
    /// Human-readable task name.
    pub name: String,
    /// Prompt sent to the nous.
    pub prompt: String,
}
```

> Parse tracker JSON and retain only open issues.
> 
> The accepted shape is a JSON array of objects with `number`, `title`,
> optional `body`, and optional `state`; missing `state` is treated as open
> so tests and simple local snapshots can use a minimal issue shape.
```rust
pub fn parse_open_issues_json (input: &str) -> Result<Vec<OpenIssue>, serde_json::Error>
```

```rust
pub fn prompt_task_from_top_open_issue (issues: &[OpenIssue]) -> Option<IssuePromptTask>
```

```rust
pub struct SelfPromptConfig {
    /// Whether self-prompting is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Maximum self-prompts per hour per agent.
    ///
    /// WHY: rate limit prevents runaway loops. A prosoche check that always
    /// produces a `## Follow-up` section would otherwise generate unbounded work.
    #[serde(default = "default_max_per_hour")]
    pub max_per_hour: u32,
}
```

## `src/state/fjall_store.rs`

> Fjall-backed store for task execution state.
> 
> One keyspace directory holds state for all tasks in a runner.
> Uses `SingleWriterTxDatabase` for durability without WAL complexity.
```rust
pub struct TaskStateStore {
    db: SingleWriterTxDatabase,
}
```

```rust
impl TaskStateStore {
    pub fn open (path: &Path) -> Result<Self>;
    pub fn load_all (&self) -> Result<Vec<TaskState>>;
    pub fn save (&self, state: &TaskState) -> Result<()>;
}
```

## `src/state.rs`

```rust
pub struct TaskState {
    /// Task ID matching `TaskDef::id`.
    // kanon:ignore RUST/primitive-for-domain-id — TaskState::task_id mirrors TaskDef::id, a user-configured cron task identifier persisted as raw string
    pub task_id: String,
    /// ISO 8601 timestamp of the last execution (success or failure).
    pub last_run_ts: Option<String>,
    /// Total completed executions.
    pub run_count: u64,
    /// Consecutive failures since the last success.
    pub consecutive_failures: u32,
}
```

```rust
pub struct DaemonConfig {
    /// Whether daemon mode is enabled for this workspace.
    #[serde(default)]
    pub enabled: bool,

    /// Reserved child-agent concurrency limit.
    ///
    /// The current runtime stores this value but does not spawn child agents.
    #[serde(default = "default_max_children")]
    pub max_children: usize,

    /// Allowed trigger types for this workspace.
    #[serde(default)]
    pub allowed_triggers: AllowedTriggers,

    /// Allowed builtin task IDs. Empty means all registered tasks are allowed.
    #[serde(default)]
    pub allowed_tasks: Vec<String>,

    /// Reserved webhook listener port. The current runtime does not start a
    /// webhook listener.
    #[serde(default)]
    pub webhook_port: Option<u16>,

    /// Reserved file watch paths (relative to workspace root). The current
    /// runtime does not start file watchers.
    #[serde(default)]
    pub watch_paths: Vec<String>,

    /// Brief output mode: truncate tool results and model responses in logs.
    #[serde(default)]
    pub brief_output: bool,

    /// Self-prompting configuration: daemon-initiated follow-up actions.
    ///
    /// WHY: self-prompting must be opt-in. Without explicit enablement, the
    /// daemon never sends itself follow-up prompts. Rate limiting prevents
    /// runaway loops from misconfigured attention checks.
    #[serde(default)]
    pub self_prompt: crate::self_prompt::SelfPromptConfig,
}
```

```rust
pub struct AllowedTriggers {
    /// Reserved file-watcher trigger opt-in.
    #[serde(default)]
    pub file_watch: bool,
    /// Reserved webhook trigger opt-in.
    #[serde(default)]
    pub webhook: bool,
}
```

```rust
impl DaemonConfig {
    pub fn load (workspace_root: &Path) -> Result<Self>;
    pub fn is_task_allowed (&self, task_id: &str) -> bool;
}
```

> Single-instance lock guard for daemon process per workspace.
> 
> WHY: only one daemon process should run per workspace. We use
> `rustix::fs::flock` directly on the lock file's file descriptor.
> 
> The lock file is created at `.aletheia/daemon.lock` in the workspace root.
> The advisory lock is bound to the file descriptor and held for as long as
> `WorkspaceGuard` (and therefore the inner `File`) lives. Drop closes the
> file descriptor, which releases the flock automatically.
> 
> # Bug history
> 
> Previously this used `fd_lock::RwLock<File>` and probed with `try_write()`,
> then immediately dropped the resulting `RwLockWriteGuard`. This was a bug:
> `RwLockWriteGuard::drop` calls `flock(fd, LOCK_UN)`, releasing the lock
> before the `WorkspaceGuard` was even returned to the caller. Two
> `acquire()` calls in the same process would both succeed. Tracked in #3026.
```rust
pub struct WorkspaceGuard {
    /// The lock file. Holding this open keeps the flock alive on the
    /// associated file descriptor; closing it releases the flock.
    _file: std::fs::File,
    path: PathBuf,
}
```

```rust
impl WorkspaceGuard {
    pub fn acquire (workspace_root: &Path) -> Result<Self>;
    pub fn lock_path (&self) -> &Path;
}
```

## `src/triggers.rs`

```rust
pub struct TriggerRouter {
    _private: (),
}
```

```rust
impl TriggerRouter {
    pub fn new () -> Self;
}
```

## `src/watchdog.rs`

```rust
pub struct WatchdogConfig {
    /// Maximum time without a heartbeat before a process is declared hung.
    pub heartbeat_timeout: Duration,
    /// How often the watchdog sweeps for hung processes.
    pub check_interval: Duration,
    /// Maximum number of restart attempts before giving up.
    pub max_restarts: u32,
    /// Base duration for restart backoff.
    pub backoff_base: Duration,
    /// Maximum restart backoff duration.
    pub backoff_cap: Duration,
}
```

```rust
impl WatchdogConfig {
    pub fn with_daemon_behavior (mut self, behavior: &taxis::config::DaemonBehaviorConfig) -> Self;
}
```

```rust
pub enum ProcessState {
    /// Process is healthy and sending heartbeats.
    Healthy,
    /// Process missed heartbeat deadline and is considered hung.
    Hung,
    /// Process is being restarted.
    Restarting,
    /// Process exceeded max restarts and is abandoned.
    Abandoned,
}
```

```rust
pub struct RestartEvent {
    /// Process identifier.
    // kanon:ignore RUST/primitive-for-domain-id — RestartEvent::process_id is a runtime process handle identifier string, not a typed domain entity ID
    pub process_id: String,
    /// What triggered the restart.
    pub cause: RestartCause,
    /// Which restart attempt this is (1-indexed).
    pub attempt: u32,
    /// ISO 8601 timestamp of the event.
    pub timestamp: String,
}
```

```rust
pub enum RestartCause {
    /// No heartbeat received within the configured timeout.
    HeartbeatTimeout {
        /// Seconds since the last heartbeat.
        elapsed_secs: u64,
    },
    /// The process exited unexpectedly.
    ProcessExited {
        /// Exit reason or error message.
        reason: String,
    },
}
```

```rust
pub struct ProcessStatus {
    /// Process identifier.
    // kanon:ignore RUST/primitive-for-domain-id — ProcessStatus::id is a runtime process handle identifier string, not a typed domain entity ID
    pub id: String,
    /// Current state.
    pub state: ProcessState,
    /// Seconds since last heartbeat.
    pub last_heartbeat_secs: u64,
    /// Total number of restarts performed.
    pub restart_count: u32,
}
```

```rust
impl Watchdog {
    pub async fn run (&mut self);
}
```

## `tests/common/mod.rs`

> Write a fixture file synchronously via `OpenOptions` + `Write`.
> 
> WHY: the daemon crate's `clippy.toml` disallows `std::fs::write` to steer
> production code toward `tokio::fs`. Integration tests still inherit that
> clippy config. Using explicit `File::create` + `write_all` is equivalent
> and keeps the lint clean.
```rust
pub fn write_fixture (path: impl AsRef<Path>, bytes: impl AsRef<[u8]>)
```

> Build a minimal `TaskRunner` bound to a throw-away cancellation token.
```rust
pub fn make_runner (nous_id: &str) -> TaskRunner
```
