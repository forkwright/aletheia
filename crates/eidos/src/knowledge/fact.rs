// kanon:ignore RUST/file-too-long — coherent domain module; splitting would break temporal locality of related types
//! Fact domain types: extraction, classification, decay, timestamps.

use serde::{Deserialize, Serialize};

use crate::id::FactId;
use crate::workspace::ProjectId;

use super::MemoryScope;

/// Maximum byte length for fact content strings.
pub const MAX_CONTENT_LENGTH: usize = 102_400;

/// Bi-temporal validity and recording timestamps for a fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactTemporal {
    /// When this fact became true in the world (domain validity time).
    pub valid_from: jiff::Timestamp,
    /// When this fact ceased to be true in the world (domain validity time).
    ///
    /// Use [`far_future`](crate::knowledge::far_future) for facts that are
    /// currently valid.
    pub valid_to: jiff::Timestamp,
    /// When the system learned about this fact (system recording time).
    ///
    /// This is distinct from `valid_from`/`valid_to`, which describe when the
    /// fact was true in the domain, not when we recorded it.
    pub recorded_at: jiff::Timestamp,
}

/// Provenance: where a fact came from and how trustworthy it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactProvenance {
    /// Normalized confidence score in `[0.0, 1.0]`.
    pub confidence: f64,
    /// Epistemic confidence tier — how the fact was established.
    ///
    /// Tier reflects the epistemic basis (e.g. verified against ground truth,
    /// inferred from context, assumed, or derived from training outcomes).
    pub tier: EpistemicTier,
    /// Session that extracted or produced this fact, if known.
    pub source_session_id: Option<String>,
    /// Base FSRS stability in hours before the tier multiplier is applied.
    pub stability_hours: f64,
}

/// Lifecycle state for supersession and intentional forgetting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactLifecycle {
    /// ID of the fact that replaced this one, if any.
    pub superseded_by: Option<FactId>,
    /// Whether this fact has been intentionally forgotten.
    pub is_forgotten: bool,
    /// When the fact was forgotten, if it has been.
    pub forgotten_at: Option<jiff::Timestamp>,
    /// Why the fact was forgotten, if applicable.
    pub forget_reason: Option<ForgetReason>,
}

/// Access-tracking counters for FSRS decay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactAccess {
    /// Number of times this fact has been recalled.
    pub access_count: u32,
    /// Timestamp of the most recent recall, if any.
    pub last_accessed_at: Option<jiff::Timestamp>,
}

/// A memory fact extracted from conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Stable identifier for this fact.
    pub id: FactId,
    /// Agent (nous) that owns this fact.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id — cross-crate nous identifier from koina, serialized as string here
    /// Classification determining base decay behavior.
    pub fact_type: String,
    /// Human-readable fact statement.
    pub content: String,

    /// Memory sharing scope for team memory.
    ///
    /// `None` for facts created before the team memory model was introduced.
    /// New facts should always populate this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MemoryScope>,

    /// Git-remote-derived project partition for project-scoped behavioral facts.
    ///
    /// `None` means the fact is global or predates project partitioning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,

    /// Data-sovereignty classification gating which provider deployment
    /// targets may receive this fact during recall (#3404, #3413).
    ///
    /// Defaults to [`FactSensitivity::Public`] via `#[serde(default)]` so
    /// facts persisted before sensitivity tracking deserialize unchanged.
    #[serde(default)]
    pub sensitivity: FactSensitivity,

    /// Visibility level controlling how broadly this fact may be shared
    /// across agents, sessions, and external systems.
    ///
    /// Defaults to [`Visibility::Private`] via `#[serde(default)]` so
    /// facts persisted before visibility tracking deserialize unchanged.
    #[serde(default)]
    pub visibility: Visibility,

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

/// Data-sovereignty classification for a fact.
///
/// Controls which `DeploymentTarget` tiers may receive the fact during
/// recall. Variants are ordered `Public < Internal < Confidential` (least
/// restrictive → most restrictive) so admission reduces to a comparison
/// against the provider's target.
///
/// | Variant | Allowed targets |
/// |---------|----------------|
/// | `Public` | any provider |
/// | `Internal` | self-hosted / embedded only |
/// | `Confidential` | embedded (in-process) only |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
// kanon:ignore RUST/non-exhaustive-enum -- WHY: intentionally exhaustive; the data-sovereignty deployment-target model defines the complete variant set and downstream code matches it exhaustively
pub enum FactSensitivity {
    /// Safe for any provider, including cloud LLM providers.
    #[default]
    Public,
    /// Safe for local or self-hosted providers; must not leave the instance
    /// via cloud APIs.
    Internal,
    /// Never send to any external provider. Only embedded (in-process)
    /// providers may receive this fact.
    Confidential,
}

