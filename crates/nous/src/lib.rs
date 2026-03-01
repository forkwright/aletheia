//! aletheia-nous — agent session pipeline
//!
//! Nous (νοῦς) — "mind." The agent session manager, message pipeline,
//! and tool execution engine. Each nous instance runs a pipeline:
//! context → history → guard → resolve → execute → finalize.
//!
//! Depends on all foundation crates: koina, taxis, mneme, hermeneus.

pub mod config;
pub mod error;
pub mod pipeline;
pub mod session;
