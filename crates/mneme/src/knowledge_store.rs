//! `CozoDB`-backed knowledge store implementation.
//!
//! This module is gated behind the `cozo` feature flag due to `sqlite3` link
//! conflict with `rusqlite`. In the final binary, the session store will migrate
//! from `rusqlite` to `CozoDB`'s embedded `SQLite` storage, resolving the conflict.
//!
//! Until then, this code compiles and tests only with:
//! ```sh
//! cargo test -p aletheia-mneme --no-default-features --features cozo
//! ```
//!
//! # Schema
//!
//! ## Relations (Datalog)
//!
//! ```text
//! facts { id: String, valid_from: String => content: String, nous_id: String,
//!         confidence: Float, tier: String, valid_to: String, superseded_by: String?,
//!         source_session_id: String?, recorded_at: String }
//!
//! entities { id: String => name: String, entity_type: String, aliases: String,
//!            created_at: String, updated_at: String }
//!
//! relationships { src: String, dst: String => relation: String, weight: Float,
//!                 created_at: String }
//!
//! embeddings { id: String => content: String, source_type: String, source_id: String,
//!              nous_id: String, embedding: <F32; DIM>, created_at: String }
//! ```
//!
//! ## HNSW Index
//!
//! ```text
//! ::hnsw create embeddings:semantic_idx {
//!     dim: DIM, m: 16, ef_construction: 200,
//!     dtype: F32, distance: Cosine, fields: [embedding]
//! }
//! ```

// This module contains the CozoDB store implementation as documentation and
// reference code. It will be activated when the cozo feature flag is enabled
// in the production binary.
//
// The Datalog queries are validated by the mneme-bench crate.

/// Datalog DDL for initializing the knowledge schema.
pub const KNOWLEDGE_DDL: &[&str] = &[
    // Facts: bi-temporal, epistemic-tiered
    r":create facts {
        id: String, valid_from: String =>
        content: String,
        nous_id: String,
        confidence: Float,
        tier: String,
        valid_to: String,
        superseded_by: String?,
        source_session_id: String?,
        recorded_at: String
    }",
    // Entities: typed nodes in the knowledge graph
    r":create entities {
        id: String =>
        name: String,
        entity_type: String,
        aliases: String,
        created_at: String,
        updated_at: String
    }",
    // Relationships: weighted edges
    r":create relationships {
        src: String, dst: String =>
        relation: String,
        weight: Float,
        created_at: String
    }",
];

/// Datalog DDL for the embeddings relation. Dimension is parameterized.
pub fn embeddings_ddl(dim: usize) -> String {
    format!(
        r":create embeddings {{
            id: String =>
            content: String,
            source_type: String,
            source_id: String,
            nous_id: String,
            embedding: <F32; {dim}>,
            created_at: String
        }}"
    )
}

/// Datalog DDL for the HNSW index on embeddings.
pub fn hnsw_ddl(dim: usize) -> String {
    format!(
        r"::hnsw create embeddings:semantic_idx {{
            dim: {dim},
            m: 16,
            ef_construction: 200,
            dtype: F32,
            distance: Cosine,
            fields: [embedding]
        }}"
    )
}

/// Query templates for common knowledge operations.
pub mod queries {
    /// Insert or update a fact.
    pub const UPSERT_FACT: &str = r"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
          superseded_by, source_session_id, recorded_at] <- [[$id, $valid_from,
          $content, $nous_id, $confidence, $tier, $valid_to, $superseded_by,
          $source_session_id, $recorded_at]]
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at}
    ";

    /// Query current facts for a nous (not superseded, currently valid).
    pub const CURRENT_FACTS: &str = r"
        ?[id, content, confidence, tier, recorded_at] :=
            *facts{id, valid_from, content, nous_id, confidence, tier,
                   valid_to, superseded_by, recorded_at},
            nous_id = $nous_id,
            valid_from <= $now,
            valid_to > $now,
            is_null(superseded_by)
        :order -confidence
        :limit $limit
    ";

    /// Point-in-time fact query.
    pub const FACTS_AT_TIME: &str = r"
        ?[id, content, confidence, tier] :=
            *facts{id, valid_from, content, confidence, tier, valid_to},
            valid_from <= $time,
            valid_to > $time
    ";

    /// Supersede a fact (close old, insert new).
    #[allow(clippy::needless_raw_string_hashes)]  // contains inner quotes
    pub const SUPERSEDE_FACT: &str = r#"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
          superseded_by, source_session_id, recorded_at] <- [
            [$old_id, $old_valid_from, $old_content, $nous_id, $old_confidence,
             $old_tier, $now, $new_id, $old_source, $old_recorded],
            [$new_id, $now, $new_content, $nous_id, $new_confidence,
             $new_tier, "9999-12-31", null, $source_session_id, $now]
        ]
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at}
    "#;

    /// Insert or update an entity.
    pub const UPSERT_ENTITY: &str = r"
        ?[id, name, entity_type, aliases, created_at, updated_at] <- [
            [$id, $name, $entity_type, $aliases, $created_at, $updated_at]
        ]
        :put entities {id => name, entity_type, aliases, created_at, updated_at}
    ";

    /// Insert a relationship.
    pub const UPSERT_RELATIONSHIP: &str = r"
        ?[src, dst, relation, weight, created_at] <- [
            [$src, $dst, $relation, $weight, $created_at]
        ]
        :put relationships {src, dst => relation, weight, created_at}
    ";

    /// 2-hop entity neighborhood.
    pub const ENTITY_NEIGHBORHOOD: &str = r"
        hop1[dst, rel] := *relationships{src: $entity_id, dst, relation: rel}
        hop2[dst, rel] := hop1[mid, _], *relationships{src: mid, dst, relation: rel}
        ?[id, name, entity_type, relation, hop] :=
            hop1[id, relation], *entities{id, name, entity_type}, hop = 1
        ?[id, name, entity_type, relation, hop] :=
            hop2[id, relation], *entities{id, name, entity_type}, hop = 2
        :order hop, name
    ";

    /// KNN vector search.
    pub const SEMANTIC_SEARCH: &str = r"
        ?[id, content, source_type, source_id, dist] :=
            ~embeddings:semantic_idx {id, content, source_type, source_id |
                query: $query_vec, k: $k, ef: $ef, bind_distance: dist}
    ";

    /// Entity search by name or alias (prefix match).
    pub const SEARCH_ENTITIES: &str = r"
        ?[id, name, entity_type] :=
            *entities{id, name, entity_type},
            starts_with(name, $prefix)
        ?[id, name, entity_type] :=
            *entities{id, name, entity_type, aliases},
            contains(aliases, $prefix)
        :limit $limit
    ";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_templates_are_valid_strings() {
        // Verify DDL templates don't panic on formatting
        assert!(KNOWLEDGE_DDL.len() == 3);
        let emb = embeddings_ddl(1024);
        assert!(emb.contains("1024"));
        let idx = hnsw_ddl(1024);
        assert!(idx.contains("1024"));
    }

    #[test]
    fn query_templates_contain_params() {
        assert!(queries::CURRENT_FACTS.contains("$nous_id"));
        assert!(queries::CURRENT_FACTS.contains("$now"));
        assert!(queries::SEMANTIC_SEARCH.contains("$query_vec"));
        assert!(queries::ENTITY_NEIGHBORHOOD.contains("$entity_id"));
        assert!(queries::SUPERSEDE_FACT.contains("$old_id"));
        assert!(queries::SUPERSEDE_FACT.contains("$new_id"));
    }
}
