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
//! - [`qa::QaGate`] — quality assurance evaluation (mechanical + LLM)
//! - [`types`] — dispatch specs, outcomes, budgets, resume policies, QA results
//! - [`error`] — snafu error types with location tracking

/// Dispatch engine trait and session types.
pub mod engine;
/// Error types for energeia operations.
pub mod error;
/// Quality assurance gate trait.
pub mod qa;
/// Core dispatch types: specs, outcomes, budgets, resume policies, QA results.
pub mod types;
