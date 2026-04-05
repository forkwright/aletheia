//! Session management: spawn, monitor, resume, and budget-enforce dispatch sessions.
//!
//! The [`SessionManager`] is the per-prompt executor that drives a single
//! prompt through initial execution and graduated resume stages, producing a
//! [`SessionOutcome`](crate::types::SessionOutcome) with cost, turn, and
//! status data.
//!
//! # Module structure
//!
//! - [`manager`] — `SessionManager::execute()` and resume loop
//! - [`events`] — event stream processing, PR URL extraction, rate limit detection
//! - [`options`] — `EngineConfig` builder for session-level configuration

pub(crate) mod events;
pub mod manager;
pub mod options;

pub use manager::SessionManager;
pub use options::EngineConfig;
