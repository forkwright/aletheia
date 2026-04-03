//! Knowledge domain types: facts, entities, relationships, and embeddings.
//!
//! These are the core data structures for the knowledge graph:
//! - **Facts**: extracted from conversations, bi-temporal (`valid_from`/`valid_to`)
//! - **Entities**: people, projects, tools, concepts with typed relationships
//! - **Vectors**: embedding-indexed for semantic recall
//! - **Memory scopes**: team memory sharing model (`User`, `Feedback`, `Project`, `Reference`)
//! - **Path validation layers**: defense-in-depth security for memory path operations

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::id::{EmbeddingId, EntityId, FactId};

/// Implement `Display` by delegating to `as_str()`.
macro_rules! display_via_as_str {
    ($($ty:ty),+ $(,)?) => {$(
        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    )+};
}

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

/// An entity in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier.
    pub id: EntityId,
    /// Display name.
    pub name: String,
    /// Entity type (person, project, tool, concept, etc.).
    pub entity_type: String,
    /// Known aliases.
    pub aliases: Vec<String>,
    /// When first observed.
    pub created_at: jiff::Timestamp,
    /// When last updated.
    pub updated_at: jiff::Timestamp,
}

/// A relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Source entity ID.
    pub src: EntityId,
    /// Target entity ID.
    pub dst: EntityId,
    /// Relationship type (e.g. `works_on`, `knows`, `depends_on`).
    pub relation: String,
    /// Relationship weight/strength (0.0--1.0).
    pub weight: f64,
    /// When first observed.
    pub created_at: jiff::Timestamp,
}

/// A vector embedding for semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedChunk {
    /// Unique identifier.
    pub id: EmbeddingId,
    /// The text that was embedded.
    pub content: String,
    /// Source type (fact, message, note, document).
    pub source_type: String,
    /// Source ID (fact ID, message `session_id:seq`, etc.).
    pub source_id: String,
    /// Which nous this belongs to (empty = shared).
    pub nous_id: String,
    /// The embedding vector (dimension depends on model).
    pub embedding: Vec<f32>,
    /// When embedded.
    pub created_at: jiff::Timestamp,
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
}

impl EpistemicTier {
    /// Return the lowercase string representation of this tier.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Inferred => "inferred",
            Self::Assumed => "assumed",
        }
    }

    /// FSRS stability multiplier: verified facts decay 2× slower than inferred.
    #[must_use]
    pub fn stability_multiplier(self) -> f64 {
        match self {
            Self::Verified => 2.0,
            Self::Inferred => 1.0,
            Self::Assumed => 0.5,
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

/// Decay score threshold for transitioning from Active to Fading.
const STAGE_ACTIVE_THRESHOLD: f64 = 0.7;
/// Decay score threshold for transitioning from Fading to Dormant.
const STAGE_FADING_THRESHOLD: f64 = 0.3;
/// Decay score threshold for transitioning from Dormant to Archived.
const STAGE_DORMANT_THRESHOLD: f64 = 0.1;

impl KnowledgeStage {
    /// Determine the lifecycle stage from a decay score in [0.0, 1.0].
    #[must_use]
    pub fn from_decay_score(decay_score: f64) -> Self {
        if decay_score >= STAGE_ACTIVE_THRESHOLD {
            Self::Active
        } else if decay_score >= STAGE_FADING_THRESHOLD {
            Self::Fading
        } else if decay_score >= STAGE_DORMANT_THRESHOLD {
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
    /// Chiron self-audit result: short-lived (30 days).
    Audit,
    /// Claim-source provenance check: ephemeral (7 days).
    Verification,
}

impl FactType {
    /// Base stability in hours for FSRS power-law decay.
    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "Audit/Event share 30-day decay, Task/Verification share 7-day decay, but are semantically distinct"
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
#[expect(
    clippy::expect_used,
    reason = "date(9999, 1, 1) is a valid Gregorian date and UTC conversion is infallible"
)]
pub fn far_future() -> jiff::Timestamp {
    jiff::civil::date(9999, 1, 1)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .expect("valid far-future date") // SAFETY: 9999-01-01 is a valid Gregorian date
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
#[expect(
    clippy::expect_used,
    reason = "UTC timezone conversion for a valid parsed date is infallible"
)]
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
                .expect("valid UTC conversion") // SAFETY: UTC conversion of a valid parsed date is infallible
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

/// Temporal ordering between cause and effect in a causal edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TemporalOrdering {
    /// Cause precedes effect in time.
    Before,
    /// Effect precedes cause in time (retroactive causation).
    After,
    /// Cause and effect are concurrent.
    Concurrent,
}

impl TemporalOrdering {
    /// Return the lowercase string representation of this ordering.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
            Self::Concurrent => "concurrent",
        }
    }
}