impl FactSensitivity {
    /// Return the lowercase string representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Confidential => "confidential",
        }
    }
}

impl std::str::FromStr for FactSensitivity {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "public" => Ok(Self::Public),
            "internal" => Ok(Self::Internal),
            "confidential" => Ok(Self::Confidential),
            other => Err(format!("unknown fact sensitivity: {other}")),
        }
    }
}

/// Visibility level for a fact within the knowledge graph.
///
/// Controls how broadly a fact may be shared across agents, sessions,
/// and external systems. Ordered from most restrictive to least.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Visibility {
    /// Visible only to the originating agent / user.
    #[default]
    Private,
    /// Visible to agents within the same team or project scope.
    Shared,
    /// Visible to a defined allow-list of consumers.
    Restricted,
    /// Visible to any authorized consumer, including external integrations.
    Published,
}

impl Visibility {
    /// Return the `snake_case` string representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
            Self::Restricted => "restricted",
            Self::Published => "published",
        }
    }
}

impl std::str::FromStr for Visibility {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "private" => Ok(Self::Private),
            "shared" => Ok(Self::Shared),
            "restricted" => Ok(Self::Restricted),
            "published" => Ok(Self::Published),
            other => Err(format!("unknown fact visibility: {other}")),
        }
    }
}

/// Epistemic confidence tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EpistemicTier {
    /// Checked against ground truth.
    Verified,
    /// Produced by self-reflection or meta-cognitive review.
    Reflected,
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
            Self::Reflected => "reflected",
            Self::Inferred => "inferred",
            Self::Assumed => "assumed",
            Self::Training => "training",
        }
    }

    /// FSRS stability multiplier applied to base stability.
    ///
    /// | Tier | Multiplier | Why |
    /// |------|------------|-----|
    /// | `Verified` | 2.0 | Ground-truth-checked facts deserve slower decay. |
    /// | `Reflected` | 2.5 | Self-reflected facts are durable but not ground-truth. |
    /// | `Inferred` | 1.0 | Baseline for reasoned-but-unverified facts. |
    /// | `Assumed` | 0.5 | Unchecked assumptions decay faster to limit risk. |
    /// | `Training` | 4.0 | Training data is a permanent record of session outcomes; |
    /// | | | it persists indefinitely and is not subject to normal memory decay. |
    #[must_use]
    pub fn stability_multiplier(self) -> f64 {
        match self {
            Self::Verified => 2.0,
            Self::Reflected => 2.5,
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
        .unwrap_or_default() // kanon:ignore RUST/no-result-unwrap-or-default — 9999-01-01 is a valid Gregorian date and UTC has no DST gaps
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
                .unwrap_or_default() // kanon:ignore RUST/no-result-unwrap-or-default — date was just parsed successfully; UTC has no DST gaps
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

/// Identifier for a published fact (a copy-on-publish derivative of a private fact).
///
/// Per R716 design: when a nous publishes a fact, the original stays scoped
/// to the publisher and a `PublishedFact` is created with cross-nous visibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublishedFactId(pub String);

/// A fact that has been published for cross-nous visibility, retaining a link
/// to its original (publisher-private) source.
///
/// Verification and contestation are tracked here; the original `Fact` is
/// untouched. See R716 Phase 3 design
/// (`kanon/projects/aletheia/planning/research/knowledge-sharing.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedFact {
    /// Stable identifier for this published fact.
    pub id: PublishedFactId,
    /// The original (publisher-private) fact this was copied from.
    pub original_fact_id: FactId,
    /// Nous that published the fact.
    pub published_by: koina::id::NousId,
    /// When the fact was published.
    pub published_at: jiff::Timestamp,
    /// Number of independent nouses that have voted Accept on this fact.
    pub verification_count: u32,
    /// Nouses that have contested this fact.
    pub contested_by: Vec<koina::id::NousId>,
    /// Free-text reason associated with the most recent contest, if any.
    pub contest_reason: Option<String>,
}

/// Per-nous access grant for a fact under restricted visibility.
///
/// Schema-only ACL bookkeeping for the `Visibility::Restricted` case
/// (R716 Phase 1 carry-over; not enforced at recall time yet — Phase 4).
///
/// **NOTE:** distinct from [`FactAccess`] (access-counter struct above).
/// Named with `Grant` suffix to avoid the type collision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactAccessGrant {
    /// Fact this grant applies to.
    pub fact_id: FactId,
    /// Nous granted read access.
    pub grantee: koina::id::NousId,
    /// When the grant was issued.
    pub granted_at: jiff::Timestamp,
}

