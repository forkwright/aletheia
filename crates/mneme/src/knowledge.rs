//! Knowledge store — facts, entities, and vectors via `CozoDB`.
//!
//! Complements the `SQLite` session store with structured knowledge:
//! - **Facts**: extracted from conversations, bi-temporal (`valid_from`/`valid_to`)
//! - **Entities**: people, projects, tools, concepts with typed relationships
//! - **Vectors**: embedding-indexed for semantic recall
//!
//! Uses `CozoDB` Datalog for graph traversal and HNSW for vector search.
//! Embedded, no sidecar. Replaced the former Mem0 stack (Qdrant + Neo4j + Ollama).

use serde::{Deserialize, Serialize};

/// A memory fact extracted from conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Unique identifier.
    pub id: String,
    /// Which nous extracted this fact.
    pub nous_id: String,
    /// The fact content.
    pub content: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f64,
    /// Epistemic tier: verified, inferred, or assumed.
    pub tier: EpistemicTier,
    /// When this fact became true (ISO 8601).
    pub valid_from: String,
    /// When this fact stopped being true (ISO 8601, "9999-12-31" = current).
    pub valid_to: String,
    /// If superseded, the ID of the replacing fact.
    pub superseded_by: Option<String>,
    /// Session where this fact was extracted.
    pub source_session_id: Option<String>,
    /// When this fact was recorded in the system.
    pub recorded_at: String,
    /// Number of times this fact has been returned in recall/search results.
    pub access_count: u32,
    /// When this fact was last accessed (ISO 8601, empty = never).
    pub last_accessed_at: String,
    /// Initial stability for FSRS decay model (hours).
    pub stability_hours: f64,
    /// Fact classification for stability defaults.
    pub fact_type: String,
    /// Whether this fact has been intentionally excluded from recall.
    pub is_forgotten: bool,
    /// When the fact was forgotten (ISO 8601).
    pub forgotten_at: Option<String>,
    /// Why the fact was forgotten.
    pub forget_reason: Option<ForgetReason>,
}

/// An entity in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Entity type (person, project, tool, concept, etc.).
    pub entity_type: String,
    /// Known aliases.
    pub aliases: Vec<String>,
    /// When first observed.
    pub created_at: String,
    /// When last updated.
    pub updated_at: String,
}

/// A relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Source entity ID.
    pub src: String,
    /// Target entity ID.
    pub dst: String,
    /// Relationship type (e.g. `works_on`, `knows`, `depends_on`).
    pub relation: String,
    /// Relationship weight/strength (0.0–1.0).
    pub weight: f64,
    /// When first observed.
    pub created_at: String,
}

/// A vector embedding for semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedChunk {
    /// Unique identifier.
    pub id: String,
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
    pub created_at: String,
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

    #[test]
    fn epistemic_tier_serde_roundtrip() {
        for tier in [
            EpistemicTier::Verified,
            EpistemicTier::Inferred,
            EpistemicTier::Assumed,
        ] {
            let json = serde_json::to_string(&tier).unwrap();
            let back: EpistemicTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, back);
        }
    }

    #[test]
    fn fact_serde_roundtrip() {
        let fact = Fact {
            id: "fact-1".to_owned(),
            nous_id: "syn".to_owned(),
            content: "The researcher published findings on memory consolidation".to_owned(),
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            valid_from: "2026-02-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: Some("ses-123".to_owned()),
            recorded_at: "2026-02-28T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        let json = serde_json::to_string(&fact).unwrap();
        let back: Fact = serde_json::from_str(&json).unwrap();
        assert_eq!(fact.content, back.content);
        assert_eq!(fact.tier, back.tier);
    }

    #[test]
    fn entity_serde_roundtrip() {
        let entity = Entity {
            id: "e-1".to_owned(),
            name: "Dr. Chen".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec!["acme_user".to_owned(), "test-user-01".to_owned()],
            created_at: "2026-01-28T00:00:00Z".to_owned(),
            updated_at: "2026-02-28T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&entity).unwrap();
        let back: Entity = serde_json::from_str(&json).unwrap();
        assert_eq!(entity.name, back.name);
        assert_eq!(entity.aliases, back.aliases);
    }

    #[test]
    fn relationship_serde_roundtrip() {
        let rel = Relationship {
            src: "e-1".to_owned(),
            dst: "e-2".to_owned(),
            relation: "works_on".to_owned(),
            weight: 0.85,
            created_at: "2026-02-28T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&rel).unwrap();
        let back: Relationship = serde_json::from_str(&json).unwrap();
        assert_eq!(rel.src, back.src);
        assert_eq!(rel.dst, back.dst);
        assert_eq!(rel.relation, back.relation);
    }

    #[test]
    fn embedded_chunk_serde_roundtrip() {
        let chunk = EmbeddedChunk {
            id: "emb-1".to_owned(),
            content: "some text".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "fact-1".to_owned(),
            nous_id: "syn".to_owned(),
            embedding: vec![0.1, 0.2, 0.3],
            created_at: "2026-02-28T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: EmbeddedChunk = serde_json::from_str(&json).unwrap();
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
        let json = serde_json::to_string(&result).unwrap();
        let back: RecallResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.content, back.content);
        assert!((result.distance - back.distance).abs() < f64::EPSILON);
    }

    #[test]
    fn fact_with_empty_content() {
        let fact = Fact {
            id: "f-empty".to_owned(),
            nous_id: "syn".to_owned(),
            content: String::new(),
            confidence: 0.5,
            tier: EpistemicTier::Assumed,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-01-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        let json = serde_json::to_string(&fact).unwrap();
        let back: Fact = serde_json::from_str(&json).unwrap();
        assert!(back.content.is_empty());
    }

    #[test]
    fn fact_with_unicode_content() {
        let fact = Fact {
            id: "f-uni".to_owned(),
            nous_id: "syn".to_owned(),
            content: "The user writes 日本語 and emoji 🦀".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Verified,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-01-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        let json = serde_json::to_string(&fact).unwrap();
        let back: Fact = serde_json::from_str(&json).unwrap();
        assert_eq!(fact.content, back.content);
    }

    #[test]
    fn entity_empty_aliases() {
        let entity = Entity {
            id: "e-2".to_owned(),
            name: "Aletheia".to_owned(),
            entity_type: "project".to_owned(),
            aliases: vec![],
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&entity).unwrap();
        let back: Entity = serde_json::from_str(&json).unwrap();
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
            let json = serde_json::to_string(&tier).unwrap();
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
            let json = serde_json::to_string(&reason).unwrap();
            let back: ForgetReason = serde_json::from_str(&json).unwrap();
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
            let json = serde_json::to_string(&reason).unwrap();
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
            let parsed: ForgetReason = reason.as_str().parse().unwrap();
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
}
