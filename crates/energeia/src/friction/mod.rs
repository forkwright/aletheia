//! Friction capture protocol for PR-body observations.
//!
//! Worker agents record out-of-scope observations in a structured PR-body
//! section. The parser in [`capture`] extracts them so ephemeral session
//! knowledge can become institutional memory.

pub mod capture;

pub use capture::{Observation, TEMPLATE, parse_pr_body};
