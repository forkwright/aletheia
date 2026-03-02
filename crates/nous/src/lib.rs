//! aletheia-nous — agent session pipeline
//!
//! Nous (νοῦς) — "mind." The agent session manager, message pipeline,
//! and tool execution engine. Each nous instance runs a pipeline:
//! context → history → guard → resolve → execute → finalize.
//!
//! Depends on all foundation crates: koina, taxis, mneme, hermeneus.

pub mod actor;
pub mod bootstrap;
pub mod budget;
pub mod config;
pub mod error;
pub mod execute;
pub mod handle;
pub mod history;
pub mod manager;
pub mod message;
pub mod pipeline;
pub mod recall;
pub mod session;