impl std::str::FromStr for TemporalOrdering {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "before" => Ok(Self::Before),
            "after" => Ok(Self::After),
            "concurrent" => Ok(Self::Concurrent),
            other => Err(format!("unknown temporal ordering: {other}")),
        }
    }
}

/// A directed causal edge between two fact nodes in the knowledge graph.
///
/// Represents "cause leads to effect" with temporal ordering and confidence.
/// Confidence propagates through causal chains: transitive confidence is the
/// product of individual edge confidences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEdge {
    /// Fact ID of the cause node.
    pub cause: FactId,
    /// Fact ID of the effect node.
    pub effect: FactId,
    /// Temporal ordering between cause and effect.
    pub ordering: TemporalOrdering,
    /// Confidence that this causal relationship holds (0.0--1.0).
    pub confidence: f64,
    /// When this edge was recorded.
    pub created_at: jiff::Timestamp,
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

/// Results from a semantic recall query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    /// The matching fact or chunk content.
    pub content: String,
    /// Distance/similarity score (lower = more similar for L2/cosine).
    pub distance: f64,
    /// Source type.
    pub source_type: String,
    /// Source ID.
    pub source_id: String,
}

// ---------------------------------------------------------------------------
// Team memory: scopes, access policies, and path validation layers
// ---------------------------------------------------------------------------

/// Memory sharing scope for multi-agent team memory.
///
/// Each scope maps to a subdirectory under the memory root and defines
/// distinct access control semantics. Scopes form the authorization
/// boundary that [`ScopeAccessPolicy`] enforces; path validation then
/// confirms the resolved filesystem path falls within the correct scope
/// directory.
///
/// Taxonomy mirrors the CC memory type model (`user`, `feedback`,
/// `project`, `reference`) from `memoryTypes.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryScope {
    /// Private to the user, never shared with other agents.
    ///
    /// WHY: User memories contain personal context (role, preferences,
    /// knowledge level) that should not leak across agent boundaries.
    User,
    /// Selectively shared corrections and preferences, write-gated to the user.
    ///
    /// WHY: Feedback memories encode behavioral guidance. Agents read them
    /// to avoid repeating mistakes, but only the user can write because
    /// agent-written feedback creates self-reinforcing loops.
    Feedback,
    /// Shared across all agents in a workspace, read-write.
    ///
    /// WHY: Project memories track ongoing work, deadlines, and decisions
    /// that every agent in the workspace needs visibility into.
    Project,
    /// Hybrid: agents read, user curates write access.
    ///
    /// WHY: Reference memories point to external systems (Linear, Grafana,
    /// Slack). Agents need to read them for context but the user controls
    /// what gets indexed because stale pointers are worse than no pointers.
    Reference,
}

impl MemoryScope {
    /// All scope variants in definition order.
    pub const ALL: [Self; 4] = [Self::User, Self::Feedback, Self::Project, Self::Reference];

    /// Return the lowercase string representation of this scope.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
        }
    }

    /// Directory name for this scope under the memory root.
    ///
    /// Each scope maps to a single subdirectory: `<memory_root>/<dir_name>/`.
    /// The name is identical to `as_str()` by convention.
    #[must_use]
    pub fn as_dir_name(self) -> &'static str {
        // WHY: Directory names match the enum's string representation to keep
        // the mapping predictable and greppable.
        self.as_str()
    }

    /// Access control policy for this scope.
    ///
    /// Returns the static [`ScopeAccessPolicy`] that describes who can
    /// read and write within this scope boundary.
    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "Feedback and Reference share the same access policy values but are semantically distinct scopes with different sharing intent"
    )]
    pub fn access_policy(self) -> ScopeAccessPolicy {
        match self {
            Self::User => ScopeAccessPolicy {
                agent_read: false,
                agent_write: false,
                user_write_only: true,
            },
            Self::Feedback => ScopeAccessPolicy {
                agent_read: true,
                agent_write: false,
                user_write_only: true,
            },
            Self::Project => ScopeAccessPolicy {
                agent_read: true,
                agent_write: true,
                user_write_only: false,
            },
            Self::Reference => ScopeAccessPolicy {
                agent_read: true,
                agent_write: false,
                user_write_only: true,
            },
        }
    }

    /// Parse from a string, returning `None` for unknown values.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "feedback" => Some(Self::Feedback),
            "project" => Some(Self::Project),
            "reference" => Some(Self::Reference),
            _ => None,
        }
    }
}

