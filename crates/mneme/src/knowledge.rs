//! Knowledge store — facts, entities, and vectors via `CozoDB`.
//!
//! Complements the `SQLite` session store with structured knowledge:
//! - **Facts**: extracted from conversations, bi-temporal (`valid_from`/`valid_to`)
//! - **Entities**: people, projects, tools, concepts with typed relationships
//! - **Vectors**: embedding-indexed for semantic recall
//!
//! Uses `CozoDB` Datalog for graph traversal and HNSW for vector search.
//! Embedded, no sidecar — replaces the `Mem0` (`Qdrant` + `Neo4j` + `Ollama`) stack.

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
            content: "Cody lives in Pflugerville".to_owned(),
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            valid_from: "2026-02-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: Some("ses-123".to_owned()),
            recorded_at: "2026-02-28T00:00:00Z".to_owned(),
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
            name: "Cody".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec!["CKickertz".to_owned(), "forkwright".to_owned()],
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
            content: "Cody lives in Pflugerville".to_owned(),
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
            content: "Cody uses 日本語 and emoji 🦀".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Verified,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-01-01T00:00:00Z".to_owned(),
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
}
