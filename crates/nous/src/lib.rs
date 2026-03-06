//! aletheia-nous — agent session pipeline
//!
//! Nous (νοῦς) — "mind." The agent session manager, message pipeline,
//! and tool execution engine. Each nous instance runs a pipeline:
//! context → history → guard → resolve → execute → finalize.
//!
//! Depends on all foundation crates: koina, taxis, mneme, hermeneus.

/// Trait adapters bridging organon tool traits to mneme SessionStore.
pub mod adapters;
/// Tokio actor driving a single nous instance's message loop.
pub mod actor;
/// System prompt assembly from workspace files and domain packs.
pub mod bootstrap;
/// Token and wall-clock time budget tracking for pipeline stages.
pub mod budget;
/// Per-agent and per-pipeline configuration types.
pub mod config;
/// Inter-agent messaging — fire-and-forget, request-response, and delivery audit.
pub mod cross;
/// Distillation trigger logic and orchestration.
pub mod distillation;
/// Nous-specific error types.
pub mod error;
/// LLM execution stage — sends the assembled prompt to the provider.
pub mod execute;
pub(crate) mod extraction;
/// Turn finalization — persists messages and emits post-turn events.
pub mod finalize;
/// Cloneable handle for sending commands to a [`actor::NousActor`].
pub mod handle;
/// Conversation history retrieval and token-budgeted formatting.
pub mod history;
/// Lifecycle manager for spawning and addressing nous actors.
pub mod manager;
/// Actor inbox message types.
pub mod message;
pub mod metrics;
/// Turn pipeline orchestration — context through finalize.
pub mod pipeline;
/// Semantic recall stage — vector search over knowledge memories.
pub mod recall;
/// Session state tracking within a nous actor.
pub mod session;
/// Ephemeral sub-agent spawning service.
pub mod spawn_svc;
/// Real-time streaming events for the turn pipeline.
pub mod stream;
/// User-facing error formatting for display in chat responses.
pub mod user_error;
