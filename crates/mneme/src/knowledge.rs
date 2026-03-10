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
}

impl std::fmt::Display for EpistemicTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Reason for intentionally forgetting a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForgetReason {
    /// User explicitly requested removal.
    UserRequested,
    /// Fact is outdated.
    Outdated,
    /// Fact is incorrect.
    Incorrect,
    /// Privacy concern.
    Privacy,
}

impl ForgetReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserRequested => "user_requested",
            Self::Outdated => "outdated",
            Self::Incorrect => "incorrect",
            Self::Privacy => "privacy",
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
            other => Err(format!("unknown forget reason: {other}")),
        }
    }
}

/// Default FSRS stability by fact type (hours until 50% recall probability).
#[must_use]
pub fn default_stability_hours(fact_type: &str) -> f64 {
    match fact_type {
        "identity" => 17520.0,
        "preference" => 8760.0,
        "relationship" => 4380.0,
        "skill" => 2190.0,
        "task" => 168.0,
        _ => 720.0,
    }
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
pub fn parse_timestamp(s: &str) -> Option<jiff::Timestamp> {
    if s.is_empty() {
        return None;
    }
    // Legacy far-future sentinel — jiff can't represent 9999-12-31 but can do 9999-01-01
    if s.starts_with("9999-") {
        return Some(far_future());
    }
    // Try full timestamp first
    if let Ok(ts) = s.parse::<jiff::Timestamp>() {
        return Some(ts);
    }
    // Try date-only (assume UTC midnight)
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
mod tests {
    use super::*;
    use crate::id::FactId;

    fn test_timestamp(s: &str) -> jiff::Timestamp {
        parse_timestamp(s).expect("valid test timestamp")
    }

    // ---- EntityId ----

    #[test]
    fn entity_id_from_str() {
        let id = EntityId::from("alice");
        assert_eq!(id.as_str(), "alice");
        assert_eq!(id.to_string(), "alice");
    }

    #[test]
    fn entity_id_from_string() {
        let id = EntityId::from("bob".to_owned());
        assert_eq!(id.as_str(), "bob");
    }

    #[test]
    fn entity_id_serde_transparent() {
        let id = EntityId::from("e-123");
        let json = serde_json::to_string(&id).expect("EntityId serialization is infallible");
        // Must serialize as a plain JSON string, not {"0":"e-123"}
        assert_eq!(
            json, r#""e-123""#,
            "EntityId must serialize as plain string"
        );
        let back: EntityId =
            serde_json::from_str(&json).expect("EntityId should deserialize from its own JSON");
        assert_eq!(id, back);
    }

    #[test]
    fn entity_id_prevents_mixing_with_plain_string() {
        // Compile-time: EntityId and String are distinct types.
        // Runtime: confirm they compare correctly via as_str.
        let eid = EntityId::from("nous-1");
        let plain: String = "nous-1".to_owned();
        assert_eq!(eid.as_str(), plain.as_str());
    }

    #[test]
    fn entity_id_display_matches_inner_string() {
        let id = EntityId::from("project-aletheia");
        assert_eq!(format!("{id}"), "project-aletheia");
    }

    #[test]
    fn entity_id_clone_equality() {
        let a = EntityId::from("e-42");
        let b = a.clone();
        assert_eq!(a, b, "cloned EntityId must equal original");
        assert_eq!(a.as_str(), b.as_str());
    }

    #[test]
    fn epistemic_tier_serde_roundtrip() {
        for tier in [
            EpistemicTier::Verified,
            EpistemicTier::Inferred,
            EpistemicTier::Assumed,
        ] {
            let json =
                serde_json::to_string(&tier).expect("EpistemicTier serialization is infallible");
            let back: EpistemicTier = serde_json::from_str(&json)
                .expect("EpistemicTier should deserialize from its own JSON");
            assert_eq!(tier, back);
        }
    }

    #[test]
    fn fact_serde_roundtrip() {
        let fact = Fact {
            id: FactId::from("fact-1"),
            nous_id: "syn".to_owned(),
            content: "The researcher published findings on memory consolidation".to_owned(),
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            valid_from: test_timestamp("2026-02-01"),
            valid_to: far_future(),
            superseded_by: None,
            source_session_id: Some("ses-123".to_owned()),
            recorded_at: test_timestamp("2026-02-28T00:00:00Z"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
        let back: Fact =
            serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
        assert_eq!(fact.content, back.content);
        assert_eq!(fact.tier, back.tier);
    }

    #[test]
    fn entity_serde_roundtrip() {
        let entity = Entity {
            id: EntityId::from("e-1"),
            name: "Dr. Chen".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec!["acme_user".to_owned(), "test-user-01".to_owned()],
            created_at: test_timestamp("2026-01-28T00:00:00Z"),
            updated_at: test_timestamp("2026-02-28T00:00:00Z"),
        };
        let json = serde_json::to_string(&entity).expect("Entity serialization is infallible");
        let back: Entity =
            serde_json::from_str(&json).expect("Entity should deserialize from its own JSON");
        assert_eq!(entity.name, back.name);
        assert_eq!(entity.aliases, back.aliases);
    }

    #[test]
    fn relationship_serde_roundtrip() {
        let rel = Relationship {
            src: EntityId::from("e-1"),
            dst: EntityId::from("e-2"),
            relation: "works_on".to_owned(),
            weight: 0.85,
            created_at: test_timestamp("2026-02-28T00:00:00Z"),
        };
        let json = serde_json::to_string(&rel).expect("Relationship serialization is infallible");
        let back: Relationship =
            serde_json::from_str(&json).expect("Relationship should deserialize from its own JSON");
        assert_eq!(rel.src, back.src);
        assert_eq!(rel.dst, back.dst);
        assert_eq!(rel.relation, back.relation);
    }

    #[test]
    fn embedded_chunk_serde_roundtrip() {
        let chunk = EmbeddedChunk {
            id: EmbeddingId::from("emb-1"),
            content: "some text".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "fact-1".to_owned(),
            nous_id: "syn".to_owned(),
            embedding: vec![0.1, 0.2, 0.3],
            created_at: test_timestamp("2026-02-28T00:00:00Z"),
        };
        let json =
            serde_json::to_string(&chunk).expect("EmbeddedChunk serialization is infallible");
        let back: EmbeddedChunk = serde_json::from_str(&json)
            .expect("EmbeddedChunk should deserialize from its own JSON");
        assert_eq!(chunk.content, back.content);
        assert_eq!(chunk.embedding.len(), back.embedding.len());
    }

    #[test]
    fn recall_result_serde_roundtrip() {
        let result = RecallResult {
            content: "The researcher published findings on memory consolidation".to_owned(),
            distance: 0.12,
            source_type: "fact".to_owned(),
            source_id: "fact-1".to_owned(),
        };
        let json =
            serde_json::to_string(&result).expect("RecallResult serialization is infallible");
        let back: RecallResult =
            serde_json::from_str(&json).expect("RecallResult should deserialize from its own JSON");
        assert_eq!(result.content, back.content);
        assert!((result.distance - back.distance).abs() < f64::EPSILON);
    }

    #[test]
    fn fact_with_empty_content() {
        let fact = Fact {
            id: FactId::from("f-empty"),
            nous_id: "syn".to_owned(),
            content: String::new(),
            confidence: 0.5,
            tier: EpistemicTier::Assumed,
            valid_from: test_timestamp("2026-01-01"),
            valid_to: far_future(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        let json =
            serde_json::to_string(&fact).expect("Fact with empty content serializes successfully");
        let back: Fact = serde_json::from_str(&json)
            .expect("Fact with empty content should deserialize from its own JSON");
        assert!(back.content.is_empty());
    }

    #[test]
    fn fact_with_unicode_content() {
        let fact = Fact {
            id: FactId::from("f-uni"),
            nous_id: "syn".to_owned(),
            content: "The user writes \u{65E5}\u{672C}\u{8A9E} and emoji \u{1F980}".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Verified,
            valid_from: test_timestamp("2026-01-01"),
            valid_to: far_future(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        let json = serde_json::to_string(&fact)
            .expect("Fact with unicode content serializes successfully");
        let back: Fact = serde_json::from_str(&json)
            .expect("Fact with unicode content should deserialize from its own JSON");
        assert_eq!(fact.content, back.content);
    }

    #[test]
    fn entity_empty_aliases() {
        let entity = Entity {
            id: EntityId::from("e-2"),
            name: "Aletheia".to_owned(),
            entity_type: "project".to_owned(),
            aliases: vec![],
            created_at: test_timestamp("2026-01-01T00:00:00Z"),
            updated_at: test_timestamp("2026-01-01T00:00:00Z"),
        };
        let json = serde_json::to_string(&entity)
            .expect("Entity with empty aliases serializes successfully");
        let back: Entity = serde_json::from_str(&json)
            .expect("Entity with empty aliases should deserialize from its own JSON");
        assert!(back.aliases.is_empty());
    }

    #[test]
    fn epistemic_tier_display() {
        assert_eq!(EpistemicTier::Verified.to_string(), "verified");
        assert_eq!(EpistemicTier::Inferred.to_string(), "inferred");
        assert_eq!(EpistemicTier::Assumed.to_string(), "assumed");
    }

    #[test]
    fn default_stability_by_fact_type() {
        assert!((default_stability_hours("identity") - 17520.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("preference") - 8760.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("relationship") - 4380.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("skill") - 2190.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("event") - 720.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("task") - 168.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("inference") - 720.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("unknown") - 720.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("") - 720.0).abs() < f64::EPSILON);
    }

    #[test]
    fn epistemic_tier_as_str_matches_serde() {
        for tier in [
            EpistemicTier::Verified,
            EpistemicTier::Inferred,
            EpistemicTier::Assumed,
        ] {
            let json =
                serde_json::to_string(&tier).expect("EpistemicTier serialization is infallible");
            let expected = format!("\"{}\"", tier.as_str());
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn forget_reason_serde_roundtrip() {
        for reason in [
            ForgetReason::UserRequested,
            ForgetReason::Outdated,
            ForgetReason::Incorrect,
            ForgetReason::Privacy,
        ] {
            let json =
                serde_json::to_string(&reason).expect("ForgetReason serialization is infallible");
            let back: ForgetReason = serde_json::from_str(&json)
                .expect("ForgetReason should deserialize from its own JSON");
            assert_eq!(reason, back);
        }
    }

    #[test]
    fn forget_reason_as_str_matches_serde() {
        for reason in [
            ForgetReason::UserRequested,
            ForgetReason::Outdated,
            ForgetReason::Incorrect,
            ForgetReason::Privacy,
        ] {
            let json =
                serde_json::to_string(&reason).expect("ForgetReason serialization is infallible");
            let expected = format!("\"{}\"", reason.as_str());
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn forget_reason_from_str_roundtrip() {
        for reason in [
            ForgetReason::UserRequested,
            ForgetReason::Outdated,
            ForgetReason::Incorrect,
            ForgetReason::Privacy,
        ] {
            let parsed: ForgetReason = reason
                .as_str()
                .parse()
                .expect("ForgetReason as_str() should round-trip through FromStr");
            assert_eq!(reason, parsed);
        }
    }

    #[test]
    fn forget_reason_from_str_unknown() {
        assert!("bogus".parse::<ForgetReason>().is_err());
    }

    #[test]
    fn forget_reason_display() {
        assert_eq!(ForgetReason::UserRequested.to_string(), "user_requested");
        assert_eq!(ForgetReason::Outdated.to_string(), "outdated");
        assert_eq!(ForgetReason::Incorrect.to_string(), "incorrect");
        assert_eq!(ForgetReason::Privacy.to_string(), "privacy");
    }

    #[test]
    fn epistemic_tier_display_roundtrip() {
        for tier in [
            EpistemicTier::Verified,
            EpistemicTier::Inferred,
            EpistemicTier::Assumed,
        ] {
            let s = tier.as_str();
            let json_str = format!("\"{s}\"");
            let parsed: EpistemicTier = serde_json::from_str(&json_str)
                .expect("EpistemicTier should deserialize from its as_str() representation");
            assert_eq!(tier, parsed, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn fact_default_stability_hours_known_types() {
        assert!((default_stability_hours("identity") - 17520.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("preference") - 8760.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("relationship") - 4380.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("skill") - 2190.0).abs() < f64::EPSILON);
        assert!((default_stability_hours("task") - 168.0).abs() < f64::EPSILON);
        assert!(
            (default_stability_hours("completely_unknown_type") - 720.0).abs() < f64::EPSILON,
            "fallback for unknown fact types should be 720 hours"
        );
    }

    #[test]
    fn forget_reason_all_variants_as_str() {
        let all = [
            ForgetReason::UserRequested,
            ForgetReason::Outdated,
            ForgetReason::Incorrect,
            ForgetReason::Privacy,
        ];
        for reason in all {
            let s = reason.as_str();
            assert!(!s.is_empty(), "as_str() must be non-empty for {reason:?}");
        }
    }

    #[test]
    fn fact_diff_empty() {
        let diff = FactDiff {
            added: vec![],
            modified: vec![],
            removed: vec![],
        };
        assert!(diff.added.is_empty());
        assert!(diff.modified.is_empty());
        assert!(diff.removed.is_empty());
        let json = serde_json::to_string(&diff).expect("FactDiff serialization is infallible");
        let back: FactDiff =
            serde_json::from_str(&json).expect("FactDiff should deserialize from its own JSON");
        assert!(back.added.is_empty());
        assert!(back.modified.is_empty());
        assert!(back.removed.is_empty());
    }

    #[test]
    fn embedded_chunk_fields() {
        let chunk = EmbeddedChunk {
            id: EmbeddingId::from("emb-42"),
            content: "test content".to_owned(),
            source_type: "note".to_owned(),
            source_id: "note-7".to_owned(),
            nous_id: "syn".to_owned(),
            embedding: vec![1.0, 2.0, 3.0, 4.0],
            created_at: test_timestamp("2026-03-01T00:00:00Z"),
        };
        assert_eq!(chunk.id.as_str(), "emb-42");
        assert_eq!(chunk.content, "test content");
        assert_eq!(chunk.source_type, "note");
        assert_eq!(chunk.source_id, "note-7");
        assert_eq!(chunk.nous_id, "syn");
        assert_eq!(chunk.embedding.len(), 4);
    }

    #[test]
    fn epistemic_tier_ordering() {
        let verified_score = match EpistemicTier::Verified {
            EpistemicTier::Verified => 3,
            EpistemicTier::Inferred => 2,
            EpistemicTier::Assumed => 1,
        };
        let inferred_score = match EpistemicTier::Inferred {
            EpistemicTier::Verified => 3,
            EpistemicTier::Inferred => 2,
            EpistemicTier::Assumed => 1,
        };
        let assumed_score = match EpistemicTier::Assumed {
            EpistemicTier::Verified => 3,
            EpistemicTier::Inferred => 2,
            EpistemicTier::Assumed => 1,
        };
        assert!(
            verified_score > inferred_score,
            "Verified must rank higher than Inferred"
        );
        assert!(
            inferred_score > assumed_score,
            "Inferred must rank higher than Assumed"
        );
    }

    #[test]
    fn fact_with_supersession() {
        let fact = Fact {
            id: FactId::from("f-old"),
            nous_id: "syn".to_owned(),
            content: "outdated claim".to_owned(),
            confidence: 0.7,
            tier: EpistemicTier::Inferred,
            valid_from: test_timestamp("2026-01-01"),
            valid_to: test_timestamp("2026-02-01"),
            superseded_by: Some(FactId::from("f-new")),
            source_session_id: None,
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        assert_eq!(
            fact.superseded_by.as_ref().map(FactId::as_str),
            Some("f-new")
        );
        let json = serde_json::to_string(&fact)
            .expect("Fact with superseded_by field serializes successfully");
        let back: Fact = serde_json::from_str(&json)
            .expect("Fact with superseded_by should deserialize from its own JSON");
        assert_eq!(
            back.superseded_by.as_ref().map(FactId::as_str),
            Some("f-new")
        );
    }

    #[test]
    fn fact_with_session_source() {
        let fact = Fact {
            id: FactId::from("f-src"),
            nous_id: "syn".to_owned(),
            content: "extracted from conversation".to_owned(),
            confidence: 0.85,
            tier: EpistemicTier::Verified,
            valid_from: test_timestamp("2026-03-01"),
            valid_to: far_future(),
            superseded_by: None,
            source_session_id: Some("ses-abc-123".to_owned()),
            recorded_at: test_timestamp("2026-03-01T00:00:00Z"),
            access_count: 3,
            last_accessed_at: Some(test_timestamp("2026-03-05T12:00:00Z")),
            stability_hours: 4380.0,
            fact_type: "relationship".to_owned(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        assert_eq!(fact.source_session_id.as_deref(), Some("ses-abc-123"));
        let json = serde_json::to_string(&fact)
            .expect("Fact with source_session_id serializes successfully");
        let back: Fact = serde_json::from_str(&json)
            .expect("Fact with source_session_id should deserialize from its own JSON");
        assert_eq!(back.source_session_id.as_deref(), Some("ses-abc-123"));
    }

    #[test]
    fn parse_timestamp_full() {
        let ts = parse_timestamp("2026-03-01T12:30:00Z");
        assert!(ts.is_some());
    }

    #[test]
    fn parse_timestamp_date_only() {
        let ts = parse_timestamp("2026-03-01");
        assert!(ts.is_some());
    }

    #[test]
    fn parse_timestamp_empty() {
        assert!(parse_timestamp("").is_none());
    }

    #[test]
    fn parse_timestamp_invalid() {
        assert!(parse_timestamp("not-a-date").is_none());
    }

    #[test]
    fn format_timestamp_roundtrip() {
        let ts =
            parse_timestamp("2026-03-01T12:30:00Z").expect("valid ISO 8601 timestamp should parse");
        let s = format_timestamp(&ts);
        assert_eq!(s, "2026-03-01T12:30:00Z");
        let back = parse_timestamp(&s).expect("formatted timestamp should parse back");
        assert_eq!(ts, back);
    }

    #[test]
    fn far_future_is_year_9999() {
        let ts = far_future();
        let s = format_timestamp(&ts);
        assert!(s.starts_with("9999-01-01"));
        assert!(is_far_future(&ts));
    }
}