impl std::str::FromStr for MemoryScope {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::from_str_opt(s).ok_or_else(|| format!("unknown memory scope: {s}"))
    }
}

/// Access control policy for a [`MemoryScope`].
///
/// Defines who can read and write within a scope boundary. The policy is
/// static per scope variant -- it does not change at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeAccessPolicy {
    /// Whether agents can read memories in this scope.
    pub agent_read: bool,
    /// Whether agents can write memories in this scope.
    pub agent_write: bool,
    /// Whether only the user can write (agent writes are rejected).
    pub user_write_only: bool,
}

impl ScopeAccessPolicy {
    /// Whether an agent is allowed to perform a write operation in this scope.
    #[must_use]
    pub fn permits_agent_write(&self) -> bool {
        self.agent_write && !self.user_write_only
    }

    /// Whether an agent is allowed to perform a read operation in this scope.
    #[must_use]
    pub fn permits_agent_read(&self) -> bool {
        self.agent_read
    }
}

/// Validation layer in the defense-in-depth path security model.
///
/// Each layer addresses a distinct class of path manipulation attack.
/// Layers are applied in order during `validate_memory_path()` (in mneme);
/// a path must pass all layers. The variant names map 1:1 to
/// `PathValidationError` variants for error classification and logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PathValidationLayer {
    /// Null bytes truncate paths in C-based syscalls (libc, kernel).
    NullByte,
    /// Raw string checks miss `foo/../../../etc/passwd`; resolved via
    /// `std::path::Path::components()`.
    Canonicalization,
    /// Symlinks can escape directory jails; resolved via
    /// `std::fs::canonicalize()` with root containment check.
    SymlinkResolution,
    /// Dangling symlinks indicate filesystem manipulation; detected via
    /// `std::fs::symlink_metadata()` when canonicalize returns ENOENT.
    DanglingSymlink,
    /// Symlink loops cause infinite recursion; capped at 40 hops matching
    /// the Linux `ELOOP` limit.
    LoopDetection,
    /// URL-encoded traversals (`%2e%2e%2f` = `../`) bypass string-level
    /// checks; detected by percent-decoding then re-checking for `..` or
    /// separator characters.
    UrlEncodedTraversal,
    /// Fullwidth characters (U+FF0E `.`, U+FF0F `/`) normalize to ASCII
    /// separators under NFKC; detected by normalizing and comparing to
    /// the original.
    UnicodeNormalization,
    /// Resolved path falls outside the expected scope subdirectory.
    ScopeContainment,
}

/// Total number of filesystem-level validation layers (excluding scope
/// containment which is a logical check).
pub const PATH_VALIDATION_FS_LAYERS: usize = 7;

/// Maximum symlink hops before declaring a loop, matching the Linux
/// `ELOOP` kernel limit.
pub const SYMLINK_HOP_LIMIT: usize = 40;

impl PathValidationLayer {
    /// All layer variants in application order.
    pub const ALL: [Self; 8] = [
        Self::NullByte,
        Self::Canonicalization,
        Self::SymlinkResolution,
        Self::DanglingSymlink,
        Self::LoopDetection,
        Self::UrlEncodedTraversal,
        Self::UnicodeNormalization,
        Self::ScopeContainment,
    ];

    /// Return the `snake_case` string representation of this layer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NullByte => "null_byte",
            Self::Canonicalization => "canonicalization",
            Self::SymlinkResolution => "symlink_resolution",
            Self::DanglingSymlink => "dangling_symlink",
            Self::LoopDetection => "loop_detection",
            Self::UrlEncodedTraversal => "url_encoded_traversal",
            Self::UnicodeNormalization => "unicode_normalization",
            Self::ScopeContainment => "scope_containment",
        }
    }

    /// Whether this layer requires filesystem I/O.
    ///
    /// Pure string-based layers (`NullByte`, `Canonicalization`,
    /// `UrlEncodedTraversal`, `UnicodeNormalization`, `ScopeContainment`)
    /// can run without touching the filesystem.
    #[must_use]
    pub fn requires_io(self) -> bool {
        matches!(
            self,
            Self::SymlinkResolution | Self::DanglingSymlink | Self::LoopDetection
        )
    }
}

