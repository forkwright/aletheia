//! Fact domain types: extraction, classification, decay, timestamps.

use serde::{Deserialize, Serialize};

use crate::id::FactId;

use super::MemoryScope;

/// Maximum byte length for fact content strings.
pub const MAX_CONTENT_LENGTH: usize = 102_400;

/// Bi-temporal validity and recording timestamps for a fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(missing_docs, reason = "temporal fields are self-documenting by name")]
pub struct FactTemporal {
    pub valid_from: jiff::Timestamp,
    pub valid_to: jiff::Timestamp,
    pub recorded_at: jiff::Timestamp,
}

/// Provenance: where a fact came from and how trustworthy it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "provenance fields are self-documenting by name"
)]
pub struct FactProvenance {
    pub confidence: f64,
    pub tier: EpistemicTier,
    pub source_session_id: Option<String>,
    pub stability_hours: f64,
}

/// Lifecycle state for supersession and intentional forgetting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(missing_docs, reason = "lifecycle fields are self-documenting by name")]
pub struct FactLifecycle {
    pub superseded_by: Option<FactId>,
    pub is_forgotten: bool,
    pub forgotten_at: Option<jiff::Timestamp>,
    pub forget_reason: Option<ForgetReason>,
}

/// Access-tracking counters for FSRS decay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(missing_docs, reason = "access fields are self-documenting by name")]
pub struct FactAccess {
    pub access_count: u32,
    pub last_accessed_at: Option<jiff::Timestamp>,
}

/// A memory fact extracted from conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(missing_docs, reason = "fact fields are self-documenting by name")]
pub struct Fact {
    pub id: FactId,
    pub nous_id: String,
    pub fact_type: String,
    pub content: String,

    /// Memory sharing scope for team memory.
    ///
    /// `None` for facts created before the team memory model was introduced.
    /// New facts should always populate this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MemoryScope>,

    /// Bi-temporal validity and recording timestamps.
    #[serde(flatten)]
    pub temporal: FactTemporal,
    /// Provenance and confidence metadata.
    #[serde(flatten)]
    pub provenance: FactProvenance,
    /// Supersession and forgetting lifecycle.
    #[serde(flatten)]
    pub lifecycle: FactLifecycle,
    /// Access-tracking counters.
    #[serde(flatten)]
    pub access: FactAccess,
}

/// Epistemic confidence tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EpistemicTier {
    /// Checked against ground truth.
    Verified,
    /// Reasoned from context.
    Inferred,
    /// Unchecked assumption.
    Assumed,
    /// Derived from agent session outcomes for training signal.
    Training,
}

impl EpistemicTier {
    /// Return the lowercase string representation of this tier.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Inferred => "inferred",
            Self::Assumed => "assumed",
            Self::Training => "training",
        }
    }

    /// FSRS stability multiplier: verified facts decay 2× slower than inferred.
    #[must_use]
    pub fn stability_multiplier(self) -> f64 {
        match self {
            Self::Verified => 2.0,
            Self::Inferred => 1.0,
            Self::Assumed => 0.5,
            // WHY: training data is a permanent record of session outcomes,
            // not subject to memory decay — it persists indefinitely.
            Self::Training => 4.0,
        }
    }
}

/// Knowledge lifecycle stage for graduated pruning.
///
/// Facts progress through stages as decay increases, rather than being
/// deleted immediately. Each stage represents a different level of
/// recall priority and pruning eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum KnowledgeStage {
    /// Fully active, included in standard recall. Decay score >= 0.7.
    Active,
    /// Recall score declining. Still retrievable but deprioritized. Decay in [0.3, 0.7).
    Fading,
    /// Low recall probability. Excluded from default recall, available on explicit query. Decay in [0.1, 0.3).
    Dormant,
    /// Below retention threshold. Candidate for permanent removal. Decay < 0.1.
    Archived,
}

/// Default decay score threshold for transitioning from Active to Fading.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::fact_active_threshold`.
pub const DEFAULT_STAGE_ACTIVE_THRESHOLD: f64 = 0.7;
/// Default decay score threshold for transitioning from Fading to Dormant.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::fact_fading_threshold`.
pub const DEFAULT_STAGE_FADING_THRESHOLD: f64 = 0.3;
/// Default decay score threshold for transitioning from Dormant to Archived.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::fact_dormant_threshold`.
pub const DEFAULT_STAGE_DORMANT_THRESHOLD: f64 = 0.1;

