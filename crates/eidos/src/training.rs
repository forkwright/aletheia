//! Training data capture types.
//!
//! Configuration and record types for training data capture.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Default maximum shard size: 50 `MiB`.
const DEFAULT_MAX_SHARD_BYTES: u64 = 50 * 1024 * 1024;

/// Configuration for training data capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrainingConfig {
    /// Whether training data capture is enabled.
    pub enabled: bool,
    /// Directory path for training data output, relative to the instance root.
    ///
    /// The JSONL file `conversations.jsonl` is written inside this directory.
    pub path: String,
    /// Maximum size in bytes before rotating to a new shard file.
    ///
    /// When the current shard exceeds this limit, it is closed and a new
    /// shard is started. Default: 50 `MiB`.
    #[serde(default = "default_max_shard_bytes")]
    pub max_shard_bytes: u64,
    /// Whether to redact PII and secret patterns from `user_message` and
    /// `assistant_response` before writing a record to disk.
    ///
    /// WHY default = `true`: training corpora are persisted to the
    /// filesystem and may be shared with downstream training jobs.
    /// A conservative default prevents accidental leakage. Operators
    /// running a trusted local-only pipeline can disable explicitly.
    #[serde(default = "default_pii_filter_enabled")]
    pub pii_filter_enabled: bool,
    /// Whether to apply the author classifier gate to training capture.
    ///
    /// When `true`, user messages classified as non-user-authored with
    /// confidence >= `author_classifier_threshold` are rejected from the
    /// training corpus. Default: `false` (regression-safe).
    #[serde(default = "default_author_classifier_enabled")]
    pub author_classifier_enabled: bool,
    /// Confidence threshold for the authorship gate.
    ///
    /// User messages where the top non-user class exceeds this threshold
    /// are filtered from training data. Range: [0.0, 1.0].
    /// Default: 0.85.
    #[serde(default = "default_author_classifier_threshold")]
    pub author_classifier_threshold: f32,
}

/// Returns the default value for [`TrainingConfig::max_shard_bytes`].
fn default_max_shard_bytes() -> u64 {
    DEFAULT_MAX_SHARD_BYTES
}

/// Default value for [`TrainingConfig::pii_filter_enabled`]: `true`.
fn default_pii_filter_enabled() -> bool {
    true
}

/// Default value for [`TrainingConfig::author_classifier_enabled`]: `false`.
fn default_author_classifier_enabled() -> bool {
    false
}

/// Default value for [`TrainingConfig::author_classifier_threshold`]: 0.85.
fn default_author_classifier_threshold() -> f32 {
    0.85
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "data/training".to_owned(),
            max_shard_bytes: DEFAULT_MAX_SHARD_BYTES,
            pii_filter_enabled: true,
            author_classifier_enabled: false,
            author_classifier_threshold: 0.85,
        }
    }
}

/// Current schema version for [`TrainingRecord`].
///
/// Bump this constant whenever fields are added, removed, or change
/// semantics so that records from different epochs can be distinguished
/// at read time.
///
/// # History
///
/// - v0: initial, no `schema_version` field persisted
/// - v1: added `schema_version` field
/// - v2: added episteme labels (`turn_type`, `is_correction`, `fact_types`, `quality_score`)
/// - v3: added `tool_outcomes`, `recall_signals`, `pii_redacted`
pub const TRAINING_RECORD_SCHEMA_VERSION: u32 = 3;

/// Outcome of a single tool invocation during a turn.
///
/// WHY: training on tool-use traces needs to know whether calls
/// succeeded or failed. Success/failure is a reward signal for RL
/// fine-tuning (DPO/ORPO) — it distinguishes "tried and succeeded"
/// from "tried and errored" trajectories so the trainer can prefer
/// the former.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolOutcome {
    /// Name of the tool invoked (e.g. `"file_read"`, `"shell"`).
    pub name: String,
    /// Whether the tool call returned a successful result.
    pub success: bool,
    /// Wall-clock execution duration in milliseconds.
    pub duration_ms: u64,
    /// Coarse error classification when `success = false`. `None` on success.
    ///
    /// Callers should use short, stable labels (e.g. `"timeout"`,
    /// `"not_found"`, `"permission_denied"`) so downstream training
    /// jobs can bucket errors without parsing free-form text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
}

