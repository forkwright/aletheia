//! Skill auto-capture: heuristic filter, signature hashing, and candidate tracking.
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

use serde::{Deserialize, Serialize};

pub mod candidate;
pub mod extract;
pub mod heuristics;
pub mod signature;

pub use candidate::{CandidateTracker, SkillCandidate, TrackResult};
pub use extract::{
    DedupInput, DedupOutcome, ExtractedSkill, PendingSkill, SkillExtractionError,
    SkillExtractionProvider, SkillExtractor, check_dedup,
};
pub use heuristics::{HeuristicScore, PatternType, score_sequence};
pub use signature::{SequenceSignature, sequence_signature, signature_similarity};

/// A recorded tool call used as input for skill pattern analysis.
///
/// This is a lightweight record, a subset of a full tool execution record
/// carrying only what the heuristic filter needs. When provenance is enabled,
/// input and result payloads are replaced by content-addressed SHA-256 hashes
/// so secrets are not retained in the skill store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ToolCallRecordRaw")]
pub struct ToolCallRecord {
    /// Tool name (e.g. `"Read"`, `"Edit"`, `"Bash"`).
    pub tool_name: String,
    /// Whether the tool call resulted in an error.
    pub is_error: bool,
    /// How long the tool call took in milliseconds.
    pub duration_ms: u64,
    /// Content-addressed SHA-256 hash of the raw tool input payload, if captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input_hash: Option<String>,
    /// Content-addressed SHA-256 hash of the raw tool result payload, if captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result_hash: Option<String>,
}

/// Raw deserialization type for [`ToolCallRecord`].
#[derive(Debug, Clone, Deserialize)]
struct ToolCallRecordRaw {
    tool_name: String,
    is_error: bool,
    duration_ms: u64,
    #[serde(default)]
    tool_input_hash: Option<String>,
    #[serde(default)]
    tool_result_hash: Option<String>,
}

impl From<ToolCallRecordRaw> for ToolCallRecord {
    fn from(raw: ToolCallRecordRaw) -> Self {
        Self {
            tool_name: raw.tool_name,
            is_error: raw.is_error,
            duration_ms: raw.duration_ms,
            tool_input_hash: raw.tool_input_hash,
            tool_result_hash: raw.tool_result_hash,
        }
    }
}

impl ToolCallRecord {
    /// Construct a successful tool call record.
    pub fn new(tool_name: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            tool_name: tool_name.into(),
            is_error: false,
            duration_ms,
            tool_input_hash: None,
            tool_result_hash: None,
        }
    }

    /// Construct an errored tool call record.
    pub fn errored(tool_name: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            tool_name: tool_name.into(),
            is_error: true,
            duration_ms,
            tool_input_hash: None,
            tool_result_hash: None,
        }
    }

    /// Build a record with content-addressed input/result hashes.
    #[must_use]
    pub fn with_hashes(
        tool_name: impl Into<String>,
        duration_ms: u64,
        is_error: bool,
        tool_input_hash: Option<String>,
        tool_result_hash: Option<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            is_error,
            duration_ms,
            tool_input_hash,
            tool_result_hash,
        }
    }
}

/// Evidence captured for a single session that contributed to a learned skill.
///
/// Keeps the redacted tool sequence observed in that session plus a hash that
/// identifies the turn/sequence without storing user text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillEvidence {
    /// Session in which the pattern was observed.
    // kanon:ignore RUST/primitive-for-domain-id — JSON serialization type for knowledge-store fact content fields
    pub session_id: String,
    /// Deterministic hash of the full tool-call sequence in this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_sequence_hash: Option<String>,
    /// Redacted tool-call records from the session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,
}

/// Audit record for an LLM skill-extraction pass.
///
/// When the provider does not expose real audit refs, we fall back to
/// deterministic SHA-256 hashes of the prompt and response material so the
/// extraction remains reproducible and referencable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionAudit {
    /// SHA-256 hash of the extraction prompt (system + user messages).
    pub prompt_hash: String,
    /// SHA-256 hash of the raw LLM response, if one was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_hash: Option<String>,
    /// Model identifier used for extraction, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// When the extraction occurred.
    pub extracted_at: jiff::Timestamp,
}

/// Review decision attached to a pending or approved learned skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDecision {
    /// Actor that performed the review (e.g. user ID or "operator").
    pub actor: String,
    /// Action taken: `"approved"` or `"rejected"`.
    pub action: String,
    /// Optional free-form reason for the decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When the decision was recorded.
    pub decided_at: jiff::Timestamp,
}

/// Compute a lowercase hex SHA-256 digest of `data`.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}