impl KnowledgeStage {
    /// Determine the lifecycle stage from a decay score in [0.0, 1.0].
    ///
    /// Uses default thresholds. Prefer [`from_decay_score_with_thresholds`](Self::from_decay_score_with_thresholds)
    /// when taxis config is available.
    #[must_use]
    pub fn from_decay_score(decay_score: f64) -> Self {
        Self::from_decay_score_with_thresholds(
            decay_score,
            DEFAULT_STAGE_ACTIVE_THRESHOLD,
            DEFAULT_STAGE_FADING_THRESHOLD,
            DEFAULT_STAGE_DORMANT_THRESHOLD,
        )
    }

    /// Determine the lifecycle stage from a decay score with configurable thresholds.
    ///
    /// Thresholds are sourced from `taxis::config::AgentBehaviorDefaults`:
    /// `fact_active_threshold`, `fact_fading_threshold`, `fact_dormant_threshold`.
    #[must_use]
    pub fn from_decay_score_with_thresholds(
        decay_score: f64,
        active_threshold: f64,
        fading_threshold: f64,
        dormant_threshold: f64,
    ) -> Self {
        if decay_score >= active_threshold {
            Self::Active
        } else if decay_score >= fading_threshold {
            Self::Fading
        } else if decay_score >= dormant_threshold {
            Self::Dormant
        } else {
            Self::Archived
        }
    }

    /// Return the `snake_case` string representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Fading => "fading",
            Self::Dormant => "dormant",
            Self::Archived => "archived",
        }
    }

    /// Whether this stage is eligible for graduated pruning.
    ///
    /// Only `Archived` facts may be permanently removed.
    #[must_use]
    pub fn is_prunable(self) -> bool {
        matches!(self, Self::Archived)
    }

    /// Whether facts in this stage should appear in default recall results.
    #[must_use]
    pub fn in_default_recall(self) -> bool {
        matches!(self, Self::Active | Self::Fading)
    }
}

impl std::str::FromStr for KnowledgeStage {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "fading" => Ok(Self::Fading),
            "dormant" => Ok(Self::Dormant),
            "archived" => Ok(Self::Archived),
            other => Err(format!("unknown knowledge stage: {other}")),
        }
    }
}

/// Record of a stage transition for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTransition {
    /// The fact that transitioned.
    pub fact_id: FactId,
    /// Previous stage.
    pub from: KnowledgeStage,
    /// New stage.
    pub to: KnowledgeStage,
    /// Decay score that triggered the transition.
    pub decay_score: f64,
    /// When the transition occurred.
    pub transitioned_at: jiff::Timestamp,
}

/// Reason for intentionally forgetting a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ForgetReason {
    /// User explicitly requested removal.
    UserRequested,
    /// Fact is outdated.
    Outdated,
    /// Fact is incorrect.
    Incorrect,
    /// Privacy concern.
    Privacy,
    /// Skill retired due to prolonged inactivity (decay score below threshold).
    Stale,
    /// Replaced by a newer or better skill during deduplication.
    Superseded,
    /// Contradicted by a newer extraction during auto-dream consolidation.
    Contradicted,
}

impl ForgetReason {
    /// Return the `snake_case` string representation of this reason.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserRequested => "user_requested",
            Self::Outdated => "outdated",
            Self::Incorrect => "incorrect",
            Self::Privacy => "privacy",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Contradicted => "contradicted",
        }
    }
}

impl std::str::FromStr for ForgetReason {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "user_requested" => Ok(Self::UserRequested),
            "outdated" => Ok(Self::Outdated),
            "incorrect" => Ok(Self::Incorrect),
            "privacy" => Ok(Self::Privacy),
            "stale" => Ok(Self::Stale),
            "superseded" => Ok(Self::Superseded),
            "contradicted" => Ok(Self::Contradicted),
            other => Err(format!("unknown forget reason: {other}")),
        }
    }
}

/// Classification of a fact for FSRS decay stability defaults.
///
/// Each variant carries a base stability (hours) calibrated to spaced repetition
/// research. Higher stability means slower decay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FactType {
    /// "My name is X": very stable (2 years).
    Identity,
    /// "I prefer tabs": stable (1 year).
    Preference,
    /// "I know Rust": moderately stable (6 months).
    Skill,
    /// "X works at Y": moderate (3 months).
    Relationship,
    /// "We discussed X": short-lived (30 days).
    Event,
    /// "TODO: fix bug": ephemeral (7 days).
    Task,
    /// "Build was slow": very ephemeral (3 days).
    Observation,
    /// Self-audit result: short-lived (30 days).
    Audit,
    /// Claim-source provenance check: ephemeral (7 days).
    Verification,
    /// Operational metric snapshot: ephemeral (3 days).
    Operational,
}

