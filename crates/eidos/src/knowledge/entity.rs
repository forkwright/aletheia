//! Entity and relationship types for the knowledge graph.

use serde::{Deserialize, Serialize};

use crate::id::{EmbeddingId, EntityId};

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
