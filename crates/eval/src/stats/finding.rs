//! Back-compat re-export of `mneme::finding`.
//!
//! The canonical types now live in `mneme::finding` so that multiple crates
//! (`eval`, `nous::self_audit`, `daemon::prosoche`) can share a single finding
//! shape without depending on `eidos` directly. This module preserves the old
//! `eval::stats::finding` path until callers migrate.

pub use mneme::finding::{ConfidenceSummary, EvidenceLevel, Finding, FindingStats};

/// Historical alias for [`Finding`].
pub use mneme::finding::Finding as EvalFinding;
