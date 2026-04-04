//! aletheia-energeia: dispatch orchestration for the Aletheia agent runtime.
//!
//! Energeia (ἐνέργεια): "actualization" — the process of bringing potential
//! into reality. This crate orchestrates the dispatch of coding tasks to agent
//! sessions, tracks budgets and health, evaluates quality, and manages the
//! lifecycle from prompt to merged PR.
//!
//! # Architecture
//!
//! - [`engine::DispatchEngine`] — session execution backend (Agent SDK HTTP/SSE)
//! - [`http`] — subprocess-based `DispatchEngine` implementation and mock engine
//! - [`qa::QaGate`] — quality assurance evaluation (mechanical + LLM)
//! - [`budget`] — atomic cost/turn/duration tracking for concurrent sessions
//! - [`resume`] — multi-stage escalation policy for stuck sessions
//! - [`dag`] — prompt dependency graph with topological frontier computation
//! - [`prompt`] — YAML frontmatter loading and DAG construction from prompt files
//! - [`types`] — dispatch specs, outcomes, QA results
//! - [`error`] — snafu error types with location tracking

/// Atomic budget tracking for dispatch runs.
pub mod budget;
/// Prompt dependency DAG and parallel frontier computation.
pub mod dag;
/// Dispatch engine trait and session types.
pub mod engine;
/// Error types for energeia operations.
pub mod error;
/// HTTP/SSE dispatch engine: subprocess-based `DispatchEngine` and mock.
pub mod http;
/// Prompt loading from YAML frontmatter files.
pub mod prompt;
/// Quality assurance gate trait.
pub mod qa;
/// Multi-stage resume escalation policy.
pub mod resume;
/// State persistence layer (fjall key-value store).
pub mod store;
/// Core dispatch types: specs, outcomes, QA results.
pub mod types;
