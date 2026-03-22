#![deny(missing_docs)]
//! aletheia-nous: agent session pipeline

/// Tokio actor driving a single nous instance's message loop.
pub(crate) mod actor;
/// Trait adapters bridging organon tool traits to mneme SessionStore.
pub mod adapters;
/// System prompt assembly from workspace files and domain packs.
pub mod bootstrap;
/// Token and wall-clock time budget tracking for pipeline stages.
pub mod budget;
/// Chiron self-auditing loop: prosoche checks, audit triggers, and knowledge graph storage.
pub mod chiron;
/// Per-agent per-domain competence tracking with rolling statistics and model escalation.
pub mod competence;
/// Per-agent and per-pipeline configuration types.
pub mod config;
/// Inter-agent messaging: fire-and-forget, request-response, and delivery audit.
pub mod cross;
/// Distillation trigger logic and orchestration.
pub mod distillation;
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
/// Instinct observation bridge: records tool usage for behavioral pattern learning.
pub(crate) mod instinct;
/// Lifecycle manager for spawning and addressing nous actors.
pub mod manager;
/// Actor inbox message types.
pub(crate) mod message;
/// Prometheus metrics for nous pipeline: turn counts, latency, and token usage.
pub mod metrics;
/// Turn pipeline orchestration: context through finalize.
pub mod pipeline;
/// Semantic recall stage: vector search over knowledge memories.
pub mod recall;
/// Parallel research orchestrator: spawns domain researchers via the sub-agent system.
pub mod research;
/// Specialized role templates for ephemeral sub-agents.
pub mod roles;
/// Session state tracking within a nous actor.
pub mod session;
/// Skill loading: queries mneme for task-relevant skills and injects them as bootstrap sections.
pub(crate) mod skills;
/// Ephemeral sub-agent spawning service.
pub mod spawn_svc;
/// Real-time streaming events for the turn pipeline.
pub mod stream;
/// Uncertainty quantification: calibration tracking for agent confidence predictions.
pub mod uncertainty;
/// User-facing error formatting for display in chat responses.
pub mod user_error;
/// Working state management: task stack, focus context, wait state.
pub mod working_state;
