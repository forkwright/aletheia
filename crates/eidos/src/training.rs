//! Training data capture types.
//!
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
    /// How the authorship decontamination gate treats captured turns.
    ///
    /// WHY this replaces the former `author_classifier_enabled` boolean: a
    /// boolean could express "gate on" but not what the gate owes the corpus
    /// when the classifier itself fails. Both capture paths answered that
    /// question by admitting the turn, so a classifier outage silently
    /// widened the corpus boundary. The policy makes the answer explicit and
    /// selectable. `TrainingConfig` denies unknown fields, so a config still
    /// carrying the removed boolean fails loudly at load rather than
    /// downgrading the gate to `Disabled` in silence.
    #[serde(default)]
    pub decontamination_policy: DecontaminationPolicy,
    /// Confidence threshold for the authorship gate.
    ///
    /// User messages where the top non-user class exceeds this threshold
    /// are filtered from training data. Range: [0.0, 1.0].
    /// Default: 0.85.
    #[serde(default = "default_author_classifier_threshold")]
    pub author_classifier_threshold: f32,
}

/// How the authorship decontamination gate disposes of a captured turn.
///
/// The gate screens the user message of every candidate training and DPO
/// record. This policy states what happens both when the classifier reports
/// non-user-authored text and when the classifier cannot produce a verdict
/// at all — the two cases a boolean toggle could not distinguish.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DecontaminationPolicy {
    /// Do not screen. Every turn passing the other quality gates is admitted.
    ///
    /// WHY this is the default: it preserves the behaviour of the former
    /// `author_classifier_enabled = false` default, so upgrading does not
    /// silently begin discarding turns an operator expected to keep.
    #[default]
    Disabled,
    /// Screen and record the verdict, but admit every turn.
    ///
    /// Use to measure how much of a corpus the gate would remove before
    /// enforcing it.
    Warn,
    /// Divert non-user-authored turns and classifier failures to a
    /// quarantine shard instead of the corpus.
    ///
    /// The turn stays on disk for inspection but is not part of the
    /// training corpus.
    Quarantine,
    /// Discard non-user-authored turns and classifier failures outright.
    FailClosed,
}

impl DecontaminationPolicy {
    /// Stable label for provenance records and metrics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Warn => "warn",
            Self::Quarantine => "quarantine",
            Self::FailClosed => "fail_closed",
        }
    }

    /// Whether this policy runs the classifier at all.
    #[must_use]
    pub fn screens(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// Returns the default value for [`TrainingConfig::max_shard_bytes`].
fn default_max_shard_bytes() -> u64 {
    DEFAULT_MAX_SHARD_BYTES
}

/// Default value for [`TrainingConfig::pii_filter_enabled`]: `true`.
fn default_pii_filter_enabled() -> bool {
    true
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
            decontamination_policy: DecontaminationPolicy::Disabled,
            author_classifier_threshold: 0.85,
        }
    }
}

/// Current schema version for [`TrainingRecord`].
///
/// Version 6 adds authorship-decontamination provenance while preserving
/// deserialization defaults for older JSONL rows.
pub const TRAINING_RECORD_SCHEMA_VERSION: u32 = 6;

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

/// Current version of `compute_quality_score`'s weighted-sum formula
/// (#4863: version every reward/quality formula). Bump when a weight, a
/// component's definition, or the saturation constant changes, so
/// downstream corpus consumers can tell which formula produced a given
/// row's `quality_score`.
pub const QUALITY_SCORE_FORMULA_VERSION: u32 = 1;

/// Component breakdown behind a `TrainingRecord`'s scalar `quality_score`
/// (#4863: preserve component scores/weights, not only the scalar sum).
///
/// Each `Some` rate is the raw signal `compute_quality_score` weighted and
/// summed; `None` means that signal did not fire for this turn (e.g. no
/// tool calls were made), matching `compute_quality_score`'s own
/// have-any-signal gating. The `weight_*` fields are recorded per-row
/// (rather than only in the formula-version doc comment) so a row remains
/// self-explaining even if the constants are re-tuned later.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QualityScoreComponents {
    /// Tool-call success rate (successes / total), when tool calls were made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_success_rate: Option<f32>,
    /// Recall-utilization rate (referenced / injected facts), when recall
    /// injected at least one fact with per-fact provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_utilization_rate: Option<f32>,
    /// Response-substance rate, saturating at the formula's char threshold.
    /// Always computed (a short response is still a valid signal input).
    pub response_substance_rate: f32,
    /// Whether the stop reason was a clean end (`EndTurn`/`StopSequence`).
    pub stop_reason_ok: bool,
    /// Correction-penalty factor applied (`0.0` if this turn was itself a
    /// correction, `1.0` otherwise), when correction status was known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_penalty_factor: Option<f32>,
    /// Weight applied to `tool_success_rate`.
    pub weight_tools: f32,
    /// Weight applied to `recall_utilization_rate`.
    pub weight_recall: f32,
    /// Weight applied to `response_substance_rate`.
    pub weight_substance: f32,
    /// Weight applied to the stop-reason term.
    pub weight_stop: f32,
    /// Weight applied to `correction_penalty_factor`.
    pub weight_correction: f32,
}