impl std::str::FromStr for PathValidationLayer {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "null_byte" => Ok(Self::NullByte),
            "canonicalization" => Ok(Self::Canonicalization),
            "symlink_resolution" => Ok(Self::SymlinkResolution),
            "dangling_symlink" => Ok(Self::DanglingSymlink),
            "loop_detection" => Ok(Self::LoopDetection),
            "url_encoded_traversal" => Ok(Self::UrlEncodedTraversal),
            "unicode_normalization" => Ok(Self::UnicodeNormalization),
            "scope_containment" => Ok(Self::ScopeContainment),
            other => Err(format!("unknown path validation layer: {other}")),
        }
    }
}

// ── Path validation error ────────────────────────────────────────────────

/// Error from defense-in-depth path validation.
///
/// Each variant maps 1:1 to a [`PathValidationLayer`], providing
/// structured information about which layer rejected the path and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "variant fields (path, scope, hops, etc.) are self-documenting by name"
)]
pub enum PathValidationError {
    /// Path contains null bytes that would truncate C-level syscalls.
    NullByte { path: String },
    /// Path contains `..` or backslash components enabling directory traversal.
    Canonicalization { path: String, component: String },
    /// Symlink resolves outside the allowed root directory.
    SymlinkResolution { path: PathBuf, root: PathBuf },
    /// Symlink target does not exist (filesystem manipulation indicator).
    DanglingSymlink { path: PathBuf },
    /// Symlink chain exceeds the hop limit (loop indicator).
    LoopDetection { path: PathBuf, hops: usize },
    /// URL-encoded traversal characters detected (`%2e`, `%2f`, `%5c`).
    UrlEncodedTraversal {
        path: String,
        decoded_fragment: String,
    },
    /// Fullwidth Unicode characters that normalize to path separators under NFKC.
    UnicodeNormalization { path: String, offending_char: char },
    /// Resolved path falls outside the expected scope subdirectory.
    ScopeContainment {
        path: PathBuf,
        scope: MemoryScope,
        expected_dir: PathBuf,
    },
}

impl PathValidationError {
    /// The validation layer that rejected the path.
    #[must_use]
    pub fn layer(&self) -> PathValidationLayer {
        match self {
            Self::NullByte { .. } => PathValidationLayer::NullByte,
            Self::Canonicalization { .. } => PathValidationLayer::Canonicalization,
            Self::SymlinkResolution { .. } => PathValidationLayer::SymlinkResolution,
            Self::DanglingSymlink { .. } => PathValidationLayer::DanglingSymlink,
            Self::LoopDetection { .. } => PathValidationLayer::LoopDetection,
            Self::UrlEncodedTraversal { .. } => PathValidationLayer::UrlEncodedTraversal,
            Self::UnicodeNormalization { .. } => PathValidationLayer::UnicodeNormalization,
            Self::ScopeContainment { .. } => PathValidationLayer::ScopeContainment,
        }
    }
}

impl std::fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NullByte { path } => write!(f, "null byte in path: {path}"),
            Self::Canonicalization { path, component } => {
                write!(
                    f,
                    "directory traversal component `{component}` in path: {path}"
                )
            }
            Self::SymlinkResolution { path, root } => {
                write!(
                    f,
                    "symlink at {} resolves outside root {}",
                    path.display(),
                    root.display()
                )
            }
            Self::DanglingSymlink { path } => {
                write!(f, "dangling symlink at {}", path.display())
            }
            Self::LoopDetection { path, hops } => {
                write!(f, "symlink loop at {} after {hops} hops", path.display())
            }
            Self::UrlEncodedTraversal {
                path,
                decoded_fragment,
            } => {
                write!(
                    f,
                    "URL-encoded traversal `{decoded_fragment}` in path: {path}"
                )
            }
            Self::UnicodeNormalization {
                path,
                offending_char,
            } => {
                write!(
                    f,
                    "fullwidth character U+{:04X} in path: {path}",
                    u32::from(*offending_char)
                )
            }
            Self::ScopeContainment {
                path,
                scope,
                expected_dir,
            } => {
                write!(
                    f,
                    "path {} escapes {} scope (expected under {})",
                    path.display(),
                    scope.as_str(),
                    expected_dir.display()
                )
            }
        }
    }
}

