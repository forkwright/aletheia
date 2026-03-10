//! Skill auto-capture — heuristic filter, signature hashing, and candidate tracking.
//!
//! ## Pipeline
//!
//! ```text
//! Session completes
//!   → tool call sequence extracted
//!   → heuristic filter scores it        (heuristics.rs)
//!   → if passed_gates → compute signature hash  (signature.rs)
//!   → check candidate tracker           (candidate.rs)
//!   → if recurrence_count >= 3 → Promoted
//!   → LLM extraction generates skill definition (separate)
//! ```

pub mod candidate;
pub mod extract;
pub mod heuristics;
pub mod signature;

pub use candidate::{CandidateTracker, SkillCandidate, TrackResult};
pub use extract::{
    ExtractedSkill, PendingSkill, SkillExtractionError, SkillExtractionProvider, SkillExtractor,
};
pub use heuristics::{HeuristicScore, PatternType, score_sequence};
pub use signature::{SequenceSignature, sequence_signature, signature_similarity};

/// A recorded tool call used as input for skill pattern analysis.
///
/// This is a lightweight record — a subset of a full tool execution record —
/// carrying only what the heuristic filter needs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallRecord {
    /// Tool name (e.g. `"Read"`, `"Edit"`, `"Bash"`).
    pub tool_name: String,
    /// Whether the tool call resulted in an error.
    pub is_error: bool,
    /// How long the tool call took in milliseconds.
    pub duration_ms: u64,
}

impl ToolCallRecord {
    /// Construct a successful tool call record.
    pub fn new(tool_name: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            tool_name: tool_name.into(),
            is_error: false,
            duration_ms,
        }
    }

    /// Construct an errored tool call record.
    pub fn errored(tool_name: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            tool_name: tool_name.into(),
            is_error: true,
            duration_ms,
        }
    }
}
