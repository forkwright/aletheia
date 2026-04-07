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
//! - [`session`] — per-prompt session management: spawn, monitor, resume, budget enforce
//! - [`qa::QaGate`] — quality assurance evaluation (mechanical + LLM)
//! - [`budget`] — atomic cost/turn/duration tracking for concurrent sessions
//! - [`resume`] — multi-stage escalation policy for stuck sessions
//! - [`dag`] — prompt dependency graph with topological frontier computation
//! - [`prompt`] — YAML frontmatter loading and DAG construction from prompt files
//! - [`types`] — dispatch specs, outcomes, QA results
//! - [`error`] — snafu error types with location tracking

/// High-level dispatch backend trait for control plane integration.
pub mod backend;
/// Atomic budget tracking for dispatch runs.
pub mod budget;
/// Per-blast-radius cost attribution ledger.
pub mod cost_ledger;
/// Prompt dependency DAG and parallel frontier computation.
pub mod dag;
/// Dispatch engine trait and session types.
pub mod engine;
/// Error types for energeia operations.
pub mod error;
/// HTTP/SSE dispatch engine: subprocess-based `DispatchEngine` and mock.
pub mod http;
/// Metrics and reporting: health signals, cost reports, status dashboard, Prometheus.
pub mod metrics;
/// Top-level dispatch orchestrator: DAG execution with concurrency and QA.
pub mod orchestrator;
/// Prompt loading from YAML frontmatter files.
pub mod prompt;
/// Quality assurance gate trait.
pub mod qa;
/// Multi-stage resume escalation policy.
pub mod resume;
/// Per-prompt session management: spawn, monitor, resume, budget enforce.
pub mod session;
/// Steward CI management pipeline: classify, merge, fix, and manage pull requests.
pub mod steward;
/// State persistence layer (fjall key-value store).
pub mod store;
/// Core dispatch types: specs, outcomes, QA results.
pub mod types;
