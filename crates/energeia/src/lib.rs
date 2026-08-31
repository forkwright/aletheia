//! aletheia-energeia: dispatch orchestration for the Aletheia agent runtime.
#![deny(missing_docs)]
//!
//! Energeia (ἐνέργεια): "actualization" — the process of bringing potential
//! into reality. This crate orchestrates the dispatch of coding tasks to agent
//! sessions, tracks budgets and health, evaluates quality, and manages the
//! lifecycle from prompt to merged PR.
//!
//! # Architecture
//!
//! - [`engine::DispatchEngine`] — session execution backend (a Claude CLI
//!   subprocess wrapper; no Anthropic-hosted "Agent SDK" HTTP/SSE endpoint
//!   exists to migrate to — see the [`http`] module docs for why)
//! - [`http`] — subprocess-based `DispatchEngine` implementation and mock engine
//! - [`session`] — per-prompt session management: spawn, monitor, resume, budget enforce
//! - [`qa::QaGate`] — quality assurance evaluation (mechanical + LLM)
//! - [`budget`] — atomic cost/turn/duration tracking for concurrent sessions
//! - [`resume`] — multi-stage escalation policy for stuck sessions
//! - [`dag`] — prompt dependency graph with topological frontier computation
//! - [`prompt`] — YAML frontmatter loading and DAG construction from prompt files
//! - [`routing`] — static and empirical provider selection (success-rate based)
//! - [`types`] — dispatch specs, outcomes, QA results
//! - [`error`] — snafu error types with location tracking

pub(crate) const CLI_BINARY: &str = "claude";

/// Atomic budget tracking for dispatch runs.
// WHY(#5576): `Budget`/`BudgetStatus` are re-exported at `types` (the
// consumed cross-crate surface); the module path itself has zero external
// consumers.
pub(crate) mod budget;
/// Per-blast-radius cost attribution ledger.
pub mod cost_ledger;
/// Cron scheduler for recurring dispatch tasks with fjall-backed locking.
#[cfg(feature = "storage-fjall")]
pub mod cron;
/// Prompt dependency DAG and parallel frontier computation.
pub mod dag;
/// Shared unified-diff parsing helpers used by QA and steward.
pub(crate) mod diff;
/// Dispatch engine trait and session types.
pub mod engine;
/// Error types for energeia operations.
pub mod error;
/// Parallel-execution frontier derivation from a [`dag::PromptDag`].
// WHY(#5576): `compute_frontier` is re-exported at `dag` (the consumed
// cross-crate surface); the module path itself has zero external consumers.
pub(crate) mod frontier;
/// HTTP/SSE dispatch engine: subprocess-based `DispatchEngine` and mock.
pub mod http;
/// Metrics and reporting: health signals, cost reports, status dashboard, Prometheus.
pub mod metrics;
/// Top-level dispatch orchestrator: DAG execution with concurrency and QA.
pub mod orchestrator;
/// 4-stage dispatch pipeline: preparation → execution → post-processing.
pub(crate) mod pipeline;
/// Predictive budget allocation from prompt characteristics.
// WHY(#6750): `classify_with_detail`/`predict_budget` have zero real
// callers — dead code under the plain `cargo check` (lib) pass `-D warnings`
// runs under if demoted to `pub(crate)`. Stays `pub`.
pub mod predictive_budget;
/// Prompt loading from YAML frontmatter files.
pub mod prompt;
/// Prompt cache optimization: static prefix / dynamic suffix split.
pub(crate) mod prompt_cache;
/// Quality assurance gate trait.
pub mod qa;
/// Multi-stage resume escalation policy.
// WHY(#5576): `ResumePolicy`/`ResumeStage` are re-exported at `types` (the
// consumed cross-crate surface); the module path itself has zero external
// consumers.
pub(crate) mod resume;
/// Provider routing: static config-driven and empirical success-rate-based selection.
pub(crate) mod routing;
/// Per-prompt session management: spawn, monitor, resume, budget enforce.
// WHY(#6750): `session::isolation` (worktree resolution) and
// `EngineConfig`'s builder methods are exercised only by their own unit
// tests — dead code under the plain `cargo check` (lib) pass `-D warnings`
// runs under if demoted to `pub(crate)`; nothing else in-crate calls them. Stays
// `pub`.
pub mod session;
/// Steward CI management pipeline: classify, merge, fix, and manage pull requests.
pub mod steward;
/// State persistence layer (fjall key-value store).
pub mod store;
/// Core dispatch types: specs, outcomes, QA results.
pub mod types;