impl std::error::Error for PathValidationError {}

display_via_as_str!(
    EpistemicTier,
    KnowledgeStage,
    ForgetReason,
    FactType,
    VerificationSource,
    VerificationStatus,
    TemporalOrdering,
    MemoryScope,
    PathValidationLayer,
);

// ── Validated path newtype ───────────────────────────────────────────────

/// A filesystem path that has passed all defense-in-depth validation layers.
///
/// This newtype can only be constructed through [`validate_memory_path()`],
/// ensuring that path security validation cannot be bypassed. The inner
/// `PathBuf` is private, so callers must go through the validation function
/// to obtain an instance.
///
/// Provides [`read()`](Self::read) and [`write()`](Self::write) methods
/// that gate all memory I/O through validated paths, making it impossible
/// to perform memory file operations without passing validation first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPath {
    inner: PathBuf,
    scope: MemoryScope,
}

impl ValidatedPath {
    /// The validated filesystem path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.inner
    }

    /// The memory scope this path was validated against.
    #[must_use]
    pub fn scope(&self) -> MemoryScope {
        self.scope
    }

    /// Consume the wrapper and return the inner `PathBuf`.
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.inner
    }

    /// Read the validated file's contents.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the file cannot be read.
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        std::fs::read(&self.inner)
    }

    /// Write data to the validated path, creating parent directories as needed.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if directories cannot be created or the file
    /// cannot be written.
    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = self.inner.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.inner, data)
    }
}

impl AsRef<Path> for ValidatedPath {
    fn as_ref(&self) -> &Path {
        &self.inner
    }
}

impl std::fmt::Display for ValidatedPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner.display())
    }
}

// ── Path validation function ─────────────────────────────────────────────

/// Validate a memory path against all defense-in-depth security layers.
///
/// Applies each [`PathValidationLayer`] in order. The path must pass all
/// layers to produce a [`ValidatedPath`]. Relative paths are resolved
/// against `root/scope_dir/`; absolute paths are checked directly against
/// the scope boundary.
///
/// # Layers (applied in order)
///
/// 1. **Null byte** — reject `\0` characters
/// 2. **Canonicalization** — reject `..` and backslash components
/// 3. **URL-encoded traversal** — detect `%2e`, `%2f`, `%5c`
/// 4. **Unicode normalization** — detect fullwidth `.` `/` `\` characters
/// 5. **Scope containment** — resolved path must be under `root/scope_dir/`
/// 6. **Symlink resolution** — canonical path must stay within root (I/O)
/// 7. **Dangling symlink / loop detection** — reject broken or looping
///    symlinks (I/O)
///
/// # Errors
///
/// Returns [`PathValidationError`] identifying the first layer that
/// rejected the path, with structured context for logging and diagnostics.
pub fn validate_memory_path(
    path: &Path,
    root: &Path,
    scope: MemoryScope,
) -> std::result::Result<ValidatedPath, PathValidationError> {
    let path_str = path.to_string_lossy();

    // Layer 1: NullByte — reject null bytes that truncate C-level syscalls.
    if path_str.contains('\0') {
        return Err(PathValidationError::NullByte {
            path: path_str.into_owned(),
        });
    }

    // Layer 2: Canonicalization — reject `..` components and backslashes.
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(PathValidationError::Canonicalization {
                path: path_str.into_owned(),
                component: "..".to_owned(),
            });
        }
    }
    if path_str.contains('\\') {
        return Err(PathValidationError::Canonicalization {
            path: path_str.into_owned(),
            component: "\\".to_owned(),
        });
    }

    // Layer 3: URL-encoded traversal — detect percent-encoded separators.
    let lower = path_str.to_ascii_lowercase();
    for pattern in &["%2e", "%2f", "%5c"] {
        if lower.contains(pattern) {
            return Err(PathValidationError::UrlEncodedTraversal {
                path: path_str.into_owned(),
                decoded_fragment: (*pattern).to_owned(),
            });
        }
    }

    // Layer 4: Unicode normalization — detect fullwidth characters that
    // normalize to ASCII separators under NFKC (U+FF0E → '.', U+FF0F → '/',
    // U+FF3C → '\').
    for ch in path_str.chars() {
        if matches!(ch, '\u{FF0E}' | '\u{FF0F}' | '\u{FF3C}') {
            return Err(PathValidationError::UnicodeNormalization {
                path: path_str.into_owned(),
                offending_char: ch,
            });
        }
    }

    // Resolve the full path: relative paths are joined with scope_dir.
    let scope_dir = root.join(scope.as_dir_name());
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        scope_dir.join(path)
    };
    let normalized = normalize_path_components(&full_path);

    // Layer 5: Scope containment — resolved path must stay within scope_dir.
    if !normalized.starts_with(&scope_dir) {
        return Err(PathValidationError::ScopeContainment {
            path: normalized,
            scope,
            expected_dir: scope_dir,
        });
    }

    // Layers 6–7: Symlink resolution, dangling symlink, loop detection (I/O).
    // WHY: Only checked when the path exists on the filesystem. Pure string
    // layers above have already validated the path structure for paths that
    // don't yet exist.
    validate_symlinks(&normalized, root, &scope_dir, scope)?;

    Ok(ValidatedPath {
        inner: normalized,
        scope,
    })
}