/// A single recalled fact captured for RL reward shaping.
///
/// Records both the raw recall score and whether the final assistant
/// output referenced the fact. The `was_referenced` field enables
/// future "did the model actually use what we gave it" reward signals
/// (Phase 06b RL training).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecalledFact {
    /// Stable identifier of the recalled source (fact / note / document id).
    pub source_id: String, // kanon:ignore RUST/primitive-for-domain-id — polymorphic source reference string, not a single domain ID type
    /// Source type label (e.g. `"fact"`, `"note"`, `"document"`).
    pub source_type: String,
    /// Final weighted recall score in `[0.0, 1.0]`.
    pub score: f64,
    /// Whether the assistant's response contained a reference to the
    /// recalled content (substring match on a content excerpt).
    pub was_referenced: bool,
}

/// Aggregate recall signals for a single turn.
///
/// WHY: Phase 06b RL training needs observability into the recall
/// stage — not just what was injected but how it was used. These
/// signals feed reward functions ("did recall help?", "did the model
/// cite what we retrieved?").
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RecallSignals {
    /// Total candidates returned by the recall engine before filtering.
    pub candidates_found: u32,
    /// Number of candidates that passed the recall threshold and were
    /// injected into the system prompt.
    pub results_injected: u32,
    /// Tokens spent on the injected recall section.
    pub tokens_consumed: u64,
    /// Per-fact recall records (source id, score, referenced flag).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<RecalledFact>,
}

/// A single training record representing one conversation turn.
///
/// Serialized as one JSON line in the output JSONL file. Fields match
/// the kanon training corpus schema for downstream compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRecord {
    /// Schema version that produced this record.
    ///
    /// Defaults to `0` when deserializing records written before the
    /// field existed, distinguishing them from version-1+ records.
    #[serde(default)]
    pub schema_version: u32,
    /// Session identifier (groups turns within a conversation).
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id — cross-crate session identifier, serialized as string here
    /// Nous agent identifier that handled the turn.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id — cross-crate nous identifier from koina, serialized as string here
    /// The user's input message.
    pub user_message: String,
    /// The assistant's response content.
    pub assistant_response: String,
    /// LLM model used for generation.
    pub model: String,
    /// Total tokens consumed (input + output).
    pub tokens: u64,
    /// When the turn was captured.
    pub timestamp: Timestamp,

    // ── Episteme labels (v2) ──────────────────────────────────────────
    /// Classification of the conversation turn (e.g. "discussion", "correction").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_type: Option<String>,
    /// Whether this turn corrects a previous response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_correction: Option<bool>,
    /// Types of facts extracted from this turn (e.g. "identity", "preference").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact_types: Option<Vec<String>>,
    /// Quality score for DPO/ORPO signal (0.0--1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<f32>,

    // ── Behavioural signals (v3) ──────────────────────────────────────
    /// Outcomes of tool calls made during the turn, in invocation order.
    ///
    /// `None` when the turn had no tool calls. An empty vec is reserved
    /// for turns that were configured to capture outcomes but produced
    /// none (should be unreachable in practice).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_outcomes: Option<Vec<ToolOutcome>>,

    /// Recall stage signals for this turn (facts recalled, whether they
    /// were referenced in the output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_signals: Option<RecallSignals>,

    /// Whether PII/secret redaction was applied to `user_message` and
    /// `assistant_response` before persistence.
    ///
    /// WHY persist as a field: downstream training jobs need to know
    /// whether a record has been scrubbed so they can refuse to
    /// re-process unredacted corpora if the redaction policy changes.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pii_redacted: bool,
}