impl QualityScoreComponents {
    /// Recompute the scalar quality score from these components.
    ///
    /// A component whose rate is `None` contributes `0.0`, not
    /// `weight * default` -- matching `compute_quality_score`'s original
    /// have-any-signal semantics (a signal that never fired should not be
    /// scored as if it had fired at its worst value). Not clamped to
    /// `[0.0, 1.0]`: callers that need the persisted scalar's exact
    /// clamping should use `TrainingRecord::quality_score` directly.
    #[must_use]
    pub fn total_score(&self) -> f32 {
        self.tool_success_rate.map_or(0.0, |r| self.weight_tools * r)
            + self
                .recall_utilization_rate
                .map_or(0.0, |r| self.weight_recall * r)
            + self.weight_substance * self.response_substance_rate
            + self.weight_stop * f32::from(self.stop_reason_ok)
            + self
                .correction_penalty_factor
                .map_or(0.0, |f| self.weight_correction * f)
    }
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
    /// Provider that actually served the turn (e.g. `anthropic`, `kimi`).
    ///
    /// A separate dimension from `model` (#4798, #4863) — do not derive
    /// one from the other; `None` for rows captured before this field
    /// existed or where the observed provider was unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Total tokens consumed (input + output).
    pub tokens: u64,
    /// Sum of provider-reported cost in USD across the turn's completions.
    ///
    /// `None` when no completion in the turn reported a cost, distinct
    /// from `Some(0.0)` which would claim a verified free turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Sum of provider round-trip duration in milliseconds across the
    /// turn's completions.
    #[serde(default)]
    pub provider_duration_ms: u64,
    /// When the turn was captured.
    pub timestamp: Timestamp,

    // NOTE: Episteme labels (v2) group the four fields below.
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
    /// [`QUALITY_SCORE_FORMULA_VERSION`] at the time `quality_score` was
    /// computed (#4863). `None` for rows written before this field
    /// existed or when `quality_score` itself is `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_score_formula_version: Option<u32>,
    /// Component breakdown behind `quality_score` (#4863).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_score_components: Option<QualityScoreComponents>,

    // NOTE: Behavioural signals (v3) covers `tool_outcomes`.
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

    /// Opaque effective tool-surface hash refs observed during this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_surface_hashes: Vec<String>,

    /// Whether `user_message` or `assistant_response` changed during
    /// PII/secret redaction before persistence.
    ///
    /// This is mutation status only. Use `pii_filter_applied` to distinguish
    /// clean-but-screened rows from rows written without screening.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pii_redacted: bool,
    /// Whether the PII/secret filter evaluated this record before persistence.
    ///
    /// Downstream corpus readers can reject unscreened rows while accepting
    /// clean rows that were checked by the configured policy.
    #[serde(default)]
    pub pii_filter_applied: bool,
    /// Number of replacements made by the PII/secret filter.
    ///
    /// Zero means either the filter found no sensitive content or the row is
    /// unscreened; `pii_filter_applied` disambiguates those cases.
    #[serde(default)]
    pub pii_redaction_count: u32,
    /// Stable policy reference used for this screening pass.
    ///
    /// `None` for legacy rows and rows captured with the filter disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pii_policy_ref: Option<String>,

    // NOTE: Decontamination provenance (v6) groups the three fields below.
    /// Authorship-decontamination policy in force when this row was written.
    ///
    /// `None` for legacy rows written before the policy existed. A reader
    /// cannot infer the policy from the row's presence alone: under
    /// `Warn` an admitted row may still be non-user-authored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decontamination_policy: Option<DecontaminationPolicy>,
    /// Verdict the authorship gate reached for this row.
    ///
    /// Stable labels: `user`, `non_user`, `classifier_error`, `not_screened`.
    /// Downstream jobs can exclude `non_user` and `classifier_error` rows
    /// admitted under `Warn` without reclassifying the corpus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decontamination_verdict: Option<String>,
    /// Artifact version of the classifier that produced the verdict.
    ///
    /// `None` when the row was not screened. Recorded so a corpus can be
    /// re-screened selectively after a classifier upgrade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier_version: Option<String>,
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
        assert_eq!(
            config.decontamination_policy,
            DecontaminationPolicy::Disabled
        );
        assert!((config.author_classifier_threshold - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn decontamination_policy_parses_from_snake_case() {
        for (text, expected) in [
            ("disabled", DecontaminationPolicy::Disabled),
            ("warn", DecontaminationPolicy::Warn),
            ("quarantine", DecontaminationPolicy::Quarantine),
            ("fail_closed", DecontaminationPolicy::FailClosed),
        ] {
            let parsed: DecontaminationPolicy =
                serde_json::from_str(&format!("\"{text}\"")).expect("policy parses");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), text);
        }
    }

