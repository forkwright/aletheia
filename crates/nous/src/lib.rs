#![deny(missing_docs)]
//! aletheia-nous: agent session pipeline

/// Tokio actor driving a single nous instance's message loop.
pub(crate) mod actor;
/// Trait adapters bridging organon tool traits to mneme SessionStore.
pub mod adapters;
/// User approval gate for reversibility-class tool calls (#3958).
pub mod approval;
/// Prompt audit log: operator-visible record of every outbound LLM request (#3411).
pub mod audit;
/// System prompt assembly from workspace files and domain packs.
pub mod bootstrap;
/// Token and wall-clock time budget tracking for pipeline stages.
pub mod budget;
/// Context compaction: microcompaction (per-turn clearing) and full compaction (summarization).
pub(crate) mod compact;
/// Per-agent per-domain competence tracking with rolling statistics and model escalation.
// WHY(#6750): zero cross-crate consumers, but `CompetenceTracker`'s only
// in-crate exerciser is its own `#[cfg(test)] mod tests` — invisible to the
// plain `cargo check` (lib) pass `-D warnings` runs under. `pub(crate)`
// makes the whole module dead code in that pass. Stays `pub` until wired to
// a real caller or removed.
pub mod competence;
/// Per-agent and per-pipeline configuration types.
pub mod config;
/// Inter-agent messaging: fire-and-forget, request-response, and delivery audit.
pub mod cross;
/// Graceful degradation contracts when the LLM provider is unavailable.
// WHY(#6750): `DegradedMode` is re-exported at the crate's public `pipeline`
// contract (`pipeline::DegradedMode`), but `DegradedAttemptContext::unknown`,
// `is_storage_failure`, and `build_degraded_response` are not re-exported and
// have no in-crate callers outside their own `#[cfg(test)]` blocks — dead
// code under the plain `cargo check` (lib) pass when the module itself is
// `pub(crate)`. Stays `pub` until wired to a real caller or removed.
pub mod degraded_mode;
/// Distillation trigger logic and orchestration.
// WHY(#6750): same dead-code trap as `competence` above —
// `DistillTriggerConfig::from_behavior` and `maybe_distill` have zero real
// callers anywhere in the workspace. Stays `pub`.
pub mod distillation;
/// Quality drift detection: rolling-window metrics with z-score deviation alerts.
// WHY(#6750): same dead-code trap as `competence` above —
// `DriftConfig::from_behavior` has zero real callers, and
// `DriftDetector::turn_count`/`reset` are exercised only by the module's own
// `#[cfg(test)]` block. Stays `pub`.
pub mod drift;
/// Nous-specific error types.
pub mod error;
/// LLM execution stage: sends the assembled prompt to the provider.
pub(crate) mod execute;
pub(crate) mod extraction;
/// Turn finalization: persists messages and emits post-turn events.
pub(crate) mod finalize;
/// Cloneable handle for sending commands to a `NousActor`.
pub mod handle;
/// Conversation history retrieval and token-budgeted formatting.
pub(crate) mod history;
/// Turn-level hook system for behavior correction at query, tool, and turn boundaries.
pub(crate) mod hooks;
/// Instinct observation bridge: records tool usage for behavioral pattern learning.
pub(crate) mod instinct;
/// Lifecycle manager for spawning and addressing nous actors.
pub mod manager;
/// Memory types for structured conversation representation.
pub(crate) mod memory;
/// Actor inbox message types.
pub(crate) mod message;
/// Prometheus metrics for nous pipeline: turn counts, latency, and token usage.
pub mod metrics;
/// Turn pipeline orchestration: context through finalize.
pub mod pipeline;
/// Semantic recall stage: vector search over knowledge memories.
pub mod recall;
/// Task-specific _llm/ loading recipes for multi-resolution context.
// WHY(#6750): same dead-code trap as `competence` above — `Recipe`'s
// `avg_reduction_pct`/`success_rate` and most of `RecipeRegistry`'s API
// (`all`, `len`, `is_empty`, `select_for_task`, `select`, `ordered_recipes`,
// the `recipe_order` field) are exercised only by the module's own
// `#[cfg(test)]` block. Stays `pub`.
pub mod recipes;
/// Parallel research orchestrator: spawns domain researchers via the sub-agent system.
// WHY(#6750): same dead-code trap as `competence` above — zero real callers,
// only exercised by its own `#[cfg(test)]` block. Stays `pub`.
pub mod research;
/// Specialized role templates for ephemeral sub-agents.
// WHY(#6750): same dead-code trap as `competence` above —
// `ToolPolicy::Unrestricted`, `RoleTemplate::role`, and most of
// `ContractRegistry`'s API (`from_toml`, `all`, `len`, `is_empty`) are
// exercised only by their own `#[cfg(test)]` blocks. Stays `pub`.
pub mod roles;
/// Self-auditing loop: prosoche checks, audit triggers, and knowledge graph storage.
pub mod self_audit;
/// Session state tracking within a nous actor.
pub mod session;
/// Skill loading: queries mneme for task-relevant skills and injects them as bootstrap sections.
pub(crate) mod skills;
/// Ephemeral sub-agent spawning service.
pub mod spawn_svc;
/// Real-time streaming events for the turn pipeline.
pub mod stream;
/// Task registry with progress streaming, cooperative cancellation, and GC.
// WHY(#6750): same dead-code trap as `competence` above — `TaskRegistry` has
// zero real callers anywhere in the workspace. Stays `pub`.
pub mod tasks;
/// Training data capture: append-only JSONL writer for conversation turns.
///
/// Pipeline tap that observes the turn loop and writes qualifying turns
/// as JSON Lines for downstream fine-tuning. Types (`TrainingConfig`,
/// `TrainingRecord`) live in eidos; capture logic lives here because it
/// is a pipeline concern, not a memory operation.
pub mod training;
/// Self-tuning feedback loop: evidence-based parameter change proposals.
// WHY(#6750): same dead-code trap as `competence` above — most of this
// module's diagnostic fields (`MetricSample::timestamp`,
// `ProposalEvidence::metric_before`/`metric_after`, the `ProposalOutcome`
// variant fields) and its `signals` submodule (`OutcomeSignal` and its
// scoring functions) are set or defined but never read outside the module's
// own `#[cfg(test)]` blocks. Stays `pub`.
pub mod tuning;
/// Durable turn-attempt lifecycle records, finalize idempotency, and
/// run-context provenance records (#4542). `pub` so external inspection
/// surfaces (e.g. `aletheia memory inspect-context`) can read
/// `RunContextRecord`s without duplicating the note-category/store
/// mechanics.
pub mod turn_record;
/// Uncertainty quantification: calibration tracking for agent confidence predictions.
// WHY(#6750): `CalibrationBin`/`OverconfidencePattern`/`CalibrationSummary`
// are `pub` with zero real callers, only exercised by the module's own
// `#[cfg(test)]` block — dead code under the plain `cargo check` (lib) pass
// when the module itself is `pub(crate)`. Stays `pub`. `UncertaintyTracker`
// is separately `pub(crate)` at the item level and unaffected by this line.
pub mod uncertainty;
/// User-facing error formatting for display in chat responses.
pub mod user_error;
/// Working-memory checkpoint persistence.
// WHY(#4588): `pub` because `FjallWorkingCheckpointStore` is opened across
// the crate boundary by `crates/aletheia/src/runtime/mod.rs` and
// `crates/aletheia/src/commands/agent_io.rs`.
pub mod working_memory;
/// Working state management: task stack, focus context, wait state.
pub(crate) mod working_state;
