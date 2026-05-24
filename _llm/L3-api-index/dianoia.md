# L3 API Index: dianoia

Crate path: `crates/dianoia`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// A phase gate blocked a state transition because one or more conditions were not satisfied.
    #[snafu(display(
        "gate blocked transition from {state:?}: unsatisfied conditions: {conditions:?}"
    ))]
    GateBlocked {
        /// The state from which the transition was attempted.
        state: crate::state::ProjectState,
        /// The condition keys that were not satisfied.
        conditions: Vec<String>,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// A state transition was attempted that is not valid from the current state.
    #[snafu(display("invalid transition {transition:?} from state {state:?}"))]
    InvalidTransition {
        /// The current project state.
        state: crate::state::ProjectState,
        /// The transition that was attempted.
        transition: crate::state::Transition,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Requested phase does not exist in the project.
    #[snafu(display("phase not found: {phase_id}"))]
    PhaseNotFound {
        /// The phase identifier that was not found.
        phase_id: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Requested plan does not exist in the phase.
    #[snafu(display("plan not found: {plan_id}"))]
    PlanNotFound {
        /// The plan identifier that was not found.
        plan_id: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// A plan exceeded its maximum iteration count without completing.
    #[snafu(display("plan {plan_id} stuck after {iterations} iterations"))]
    PlanStuck {
        /// The plan identifier that got stuck.
        plan_id: String,
        /// Number of iterations completed before the limit was hit.
        iterations: u32,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Circular dependency detected among plan `depends_on` relationships.
    #[snafu(display("circular dependency detected: {cycle}"))]
    CircularDependency {
        /// Human-readable cycle path, e.g. "A -> B -> A".
        cycle: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Filesystem operation failed in the project workspace.
    #[snafu(display("workspace I/O error at {}", path.display()))]
    WorkspaceIo {
        /// The path at which the I/O error occurred.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to deserialize a project file from JSON.
    #[snafu(display("workspace deserialization error"))]
    WorkspaceDeserialize {
        /// The underlying deserialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to serialize a project to JSON.
    #[snafu(display("workspace serialization error"))]
    WorkspaceSerialize {
        /// The underlying serialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// No project file exists at the expected workspace path.
    #[snafu(display("project not found in workspace at {}", path.display()))]
    ProjectNotFound {
        /// The path at which the project file was expected.
        path: PathBuf,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Filesystem error during handoff file operations.
    #[snafu(display("handoff I/O error at {}", path.display()))]
    HandoffIo {
        /// The path at which the I/O error occurred.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to deserialize a handoff file from JSON.
    #[snafu(display("handoff deserialization error"))]
    HandoffDeserialize {
        /// The underlying deserialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to serialize handoff context to JSON.
    #[snafu(display("handoff serialization error"))]
    HandoffSerialize {
        /// The underlying serialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}
```

## `src/gate.rs`

```rust
pub enum GateCondition {
    /// All tests in the CI suite are currently passing.
    TestsPassing,
    /// No lint errors or warnings are reported.
    LintClean,
    /// Relevant documentation has been updated.
    DocsUpdated,
    /// At least one reviewer has approved the work.
    ReviewApproved,
    /// A named custom condition (description is the key checked against [`PhaseGate::satisfied`]).
    Custom(String),
}
```

```rust
pub enum GateResult {
    /// All conditions are satisfied; the transition may proceed.
    Pass,
    /// One or more conditions are not satisfied.
    Fail(Vec<String>),
}
```

```rust
impl GateResult {
    pub fn is_pass (&self) -> bool;
}
```

```rust
pub struct PhaseGate {
    /// The project state this gate guards (the state being *left*).
    pub from: ProjectState,
    /// Conditions that must all be satisfied before advancing.
    pub conditions: Vec<GateCondition>,
    /// Condition keys that have been marked satisfied by external systems.
    pub satisfied: Vec<String>,
}
```

```rust
impl PhaseGate {
    pub fn new (from: ProjectState, conditions: Vec<GateCondition>) -> Self;
    pub fn mark_satisfied (&mut self, key: impl Into<String>);
}
```

```rust
pub fn evaluate_gate (gate: &PhaseGate) -> GateResult
```

```rust
pub fn default_gate (from: &ProjectState) -> Option<PhaseGate>
```

## `src/handoff.rs`

```rust
pub enum HandoffReason {
    /// Context distillation is about to compress the conversation.
    Distillation,
    /// The process is shutting down in a controlled manner.
    ControlledShutdown,
    /// The context window is approaching its limit.
    ContextLimitApproaching,
}
```

```rust
pub struct HandoffContext {
    /// Description of the current task being worked on.
    pub task: String,
    /// Progress made so far (milestones, completed steps).
    pub progress: Vec<String>,
    /// What needs to happen next to continue the work.
    pub next_steps: Vec<String>,
    /// File paths relevant to the current work.
    pub relevant_paths: Vec<PathBuf>,
    /// Partial results or intermediate outputs worth preserving.
    pub partial_results: Vec<String>,
    /// Project identifier, if operating within a project context.
    pub project_id: Option<String>,
    /// Session identifier for the originating session.
    pub session_id: Option<String>,
    /// Why the handoff was triggered.
    pub reason: HandoffReason,
    /// When the handoff was created.
    pub created_at: jiff::Timestamp,
}
```

## `src/intent.rs`

```rust
pub enum ConvictionTier {
    /// Must do: governs every autonomous decision in scope.
    ///
    /// Only the operator may add Directives. The nous treats them as inviolable
    /// constraints, not suggestions to weigh.
    Directive = 2,
    /// Should do: strong preference the nous applies unless blocked.
    ///
    /// Only the operator may add Preferences. The nous respects them unless a
    /// Directive or hard constraint requires otherwise.
    Preference = 1,
    /// May do: weak guidance the nous applies when convenient.
    ///
    /// Both operator and nous may add Suggestions. They act as lightweight
    /// nudges that can be overridden by any harder constraint or better option.
    Suggestion = 0,
}
```

```rust
pub enum IntentSource {
    /// Added by the human operator via explicit instruction.
    Operator,
    /// Added by the nous itself during autonomous operation.
    Nous,
}
```

```rust
pub struct Intent {
    /// Unique intent identifier.
    pub id: Ulid,
    /// Human-readable description of what the intent governs.
    ///
    /// Example: "Prioritise kanon fixes over new features until Phase 04b lands."
    pub description: String,
    /// How strongly this intent governs autonomous behaviour.
    pub conviction_tier: ConvictionTier,
    /// Who created this intent.
    pub source: IntentSource,
    /// When the intent was created.
    pub created_at: jiff::Timestamp,
    /// When the intent expires, if ever.
    ///
    /// `None` means the intent is open-ended and must be explicitly resolved.
    pub expires_at: Option<jiff::Timestamp>,
    /// Whether the intent has been explicitly resolved.
    ///
    /// Resolved intents are retained for audit purposes but excluded from the
    /// active set consulted during bootstrap.
    pub resolved: bool,
}
```

```rust
pub struct IntentError {
    message: String,
}
```

```rust
impl Intent {
    pub fn new (
        description: String,
        conviction_tier: ConvictionTier,
        source: IntentSource,
        expires_at: Option<jiff::Timestamp>,
    ) -> std::result::Result<Self, IntentError>;
}
```

> Persistent store for operator intents.
> 
> Backed by `instance/nous/<agent>/intents.json`. All mutating operations
> write through to disk immediately  -  there is no in-memory cache to go stale.
> 
> Intents are not subject to FSRS decay. They persist until explicitly resolved
> or their `expires_at` timestamp passes.
```rust
pub struct IntentStore {
    path: PathBuf,
}
```

```rust
impl IntentStore {
    pub fn new (agent_dir: impl Into<PathBuf>) -> Self;
    pub fn path_for (instance_root: &Path, agent_id: &str) -> PathBuf;
    pub fn add_intent (&self, intent: Intent) -> Result<Intent>;
    pub fn list_intents (&self) -> Result<Vec<Intent>>;
    pub fn expire_intents (&self) -> Result<usize>;
    pub fn resolve_intent (&self, id: Ulid) -> Result<bool>;
    pub fn render_for_bootstrap (&self) -> Result<Option<String>>;
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

> Force-initialize all lazy metric statics.
> 
> Primarily a compatibility shim for the binary crate's startup; prefer
> [`register`] which installs the families into a shared registry.
```rust
pub fn init ()
```

## `src/phase.rs`

```rust
pub struct Phase {
    /// Unique phase identifier.
    pub id: Ulid,
    /// Human-readable phase name (e.g., "Foundation", "Core Features").
    pub name: String,
    /// What this phase aims to accomplish.
    pub goal: String,
    /// Preconditions that must hold before this phase can begin.
    pub requirements: Vec<String>,
    /// Executable plans within this phase.
    pub plans: Vec<Plan>,
    /// Current lifecycle state.
    pub state: PhaseState,
    /// Ordering position within the project (lower runs first).
    pub order: u32,
}
```

```rust
pub enum PhaseState {
    /// Phase has not started yet.
    Pending,
    /// Phase is active and plans are being prepared.
    Active,
    /// Plans within this phase are being executed.
    Executing,
    /// Execution is done, verifying outcomes.
    Verifying,
    /// All plans completed successfully.
    Complete,
    /// Phase failed, with a flag indicating whether retry is possible.
    Failed {
        /// Whether the phase can be retried after failure.
        can_retry: bool,
    },
}
```

```rust
impl Phase {
    pub fn new (name: String, goal: String, order: u32) -> Self;
    pub fn add_plan (&mut self, plan: Plan);
}
```

## `src/plan.rs`

> Default maximum planning iterations.
> 
> Callers with access to the resolved taxis config should use
> `AgentBehaviorDefaults::planning_max_iterations` instead of this fallback.
```rust
pub const DEFAULT_MAX_ITERATIONS: u32 = 10;
```

```rust
pub struct Plan {
    /// Unique plan identifier.
    pub id: Ulid,
    /// Short title for the plan.
    pub title: String,
    /// Detailed description of what this plan accomplishes.
    pub description: String,
    /// Execution wave: plans in the same wave can run in parallel.
    pub wave: u32,
    /// Plan IDs that must complete before this plan can start.
    pub depends_on: Vec<Ulid>,
    /// Current lifecycle state.
    pub state: PlanState,
    /// Maximum allowed iterations before the plan is marked stuck.
    pub max_iterations: u32,
    /// Number of iterations executed so far.
    pub iterations: u32,
    /// Blockers discovered during execution.
    pub blockers: Vec<Blocker>,
    /// Notable accomplishments recorded during execution.
    pub achievements: Vec<String>,
}
```

```rust
pub enum PlanState {
    /// Plan has not been evaluated for readiness yet.
    Pending,
    /// All dependencies are satisfied and the plan can execute.
    Ready,
    /// Plan is currently being executed.
    Executing,
    /// Plan finished successfully.
    Complete,
    /// Plan execution failed.
    Failed,
    /// Plan was intentionally skipped (e.g., no longer relevant).
    Skipped,
    /// Plan exceeded its iteration limit without completing.
    Stuck,
}
```

```rust
pub struct Blocker {
    /// What is blocking progress.
    pub description: String,
    /// The plan that is blocked.
    pub plan_id: Ulid,
    /// When the blocker was identified.
    pub detected_at: jiff::Timestamp,
}
```

```rust
impl Plan {
    pub fn new (title: String, description: String, wave: u32) -> Self;
    pub fn from_research (research: &ResearchOutput) -> Vec<Self>;
    pub fn from_template (completed: &Plan, next_wave: u32) -> Self;
    pub fn with_max_iterations (mut self, max: u32) -> Self;
    pub fn set_depends_on (&mut self, deps: Vec<Ulid>, all_plans: &[Plan]) -> Result<()>;
}
```

> Detect circular dependencies in the plan graph.
> 
> Builds an adjacency list from `all_plans` (with `updated_plan` overriding any
> plan with the same ID), then runs DFS-based cycle detection. Returns `Ok(())`
> if the graph is acyclic, or an error describing the first cycle found.
> 
> WHY separate function: called from `Plan::set_depends_on` and available for
> bulk validation of an entire phase's plan set.
```rust
pub fn detect_cycles (all_plans: &[Plan], updated_plan: &Plan) -> Result<()>
```

## `src/project.rs`

```rust
pub struct Project {
    /// Unique project identifier.
    pub id: Ulid,
    /// Human-readable project name.
    pub name: String,
    /// What this project aims to accomplish.
    pub description: String,
    /// Optional scope constraint (e.g., "crate X only").
    pub scope: Option<String>,
    /// Current lifecycle state.
    pub state: ProjectState,
    /// Operating mode controlling planning depth.
    pub mode: ProjectMode,
    /// Ordered phases within this project.
    pub phases: Vec<Phase>,
    /// When the project was created.
    pub created_at: jiff::Timestamp,
    /// When the project was last modified.
    pub updated_at: jiff::Timestamp,
    /// Nous or user that owns this project.
    pub owner: String,
}
```

```rust
pub enum ProjectMode {
    /// Full multi-phase project with research, scoping, planning, and verification.
    Full,
    /// Time-boxed quick task with an appetite limit in minutes.
    Quick {
        /// Maximum number of minutes to spend on this task.
        appetite_minutes: u32,
    },
    /// Autonomous background processing without user interaction.
    Background,
}
```

```rust
impl Project {
    pub fn new (name: String, description: String, mode: ProjectMode, owner: String) -> Self;
    pub fn advance (&mut self, transition: Transition) -> Result<()>;
    pub fn add_phase (&mut self, phase: Phase);
}
```

## `src/reconciler.rs`

```rust
pub struct ProjectSnapshot {
    /// The project state from this source.
    pub project: Project,
    /// Which source this snapshot came from.
    pub origin: SnapshotOrigin,
}
```

```rust
pub enum SnapshotOrigin {
    /// Loaded from the database.
    Database,
    /// Loaded from the filesystem (workspace JSON).
    Filesystem,
}
```

```rust
pub enum ReconciliationDirection {
    /// Database is newer; regenerate filesystem from DB.
    DbToFiles,
    /// Filesystem is newer; import into DB.
    FilesToDb,
    /// Both sources are in sync (within tolerance).
    InSync,
    /// Project exists only in the database.
    DbOnly,
    /// Project exists only on the filesystem.
    FilesOnly,
}
```

```rust
pub struct ConflictEntry {
    /// Which field diverged between the two sources.
    pub field: String,
    /// Value from the database source.
    pub db_value: String,
    /// Value from the filesystem source.
    pub fs_value: String,
    /// Which source won (newest-wins).
    pub resolution: SnapshotOrigin,
}
```

```rust
pub struct ReconciliationResult {
    /// The project ID that was reconciled.
    pub project_id: String,
    /// Which direction the sync went.
    pub direction: ReconciliationDirection,
    /// Conflicts detected between the two sources.
    pub conflicts: Vec<ConflictEntry>,
    /// The winning project state after reconciliation.
    pub resolved: Option<Project>,
    /// Errors encountered during reconciliation.
    pub errors: Vec<String>,
}
```

```rust
pub struct ReconciliationSummary {
    /// Per-project results.
    pub projects: Vec<ReconciliationResult>,
    /// Total number of errors across all projects.
    pub total_errors: usize,
    /// Total number of conflicts detected.
    pub total_conflicts: usize,
}
```

> Default tolerance in seconds for timestamp comparison.
> 
> Callers with access to the resolved taxis config should use
> `AgentBehaviorDefaults::planning_reconciler_timestamp_tolerance_secs` instead.
```rust
pub const DEFAULT_TIMESTAMP_TOLERANCE_SECS: i64 = 5;
```

## `src/research/mod.rs`

```rust
pub enum ResearchDomain {
    /// Technology stack analysis: dependencies, versions, compatibility.
    Stack,
    /// Feature landscape and competitive analysis.
    Features,
    /// System design patterns, crate structure, integration points.
    Architecture,
    /// Known issues, antipatterns, failure modes, gotchas.
    Pitfalls,
}
```

```rust
impl ResearchDomain {
    pub fn as_str (self) -> &'static str;
    pub fn heading (self) -> &'static str;
}
```

```rust
pub enum FindingStatus {
    /// Researcher returned full results.
    Complete,
    /// Researcher returned partial or unvalidated results.
    Partial,
    /// Researcher encountered an error.
    Failed,
    /// Researcher exceeded the timeout.
    TimedOut,
}
```

```rust
pub struct ResearchFinding {
    /// Which domain this finding covers.
    pub domain: ResearchDomain,
    /// The researcher's output text.
    pub content: String,
    /// Whether the researcher completed successfully.
    pub status: FindingStatus,
}
```

```rust
pub struct ResearchConfig {
    /// Timeout per researcher in seconds (default: 300).
    pub timeout_secs: u64,
    /// Which domains to investigate.
    pub domains: Vec<ResearchDomain>,
}
```

```rust
pub struct ResearchOutput {
    /// Individual findings per domain (deduplicated).
    pub findings: Vec<ResearchFinding>,
    /// Unified structured markdown combining all domains.
    pub markdown: String,
}
```

```rust
pub fn domain_prompt (domain: ResearchDomain, project_goal: &str) -> String
```

```rust
pub fn merge_research (findings: Vec<ResearchFinding>) -> ResearchOutput
```

```rust
pub enum ResearchLevel {
    /// Well-understood domain, existing patterns. No research needed.
    Skip,
    /// Mostly understood with a few unknowns. Quick validation (pitfalls only).
    Quick,
    /// New domain or significant complexity. Full 4-dimension research.
    Standard,
    /// Novel architecture, high risk, or unfamiliar domain. Extended research.
    DeepDive,
}
```

```rust
impl ResearchLevel {
    pub fn name (self) -> &'static str;
    pub fn domains (self) -> Vec<ResearchDomain>;
    pub fn needs_synthesis (self) -> bool;
    pub fn to_config (self, timeout_secs: u64) -> ResearchConfig;
}
```

```rust
impl ResearchLevel {
    pub fn depth (self) -> u8;
}
```

```rust
pub struct ComplexitySignals {
    /// Number of requirements in the phase.
    pub requirement_count: u32,
    /// Whether requirements mention unfamiliar technologies.
    pub has_novel_technology: bool,
    /// Whether requirements mention security or auth.
    pub has_security_concerns: bool,
    /// Whether requirements mention data migration.
    pub has_data_migration: bool,
    /// Whether requirements mention external integrations.
    pub has_external_integrations: bool,
    /// Whether the phase goal mentions architectural decisions.
    pub has_architectural_decisions: bool,
    /// Whether similar code exists in the codebase.
    pub has_existing_patterns: bool,
    /// Explicit user override (bypasses auto-detection).
    pub user_override: Option<ResearchLevel>,
}
```

```rust
pub fn select_research_level (signals: &ComplexitySignals) -> ResearchLevel
```

## `src/runtime.rs`

```rust
pub struct PlanningRuntime {
    project: Project,
    stuck_detector: StuckDetector,
}
```

```rust
impl PlanningRuntime {
    pub fn new (project: Project, stuck_config: StuckConfig) -> Self;
    pub fn project (&self) -> &Project;
    pub fn project_mut (&mut self) -> &mut Project;
    pub fn active_phase (&self) -> Option<&Phase>;
    pub fn start_active_phase (&mut self) -> Option<&mut Phase>;
    pub fn record_tool_invocation (&mut self, invocation: ToolInvocation) -> Option<StuckSignal>;
    pub fn reset_stuck_detection (&mut self);
    pub fn stuck_history_len (&self) -> usize;
    pub fn stuck_config (&self) -> &StuckConfig;
    pub fn reconcile_snapshots (
        db_snapshots: &[ProjectSnapshot],
        fs_snapshots: &[ProjectSnapshot],
        tolerance_secs: i64,
    ) -> ReconciliationSummary;
}
```

## `src/state.rs`

```rust
pub enum ProjectState {
    /// Project has been created but work has not started.
    Created,
    /// Gathering clarifying questions about the project goals.
    Questioning,
    /// Researching the domain, codebase, or prior art.
    Researching,
    /// Defining the project scope and boundaries.
    Scoping,
    /// Breaking work into phases and plans.
    Planning,
    /// Reviewing and discussing the plan before execution.
    Discussing,
    /// Actively executing plans.
    Executing,
    /// Verifying that execution outcomes meet acceptance criteria.
    Verifying,
    /// All work completed and verified (terminal).
    Complete,
    /// Project was abandoned (terminal).
    Abandoned,
    /// Project is paused, remembering which state to resume to.
    Paused {
        /// The state to resume to when the project is unpaused.
        previous: Box<ProjectState>,
    },
}
```

```rust
pub enum Transition {
    /// Move from Created to Questioning.
    StartQuestioning,
    /// Skip questioning and research, go directly to Scoping.
    SkipToResearch,
    /// Move from Questioning to Researching.
    StartResearch,
    /// Skip research, go directly to Scoping.
    SkipResearch,
    /// Move from Researching to Scoping.
    StartScoping,
    /// Move from Scoping to Planning.
    StartPlanning,
    /// Move from Planning to Discussing.
    StartDiscussion,
    /// Move from Planning or Discussing to Executing.
    StartExecution,
    /// Move from Executing to Verifying.
    StartVerification,
    /// Move from Verifying to Complete (terminal).
    Complete,
    /// Abandon the project from any non-terminal state (terminal).
    Abandon,
    /// Pause the project, preserving the current state for later resume.
    Pause,
    /// Resume a paused project to its previous state.
    Resume,
    /// Revert from Verifying to an earlier phase (Scoping, Planning, or Executing).
    Revert {
        /// The state to revert to.
        to: ProjectState,
    },
}
```

```rust
impl ProjectState {
    pub fn transition_gated (self, t: Transition, gate: Option<&PhaseGate>) -> Result<Self>;
}
```

## `src/stuck.rs`

```rust
pub struct StuckConfig {
    /// How many identical consecutive errors trigger detection.
    pub repeated_error_threshold: u32,
    /// How many identical consecutive tool+args calls trigger detection.
    pub same_args_threshold: u32,
    /// How many alternating failure cycles trigger detection.
    pub alternating_threshold: u32,
    /// How many scattered retries with the same error trigger detection.
    pub escalating_retry_threshold: u32,
    /// Maximum number of invocations retained in the sliding window.
    pub history_window: usize,
}
```

```rust
pub enum InvocationOutcome {
    /// Tool executed successfully.
    Success,
    /// Tool execution failed with an error message.
    Error {
        /// The error message from the failed invocation.
        message: String,
    },
}
```

```rust
pub struct ToolInvocation {
    /// Name of the tool that was invoked.
    pub tool_name: String,
    /// Serialized arguments for identity comparison.
    pub arguments: String,
    /// Whether the invocation succeeded or failed.
    pub outcome: InvocationOutcome,
    /// When this invocation was recorded.
    pub recorded_at: jiff::Timestamp,
}
```

```rust
pub enum StuckPattern {
    /// The same error message appeared in the last N consecutive invocations.
    RepeatedError {
        /// The repeated error message.
        message: String,
        /// How many consecutive times the error appeared.
        count: u32,
    },
    /// The same tool was called with identical arguments N consecutive times.
    SameToolSameArgs {
        /// The tool that was called repeatedly.
        tool_name: String,
        /// How many consecutive times it was called with the same args.
        count: u32,
    },
    /// Two tools alternated back and forth, both failing.
    AlternatingFailure {
        /// First tool in the alternating pattern.
        tool_a: String,
        /// Second tool in the alternating pattern.
        tool_b: String,
        /// How many complete alternation cycles were detected.
        cycles: u32,
    },
    /// The same tool+args+error combination appeared scattered across the history window.
    EscalatingRetry {
        /// The tool being retried.
        tool_name: String,
        /// How many times the same failure was observed in the window.
        count: u32,
    },
}
```

```rust
pub struct StuckSignal {
    /// The pattern that triggered stuck detection.
    pub pattern: StuckPattern,
    /// A human-readable suggestion for how to intervene.
    pub suggestion: String,
}
```

```rust
pub struct StuckDetector {
    config: StuckConfig,
    history: VecDeque<ToolInvocation>,
}
```

## `src/verify.rs`

```rust
pub enum VerificationStatus {
    /// All criteria are met.
    Met,
    /// Some criteria are met, some are not.
    PartiallyMet,
    /// No criteria are met (or critical criteria failed).
    NotMet,
}
```

```rust
pub enum CriterionStatus {
    /// Criterion is satisfied.
    Met,
    /// Criterion is partially satisfied.
    PartiallyMet,
    /// Criterion is not satisfied.
    NotMet,
}
```

```rust
pub struct Evidence {
    /// Type of evidence (e.g., "file", "test-result", "output").
    pub kind: String,
    /// The evidence content or reference (file path, test output, etc.).
    pub content: String,
}
```

```rust
pub struct VerificationGap {
    /// The criterion text that was evaluated.
    pub criterion: String,
    /// Current status of this criterion.
    pub status: CriterionStatus,
    /// Detailed explanation of why the criterion is not fully met.
    pub detail: String,
    /// Concrete next step to close the gap.
    pub proposed_fix: String,
}
```

```rust
pub struct CriterionResult {
    /// The criterion text being evaluated.
    pub criterion: String,
    /// Whether it passed, partially passed, or failed.
    pub status: CriterionStatus,
    /// Evidence supporting this evaluation.
    pub evidence: Vec<Evidence>,
    /// Explanation of the evaluation outcome.
    pub detail: String,
}
```

```rust
pub struct VerificationResult {
    /// Overall phase verification status.
    pub status: VerificationStatus,
    /// Human-readable summary of the verification outcome.
    pub summary: String,
    /// Per-criterion results.
    pub criteria: Vec<CriterionResult>,
    /// Gaps that need to be addressed.
    pub gaps: Vec<VerificationGap>,
    /// When the verification was performed.
    pub verified_at: jiff::Timestamp,
    /// Whether this result was manually overridden.
    pub overridden: bool,
    /// Override justification, if any.
    pub override_note: Option<String>,
}
```

```rust
pub struct CriterionInput {
    /// The criterion text to evaluate.
    pub criterion: String,
    /// Whether the criterion is met.
    pub status: CriterionStatus,
    /// Evidence supporting the evaluation.
    pub evidence: Vec<Evidence>,
    /// Explanation of the evaluation.
    pub detail: String,
    /// If not met, a proposed fix.
    pub proposed_fix: Option<String>,
}
```

```rust
pub struct GoalTrace {
    /// The goal being traced.
    pub goal: String,
    /// Criteria that map to this goal, with their statuses.
    pub criteria: Vec<CriterionResult>,
    /// Whether this goal is fully satisfied.
    pub satisfied: bool,
}
```

```rust
pub fn verify_phase (phase: &Phase, inputs: &[CriterionInput]) -> VerificationResult
```

```rust
pub fn trace_goals (phase: &Phase, criteria: &[CriterionResult]) -> Vec<GoalTrace>
```

## `src/workspace.rs`

> Manages the on-disk workspace for a project.
```rust
pub struct ProjectWorkspace {
    root: PathBuf,
}
```

```rust
impl ProjectWorkspace {
    pub fn create (root: impl Into<PathBuf>) -> Result<Self>;
    pub fn open (root: impl Into<PathBuf>) -> Result<Self>;
    pub fn save_project (&self, project: &Project) -> Result<()>;
    pub fn load_project (&self) -> Result<Project>;
}
```
