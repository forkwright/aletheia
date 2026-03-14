//! Knowledge store — facts, entities, and vectors via `CozoDB`.
//!
//! Complements the `SQLite` session store with structured knowledge:
//! - **Facts**: extracted from conversations, bi-temporal (`valid_from`/`valid_to`)
//! - **Entities**: people, projects, tools, concepts with typed relationships
//! - **Vectors**: embedding-indexed for semantic recall
//!
//! Uses `CozoDB` Datalog for graph traversal and HNSW for vector search.
//! Embedded, no sidecar. Replaced the former Mem0 stack (Qdrant + Neo4j + Ollama).

use crate::id::{EmbeddingId, EntityId, FactId};
use serde::{Deserialize, Serialize};

/// Maximum content length for facts and entities (100 KB).
pub const MAX_CONTENT_LENGTH: usize = 102_400;

/// A memory fact extracted from conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Unique identifier.
    pub id: FactId,
    /// Which nous extracted this fact.
    pub nous_id: String,
    /// The fact content.
    pub content: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f64,
    /// Epistemic tier: verified, inferred, or assumed.
    pub tier: EpistemicTier,
    /// When this fact became true.
    pub valid_from: jiff::Timestamp,
    /// When this fact stopped being true.
    pub valid_to: jiff::Timestamp,
    /// If superseded, the ID of the replacing fact.
    pub superseded_by: Option<FactId>,
    /// Session where this fact was extracted.
    pub source_session_id: Option<String>,
    /// When this fact was recorded in the system.
    pub recorded_at: jiff::Timestamp,
    /// Number of times this fact has been returned in recall/search results.
    pub access_count: u32,
    /// When this fact was last accessed.
    pub last_accessed_at: Option<jiff::Timestamp>,
    /// Initial stability for FSRS decay model (hours).
    pub stability_hours: f64,
    /// Fact classification for stability defaults.
    pub fact_type: String,
    /// Whether this fact has been intentionally excluded from recall.
    pub is_forgotten: bool,
    /// When the fact was forgotten.
    pub forgotten_at: Option<jiff::Timestamp>,
    /// Why the fact was forgotten.
    pub forget_reason: Option<ForgetReason>,
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
    /// Relationship weight/strength (0.0–1.0).
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
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Inferred => "inferred",
            Self::Assumed => "assumed",
        }
    }

    /// FSRS stability multiplier — verified facts decay 2× slower than inferred.
    #[must_use]
    pub fn stability_multiplier(self) -> f64 {
        match self {
            Self::Verified => 2.0,
            Self::Inferred => 1.0,
            Self::Assumed => 0.5,
        }
    }
}

impl std::fmt::Display for EpistemicTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
}

impl ForgetReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserRequested => "user_requested",
            Self::Outdated => "outdated",
            Self::Incorrect => "incorrect",
            Self::Privacy => "privacy",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
        }
    }
}

impl std::fmt::Display for ForgetReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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
    /// "My name is X" — very stable (2 years).
    Identity,
    /// "I prefer tabs" — stable (1 year).
    Preference,
    /// "I know Rust" — moderately stable (6 months).
    Skill,
    /// "X works at Y" — moderate (3 months).
    Relationship,
    /// "We discussed X" — short-lived (30 days).
    Event,
    /// "TODO: fix bug" — ephemeral (7 days).
    Task,
    /// "Build was slow" — very ephemeral (3 days).
    Observation,
}

impl FactType {
    /// Base stability in hours for FSRS power-law decay.
    #[must_use]
    pub fn base_stability_hours(self) -> f64 {
        match self {
            Self::Identity => 17_520.0,
            Self::Preference => 8_760.0,
            Self::Skill => 4_380.0,
            Self::Relationship => 2_190.0,
            Self::Event => 720.0,
            Self::Task => 168.0,
            Self::Observation => 72.0,
        }
    }

    /// Classify a fact by its text content using keyword heuristics.
    ///
    /// Falls back to [`FactType::Observation`] when no pattern matches.
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
            // NOTE: "observation" and any unknown value both fall back to Observation
            _ => Self::Observation,
        }
    }
}

impl std::fmt::Display for FactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
        .expect("valid far-future date")
        .timestamp()
}

/// Check whether a timestamp represents the "no end date" sentinel.
///
/// Returns `true` for any timestamp in year 9999, accommodating both the new
/// `9999-01-01` sentinel and legacy `9999-12-31` strings.
#[must_use]
pub fn is_far_future(ts: &jiff::Timestamp) -> bool {
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
    // NOTE: legacy far-future sentinel — jiff can't represent 9999-12-31 but can do 9999-01-01
    if s.starts_with("9999-") {
        return Some(far_future());
    }
    // NOTE: try full timestamp first, then fall back to date-only
    if let Ok(ts) = s.parse::<jiff::Timestamp>() {
        return Some(ts);
    }
    // NOTE: date-only strings are assumed to represent UTC midnight
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        return Some(
            date.to_zoned(jiff::tz::TimeZone::UTC)
                .expect("valid UTC conversion")
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

#[cfg(test)]
#[path = "knowledge_tests.rs"]
mod tests;
