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

use serde::Deserialize;

pub mod candidate;
pub mod extract;
pub mod heuristics;
pub mod signature;

pub use candidate::{CandidateTracker, SkillCandidate, SkillObservationEvidence, TrackResult};
pub use extract::{
    DedupInput, DedupOutcome, ExtractedSkill, PendingSkill, SkillExtractionAudit,
    SkillExtractionError, SkillExtractionProvider, SkillExtractionResult, SkillExtractor,
    SkillReviewAudit, SkillReviewDecision, SkillReviewInput, SkillSourceEvidence, check_dedup,
};
pub use heuristics::{HeuristicScore, PatternType, score_sequence};
pub use signature::{SequenceSignature, sequence_signature, signature_similarity};

/// Default maximum length retained for redacted tool input values.
pub const DEFAULT_TOOL_EVIDENCE_VALUE_LEN: usize = crate::instinct::DEFAULT_MAX_PARAM_VALUE_LEN;

/// A content-addressed reference to evidence that is not stored inline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContentEvidenceRef {
    /// Evidence kind, such as `"tool_result"` or `"extraction_prompt"`.
    pub kind: String,
    /// Digest algorithm used for [`Self::digest`].
    pub algorithm: String,
    /// Hex-encoded digest of the original content bytes.
    pub digest: String,
    /// Original byte length before hashing.
    pub bytes: usize,
}

impl ContentEvidenceRef {
    /// Create a SHA-256 evidence reference for content bytes.
    #[must_use]
    pub fn sha256(kind: impl Into<String>, content: impl AsRef<[u8]>) -> Self {
        let bytes = content.as_ref();
        Self {
            kind: kind.into(),
            algorithm: "sha256".to_owned(),
            digest: sha256_hex(bytes),
            bytes: bytes.len(),
        }
    }
}

/// A recorded tool call used as input for skill pattern analysis.
///
/// This is a lightweight record, a subset of a full tool execution record
/// carrying what the heuristic filter needs plus redacted review evidence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(from = "ToolCallRecordRaw")]
pub struct ToolCallRecord {
    /// Tool name (e.g. `"Read"`, `"Edit"`, `"Bash"`).
    pub tool_name: String,
    /// Whether the tool call resulted in an error.
    pub is_error: bool,
    /// How long the tool call took in milliseconds.
    pub duration_ms: u64,
    /// Optional tool-call identifier from the source turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    /// Sanitized input parameters with secret-like fields redacted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_input: Option<serde_json::Value>,
    /// Content-addressed reference to the result text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<ContentEvidenceRef>,
    /// Optional execution receipt already produced by the tool subsystem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
    /// SHA-256 hash over the durable, redacted tool-call evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_hash: Option<String>,
}

/// Raw deserialization type for [`ToolCallRecord`].
#[derive(Debug, Clone, Deserialize)]
struct ToolCallRecordRaw {
    tool_name: String,
    is_error: bool,
    duration_ms: u64,
    #[serde(default)]
    tool_id: Option<String>,
    #[serde(default)]
    redacted_input: Option<serde_json::Value>,
    #[serde(default)]
    result_ref: Option<ContentEvidenceRef>,
    #[serde(default)]
    receipt: Option<String>,
    #[serde(default)]
    call_hash: Option<String>,
}

impl From<ToolCallRecordRaw> for ToolCallRecord {
    fn from(raw: ToolCallRecordRaw) -> Self {
        Self {
            tool_name: raw.tool_name,
            is_error: raw.is_error,
            duration_ms: raw.duration_ms,
            tool_id: raw.tool_id,
            redacted_input: raw.redacted_input,
            result_ref: raw.result_ref,
            receipt: raw.receipt,
            call_hash: raw.call_hash,
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
            tool_id: None,
            redacted_input: None,
            result_ref: None,
            receipt: None,
            call_hash: None,
        }
    }

    /// Construct an errored tool call record.
    pub fn errored(tool_name: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            tool_name: tool_name.into(),
            is_error: true,
            duration_ms,
            tool_id: None,
            redacted_input: None,
            result_ref: None,
            receipt: None,
            call_hash: None,
        }
    }

    /// Attach redacted, content-addressed evidence to a tool call record.
    #[must_use]
    pub fn with_evidence(
        mut self,
        tool_id: impl Into<String>,
        input: &serde_json::Value,
        result: Option<&str>,
        receipt: Option<&str>,
    ) -> Self {
        self.tool_id = Some(tool_id.into());
        self.redacted_input = Some(crate::instinct::sanitize_parameters(
            input,
            DEFAULT_TOOL_EVIDENCE_VALUE_LEN,
        ));
        self.result_ref = result.map(|text| ContentEvidenceRef::sha256("tool_result", text));
        self.receipt = receipt.map(str::to_owned);
        self.call_hash = Some(self.compute_call_hash());
        self
    }

    /// Compute the durable hash for this redacted tool-call record.
    #[must_use]
    pub fn compute_call_hash(&self) -> String {
        let payload = serde_json::json!({
            "tool_name": self.tool_name,
            "is_error": self.is_error,
            "duration_ms": self.duration_ms,
            "tool_id": self.tool_id,
            "redacted_input": self.redacted_input,
            "result_ref": self.result_ref,
            "receipt": self.receipt,
        });
        sha256_hex(payload.to_string())
    }
}

/// Compute a stable SHA-256 hash for a sequence of redacted tool calls.
#[must_use]
pub fn tool_sequence_hash(tool_calls: &[ToolCallRecord]) -> String {
    let payload = serde_json::to_string(tool_calls).unwrap_or_else(|_| "[]".to_owned());
    sha256_hex(payload)
}

fn sha256_hex(content: impl AsRef<[u8]>) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(content.as_ref());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(hex_char(byte >> 4));
        hex.push(hex_char(byte & 0x0f));
    }
    hex
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '0',
    }
}