/// Serde skip helper for boolean fields defaulting to `false`.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if signature"
)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn training_config_defaults() {
        let config = TrainingConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.path, "data/training");
        assert_eq!(config.max_shard_bytes, 50 * 1024 * 1024);
        assert!(config.pii_filter_enabled);
        assert!(!config.author_classifier_enabled);
        assert!((config.author_classifier_threshold - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn training_record_serde_roundtrip() {
        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
            session_id: "ses-1".to_owned(),
            nous_id: "syn".to_owned(),
            user_message: "test input".to_owned(),
            assistant_response: "test output".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            tokens: 200,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: Some("discussion".to_owned()),
            is_correction: Some(false),
            fact_types: Some(vec!["preference".to_owned()]),
            quality_score: Some(0.85),
            tool_outcomes: Some(vec![ToolOutcome {
                name: "file_read".to_owned(),
                success: true,
                duration_ms: 12,
                error_kind: None,
            }]),
            recall_signals: Some(RecallSignals {
                candidates_found: 5,
                results_injected: 2,
                tokens_consumed: 120,
                facts: vec![RecalledFact {
                    source_id: "fact-1".to_owned(),
                    source_type: "fact".to_owned(),
                    score: 0.73,
                    was_referenced: true,
                }],
            }),
            pii_redacted: true,
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let back: TrainingRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.schema_version, TRAINING_RECORD_SCHEMA_VERSION);
        assert_eq!(back.session_id, record.session_id);
        assert_eq!(back.tokens, record.tokens);
        assert_eq!(back.turn_type, Some("discussion".to_owned()));
        assert_eq!(back.is_correction, Some(false));
        assert_eq!(back.fact_types, Some(vec!["preference".to_owned()]));
        assert_eq!(back.quality_score, Some(0.85));
        assert_eq!(back.tool_outcomes.as_deref().map(<[_]>::len), Some(1));
        assert!(back.recall_signals.is_some());
        assert!(back.pii_redacted);
    }

    #[test]
    fn training_record_serde_roundtrip_no_labels() {
        // Records without labels should serialize without the optional fields.
        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
            session_id: "ses-1".to_owned(),
            nous_id: "syn".to_owned(),
            user_message: "test input".to_owned(),
            assistant_response: "test output".to_owned(),
            model: "test-model".to_owned(),
            tokens: 100,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: None,
            is_correction: None,
            fact_types: None,
            quality_score: None,
            tool_outcomes: None,
            recall_signals: None,
            pii_redacted: false,
        };

        let json = serde_json::to_string(&record).expect("serialize");
        assert!(!json.contains("turn_type"), "None fields should be skipped");
        assert!(
            !json.contains("is_correction"),
            "None fields should be skipped"
        );
        assert!(
            !json.contains("fact_types"),
            "None fields should be skipped"
        );
        assert!(
            !json.contains("quality_score"),
            "None fields should be skipped"
        );
        assert!(
            !json.contains("tool_outcomes"),
            "None fields should be skipped"
        );
        assert!(
            !json.contains("recall_signals"),
            "None fields should be skipped"
        );
        assert!(
            !json.contains("pii_redacted"),
            "false bool should be skipped"
        );

        let back: TrainingRecord = serde_json::from_str(&json).expect("deserialize");
        assert!(back.turn_type.is_none());
        assert!(back.is_correction.is_none());
        assert!(back.tool_outcomes.is_none());
        assert!(back.recall_signals.is_none());
        assert!(!back.pii_redacted);
    }

    #[test]
    fn training_record_deserialize_missing_schema_version() {
        // Records written before schema_version existed should deserialize
        // with schema_version defaulting to 0.
        let json = r#"{"session_id":"ses-old","nous_id":"syn","user_message":"hi","assistant_response":"hello","model":"test","tokens":10,"timestamp":"1970-01-01T00:00:00Z"}"#;
        let record: TrainingRecord = serde_json::from_str(json).expect("deserialize legacy");
        assert_eq!(record.schema_version, 0);
        assert_eq!(record.session_id, "ses-old");
        // Legacy records should have None for all label fields.
        assert!(record.turn_type.is_none());
        assert!(record.is_correction.is_none());
        assert!(record.fact_types.is_none());
        assert!(record.quality_score.is_none());
        assert!(record.tool_outcomes.is_none());
        assert!(record.recall_signals.is_none());
        assert!(!record.pii_redacted);
    }
}