/// Normalize path components without filesystem access.
///
/// Resolves `.` (current dir) by skipping and `..` (parent dir) by
/// popping. This is a string-level operation; no symlinks are resolved.
fn normalize_path_components(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for c in path.components() {
        match c {
            // WHY: ParentDir should already be rejected by Layer 2, but
            // defense-in-depth means we handle it here too.
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            other => parts.push(other),
        }
    }
    parts.iter().collect()
}

/// Check symlink-related security layers on a path that exists on the filesystem.
///
/// Only performs I/O when the path actually exists as a symlink. Skips
/// silently when the path does not exist, since the pure string layers
/// have already validated the path structure.
fn validate_symlinks(
    path: &Path,
    root: &Path,
    scope_dir: &Path,
    scope: MemoryScope,
) -> std::result::Result<(), PathValidationError> {
    // WHY: symlink_metadata returns info about the link itself (not its target),
    // so is_symlink() is accurate even for dangling links.
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(()); // Path doesn't exist yet; pure layers sufficient.
    };

    if !meta.file_type().is_symlink() {
        return Ok(()); // Not a symlink; no further checks needed.
    }

    // Resolve symlinks with hop counting for loop detection.
    let canonical = resolve_with_hop_limit(path)?;

    // Layer 6: Symlink resolution — canonical path must stay within root.
    if !canonical.starts_with(root) {
        return Err(PathValidationError::SymlinkResolution {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        });
    }

    // Re-check scope containment on the canonical path.
    if !canonical.starts_with(scope_dir) {
        return Err(PathValidationError::ScopeContainment {
            path: canonical,
            scope,
            expected_dir: scope_dir.to_path_buf(),
        });
    }

    Ok(())
}

/// Resolve a symlink chain with hop counting.
///
/// Returns the final resolved path, or a [`PathValidationError`] if the
/// chain exceeds [`SYMLINK_HOP_LIMIT`] (loop) or a link target doesn't
/// exist (dangling).
fn resolve_with_hop_limit(start: &Path) -> std::result::Result<PathBuf, PathValidationError> {
    let mut current = start.to_path_buf();
    let mut hops: usize = 0;

    loop {
        let Ok(meta) = std::fs::symlink_metadata(&current) else {
            return Err(PathValidationError::DanglingSymlink {
                path: start.to_path_buf(),
            });
        };

        if !meta.file_type().is_symlink() {
            return Ok(current);
        }

        hops += 1;
        if hops > SYMLINK_HOP_LIMIT {
            return Err(PathValidationError::LoopDetection {
                path: start.to_path_buf(),
                hops,
            });
        }

        let Ok(target) = std::fs::read_link(&current) else {
            return Err(PathValidationError::DanglingSymlink {
                path: start.to_path_buf(),
            });
        };

        current = if target.is_absolute() {
            target
        } else {
            // WHY: Relative symlink targets resolve from the link's parent.
            current
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .join(&target)
        };
    }
}

#[cfg(test)]
#[path = "knowledge_test.rs"]
mod tests;