/// A pending verification proposal on a published fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationProposal {
    /// The fact under verification.
    pub fact_id: FactId,
    /// Nous that initiated the proposal.
    pub proposing_nous: koina::id::NousId,
    /// Tier the proposing nous wants to promote the fact to (typically `Verified`).
    pub proposed_tier: EpistemicTier,
    /// Votes cast so far.
    pub votes: Vec<VerificationVote>,
}

/// A single vote on a verification proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationVote {
    /// Voter nous.
    pub voter: koina::id::NousId,
    /// Verdict cast.
    pub verdict: VerificationVerdict,
    /// When the vote was cast.
    pub at: jiff::Timestamp,
}

/// Verdict a voter casts on a verification proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum VerificationVerdict {
    /// Voter agrees the fact merits the proposed tier.
    Accept,
    /// Voter disagrees and contests the fact.
    Contest,
    /// Voter declines to opine.
    Abstain,
}

/// Resolution of a cross-nous fact conflict.
///
/// Composite scoring per R716 design:
/// `score = 0.4 * confidence + 0.3 * tier_score + 0.2 * recency + 0.1 * supporter_count`
/// where:
/// - `confidence` ∈ [0.0, 1.0] is the fact's `FactProvenance::confidence`
/// - `tier_score` ∈ [0.0, 1.0] maps tier → numeric weight (Verified=1.0, Inferred=0.66, Assumed=0.33, Training=0.5)
/// - `recency` ∈ [0.0, 1.0] is `1.0` for facts recorded within the last 24h, decaying linearly to `0.0` at 30 days
/// - `supporter_count` is normalized as `min(1.0, supporters / 5.0)` (5 supporters saturates the term)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolution {
    /// Winning fact ID.
    pub winner: FactId,
    /// Losing fact ID(s); they retain `contested_by` provenance and are not deleted.
    pub losers: Vec<FactId>,
    /// Composite score that selected the winner.
    pub winning_score: f64,
    /// When the resolution was computed.
    pub resolved_at: jiff::Timestamp,
}

impl ConflictResolution {
    /// Compute the composite resolution score for a fact.
    ///
    /// `supporters` is the count of distinct nouses that have independently
    /// extracted or accepted this fact (typically `verification_count + 1`
    /// to include the publisher).
    ///
    /// `now` is the reference timestamp for recency normalization (callers
    /// pass `jiff::Timestamp::now()` in production; tests pass a fixed value
    /// for determinism).
    #[must_use]
    pub fn compute_score(fact: &Fact, supporters: u32, now: jiff::Timestamp) -> f64 {
        let confidence = fact.provenance.confidence.clamp(0.0, 1.0);
        let tier_score = match fact.provenance.tier {
            EpistemicTier::Verified => 1.0,
            EpistemicTier::Reflected => 0.83,
            EpistemicTier::Inferred => 0.66,
            EpistemicTier::Training => 0.5,
            EpistemicTier::Assumed => 0.33,
        };
        let recency = recency_score(fact.temporal.recorded_at, now);
        let supporter_norm = (f64::from(supporters) / 5.0).min(1.0);
        0.4 * confidence + 0.3 * tier_score + 0.2 * recency + 0.1 * supporter_norm
    }
}

/// Linearly decaying recency factor. `1.0` if `now - recorded_at` ≤ 24h,
/// `0.0` if ≥ 30 days, linear in between. Negative deltas (future timestamps)
/// clamp to `1.0`.
fn recency_score(recorded_at: jiff::Timestamp, now: jiff::Timestamp) -> f64 {
    let delta = now.duration_since(recorded_at);
    if delta.is_negative() {
        return 1.0;
    }
    let hours = delta.as_secs_f64() / 3600.0;
    if hours <= 24.0 {
        1.0
    } else if hours >= 24.0 * 30.0 {
        0.0
    } else {
        let span = 24.0 * 30.0 - 24.0;
        ((24.0 * 30.0) - hours) / span
    }
}