impl FactType {
    /// Base stability in hours for FSRS power-law decay.
    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "Audit/Event share 30-day decay, Task/Verification share 7-day decay, Observation/Operational share 3-day decay, but are semantically distinct"
    )]
    pub fn base_stability_hours(self) -> f64 {
        match self {
            Self::Identity => 17_520.0,
            Self::Preference => 8_760.0,
            Self::Skill => 4_380.0,
            Self::Relationship => 2_190.0,
            Self::Event => 720.0,
            Self::Task => 168.0,
            Self::Observation => 72.0,
            Self::Audit => 720.0,
            Self::Verification => 168.0,
            Self::Operational => 72.0,
        }
    }

    /// Classify a fact by its text content using keyword heuristics.
    ///
    /// Falls back to [`FactType::Observation`] when no pattern matches.
    /// Audit facts are identified by `fact_type` field, not content heuristics.
    #[must_use]
    pub fn classify(content: &str) -> Self {
        let lower = content.to_lowercase();
        if lower.contains("i am") || lower.contains("my name") || lower.contains("i identify") {
            Self::Identity
        } else if lower.contains("i prefer")
            || lower.contains("i like")
            || lower.contains("i don't like")
            || lower.contains("i do not like")
        {
            Self::Preference
        } else if lower.contains("i know")
            || lower.contains("i use")
            || lower.contains("i work with")
        {
            Self::Skill
        } else if lower.contains("todo") || lower.contains("need to") || lower.contains("should") {
            Self::Task
        } else if lower.contains("yesterday")
            || lower.contains("last week")
            || lower.contains("last month")
            || lower.contains("last year")
            || lower.contains("today")
        {
            Self::Event
        } else if contains_named_entity_relationship(&lower) {
            Self::Relationship
        } else {
            Self::Observation
        }
    }

    /// Return the lowercase string representation of this fact type.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::Skill => "skill",
            Self::Relationship => "relationship",
            Self::Event => "event",
            Self::Task => "task",
            Self::Observation => "observation",
            Self::Audit => "audit",
            Self::Verification => "verification",
            Self::Operational => "operational",
        }
    }

    /// Parse from a string, falling back to [`FactType::Observation`] for unknown values.
    #[must_use]
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "identity" => Self::Identity,
            "preference" => Self::Preference,
            "skill" => Self::Skill,
            "relationship" => Self::Relationship,
            "event" => Self::Event,
            "task" => Self::Task,
            "audit" => Self::Audit,
            "verification" => Self::Verification,
            "operational" => Self::Operational,
            // WHY: Unknown values fall back to Observation to keep the type system open.
            _ => Self::Observation,
        }
    }
}

/// How a verification claim was checked against ground truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum VerificationSource {
    /// Shell command whose output is compared against the claim.
    Command,
    /// Database or API query returning structured data.
    Query,
    /// Arithmetic re-derivation (e.g. sum checks, percentage recalculation).
    Arithmetic,
    /// Cross-reference against an authoritative document or fact.
    Reference,
}

impl VerificationSource {
    /// Return the `snake_case` string representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Query => "query",
            Self::Arithmetic => "arithmetic",
            Self::Reference => "reference",
        }
    }

    /// Parse from a string, returning `None` for unknown values.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "command" => Some(Self::Command),
            "query" => Some(Self::Query),
            "arithmetic" => Some(Self::Arithmetic),
            "reference" => Some(Self::Reference),
            _ => None,
        }
    }
}

/// Outcome of a verification check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum VerificationStatus {
    /// Actual value matches expected within tolerance.
    Pass,
    /// Actual value diverges from expected beyond tolerance.
    Fail,
    /// Verification result is older than the staleness threshold.
    Stale,
}

impl VerificationStatus {
    /// Return the `snake_case` string representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Stale => "stale",
        }
    }

    /// Parse from a string, returning `None` for unknown values.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            "stale" => Some(Self::Stale),
            _ => None,
        }
    }
}

