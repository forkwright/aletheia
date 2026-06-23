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
pub mod competence;
/// Per-agent and per-pipeline configuration types.
pub mod config;
/// Inter-agent messaging: fire-and-forget, request-response, and delivery audit.
pub mod cross;
/// Graceful degradation contracts when the LLM provider is unavailable.
pub mod degraded_mode;
/// Distillation trigger logic and orchestration.
pub mod distillation;
/// Quality drift detection: rolling-window metrics with z-score deviation alerts.
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
pub mod memory;
/// Actor inbox message types.
pub(crate) mod message;
/// Prometheus metrics for nous pipeline: turn counts, latency, and token usage.
pub mod metrics;
/// Turn pipeline orchestration: context through finalize.
pub mod pipeline;
/// Semantic recall stage: vector search over knowledge memories.
pub mod recall;
/// Durable turn-attempt lifecycle records and finalize idempotency.
pub(crate) mod turn_record;
/// Task-specific _llm/ loading recipes for multi-resolution context.
pub mod recipes;
/// Parallel research orchestrator: spawns domain researchers via the sub-agent system.
pub mod research;
/// Specialized role templates for ephemeral sub-agents.
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
pub mod tasks;
/// Training data capture: append-only JSONL writer for conversation turns.
///
/// Pipeline tap that observes the turn loop and writes qualifying turns
/// as JSON Lines for downstream fine-tuning. Types (`TrainingConfig`,
/// `TrainingRecord`) live in eidos; capture logic lives here because it
/// is a pipeline concern, not a memory operation.
pub mod training;
/// Self-tuning feedback loop: evidence-based parameter change proposals.
pub mod tuning;
/// Uncertainty quantification: calibration tracking for agent confidence predictions.
pub mod uncertainty;
/// User-facing error formatting for display in chat responses.
pub mod user_error;
/// Working-memory checkpoint persistence.
pub mod working_memory;
/// Working state management: task stack, focus context, wait state.
pub mod working_state;