    #[test]
    fn removed_author_classifier_toggle_is_rejected_not_ignored() {
        // WHY: silently ignoring the removed key would downgrade an operator
        // who had enabled the gate back to `Disabled` without saying so.
        let err = serde_json::from_str::<TrainingConfig>(r#"{"author_classifier_enabled":true}"#)
            .expect_err("removed key must be rejected");
        assert!(
            err.to_string().contains("author_classifier_enabled"),
            "error should name the removed key, got: {err}"
        );
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
            provider: Some("anthropic".to_owned()),
            tokens: 200,
            cost_usd: Some(0.0123),
            provider_duration_ms: 850,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: Some("discussion".to_owned()),
            is_correction: Some(false),
            fact_types: Some(vec!["preference".to_owned()]),
            quality_score: Some(0.85),
            quality_score_formula_version: Some(QUALITY_SCORE_FORMULA_VERSION),
            quality_score_components: Some(QualityScoreComponents {
                tool_success_rate: Some(1.0),
                recall_utilization_rate: Some(0.5),
                response_substance_rate: 0.3,
                stop_reason_ok: true,
                correction_penalty_factor: Some(1.0),
                weight_tools: 0.40,
                weight_recall: 0.20,
                weight_substance: 0.20,
                weight_stop: 0.10,
                weight_correction: 0.10,
            }),
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
            tool_surface_hashes: vec!["ts1:test".to_owned()],
            pii_redacted: true,
            pii_filter_applied: true,
            pii_redaction_count: 1,
            pii_policy_ref: Some("nous-training-pii-v1".to_owned()),
            decontamination_policy: Some(DecontaminationPolicy::FailClosed),
            decontamination_verdict: Some("user".to_owned()),
            classifier_version: Some("0.1.0-heuristic".to_owned()),
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
        assert_eq!(back.tool_surface_hashes, vec!["ts1:test"]);
        assert!(back.pii_redacted);
        assert!(back.pii_filter_applied);
        assert_eq!(back.pii_redaction_count, 1);
        assert_eq!(back.pii_policy_ref.as_deref(), Some("nous-training-pii-v1"));
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
            provider: None,
            tokens: 100,
            cost_usd: None,
            provider_duration_ms: 0,
            timestamp: Timestamp::UNIX_EPOCH,
            turn_type: None,
            is_correction: None,
            fact_types: None,
            quality_score: None,
            quality_score_formula_version: None,
            quality_score_components: None,
            tool_outcomes: None,
            recall_signals: None,
            tool_surface_hashes: Vec::new(),
            pii_redacted: false,
            pii_filter_applied: false,
            pii_redaction_count: 0,
            pii_policy_ref: None,
            decontamination_policy: None,
            decontamination_verdict: None,
            classifier_version: None,
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
            !json.contains("tool_surface_hashes"),
            "empty hash refs should be skipped"
        );
        assert!(
            !json.contains("pii_redacted"),
            "false bool should be skipped"
        );
        assert!(
            json.contains("\"pii_filter_applied\":false"),
            "screening status should be explicit"
        );
        assert!(
            json.contains("\"pii_redaction_count\":0"),
            "redaction count should be explicit"
        );
        assert!(
            !json.contains("pii_policy_ref"),
            "disabled/unscreened policy ref should be skipped"
        );

        let back: TrainingRecord = serde_json::from_str(&json).expect("deserialize");
        assert!(back.turn_type.is_none());
        assert!(back.is_correction.is_none());
        assert!(back.tool_outcomes.is_none());
        assert!(back.recall_signals.is_none());
        assert!(!back.pii_redacted);
        assert!(!back.pii_filter_applied);
        assert_eq!(back.pii_redaction_count, 0);
        assert!(back.pii_policy_ref.is_none());
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
        assert!(!record.pii_filter_applied);
        assert_eq!(record.pii_redaction_count, 0);
        assert!(record.pii_policy_ref.is_none());
    }
}