/// Structured record of a claim-source provenance check.
///
/// Stored as JSON in the `content` field of a `Fact` with
/// `fact_type = "verification"`. Captures what was claimed, how it was
/// checked, and whether the check passed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationRecord {
    /// The assertion being verified (e.g. "build succeeded", "total is 383").
    pub claim: String,
    /// How the claim was checked against ground truth.
    pub source: VerificationSource,
    /// The value the claim asserts.
    pub expected: serde_json::Value,
    /// The value observed from the source.
    pub actual: serde_json::Value,
    /// Acceptable relative deviation before marking as `Fail` (0.0 = exact match).
    pub tolerance: f64,
    /// Outcome of the comparison.
    pub status: VerificationStatus,
    /// When the verification was performed.
    pub verified_at: jiff::Timestamp,
}

/// Heuristic: content mentions a named relationship pattern (e.g. "works at", "reports to").
fn contains_named_entity_relationship(lower: &str) -> bool {
    lower.contains("works at")
        || lower.contains("works for")
        || lower.contains("reports to")
        || lower.contains("manages")
        || lower.contains("member of")
        || lower.contains("belongs to")
}

/// Default FSRS stability by fact type string (hours until 50% recall probability).
///
/// Prefer [`FactType::base_stability_hours`] for typed access. This function
/// exists for backward compatibility with string-typed `fact_type` fields.
#[must_use]
pub fn default_stability_hours(fact_type: &str) -> f64 {
    FactType::from_str_lossy(fact_type).base_stability_hours()
}

/// Sentinel timestamp representing "current / no end date" in bi-temporal facts.
///
/// Uses `9999-01-01T00:00:00Z` as the far-future sentinel. The previous string
/// convention was `"9999-12-31"`, but jiff's `Timestamp` range caps at ~9999-04,
/// so we use January 1 to stay well within bounds.
///
/// The sentinel is stored as the string `"9999-01-01T00:00:00Z"` in Datalog,
/// so existing data using `"9999-12-31"` must be treated equivalently (any year-9999
/// timestamp means "no end date").
#[must_use]
pub fn far_future() -> jiff::Timestamp {
    jiff::civil::date(9999, 1, 1)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .unwrap_or_default() // SAFETY: 9999-01-01 is a valid Gregorian date
        .timestamp()
}

/// Check whether a timestamp represents the "no end date" sentinel.
///
/// Returns `true` for any timestamp in year 9999, accommodating both the new
/// `9999-01-01` sentinel and legacy `9999-12-31` strings.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "bi-temporal validity check used by knowledge_test.rs"
    )
)]
pub(crate) fn is_far_future(ts: &jiff::Timestamp) -> bool {
    let s = format_timestamp(ts);
    s.starts_with("9999-")
}

/// Parse an ISO 8601 string into a `jiff::Timestamp`.
///
/// Handles both full timestamps (`2026-01-01T00:00:00Z`) and date-only (`2026-01-01`)
/// by assuming UTC midnight for date-only strings.
///
/// Legacy `9999-12-31` sentinels (which overflow jiff's range) are mapped to
/// [`far_future()`].
///
/// Returns `None` for empty or unparseable strings.
#[must_use]
pub fn parse_timestamp(s: &str) -> Option<jiff::Timestamp> {
    if s.is_empty() {
        return None;
    }
    // WHY: jiff cannot represent 9999-12-31; 9999-01-01 is the far-future sentinel.
    if s.starts_with("9999-") {
        return Some(far_future());
    }
    if let Ok(ts) = s.parse::<jiff::Timestamp>() {
        return Some(ts);
    }
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        return Some(
            date.to_zoned(jiff::tz::TimeZone::UTC)
                .unwrap_or_default() // SAFETY: UTC conversion of a valid parsed date is infallible
                .timestamp(),
        );
    }
    None
}

/// Format a `jiff::Timestamp` as an ISO 8601 string for Datalog storage.
#[must_use]
pub fn format_timestamp(ts: &jiff::Timestamp) -> String {
    ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Diff between two temporal snapshots of the knowledge base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactDiff {
    /// Facts that became valid in the interval.
    pub added: Vec<Fact>,
    /// Facts where `valid_from` is before the interval but content or metadata changed.
    /// Tuple: (old version, new version).
    pub modified: Vec<(Fact, Fact)>,
    /// Facts whose `valid_to` fell within the interval.
    pub removed: Vec<Fact>,
}
