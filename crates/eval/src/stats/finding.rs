//! Back-compat re-export of `eidos::knowledge::finding`.
//!
//! The canonical types live in `eidos` so that multiple crates (`eval`,
//! `nous::self_audit`, `daemon::prosoche`) can share a single finding shape.
//! This module preserves the old `eval::stats::finding` path until callers
//! migrate.

pub use eidos::knowledge::finding::{ConfidenceSummary, EvidenceLevel, Finding, FindingStats};

/// Historical alias for [`Finding`].
pub use eidos::knowledge::finding::Finding as EvalFinding;
