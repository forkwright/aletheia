//! Builder-generated query scripts for `KnowledgeStore` operations.

// WHY: `#[expect]` cannot be used here; this module is only compiled with the mneme-engine
// feature, so the expectation would be unfulfilled in default-feature compilations.
#![allow(
    clippy::enum_glob_use,
    clippy::wildcard_imports,
    reason = "query builders use glob imports for enum field variants"
)]

use super::*;

/// Insert or update a fact. Params: `$id`, `$valid_from`, `$content`,
/// `$nous_id`, `$confidence`, `$tier`, `$valid_to`, `$superseded_by`,
/// `$source_session_id`, `$recorded_at`.
#[must_use]
pub fn upsert_fact() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .put(Relation::Facts)
        .keys(&[Id, ValidFrom])
        .values(&[
            Content,
            NousId,
            Confidence,
            Tier,
            ValidTo,
            SupersededBy,
            SourceSessionId,
            RecordedAt,
            AccessCount,
            LastAccessedAt,
            StabilityHours,
            FactType,
            IsForgotten,
            ForgottenAt,
            ForgetReason,
        ])
        .done()
        .build_script()
}

/// Query current facts for a nous (not superseded, currently valid).
/// Params: `$nous_id`, `$now`, `$limit`.
#[must_use]
pub fn current_facts() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content, Confidence, Tier, RecordedAt])
        .bind(Id)
        .bind(ValidFrom)
        .bind(Content)
        .bind(NousId)
        .bind(Confidence)
        .bind(Tier)
        .bind(ValidTo)
        .bind(SupersededBy)
        .bind(RecordedAt)
        .bind(AccessCount)
        .bind(LastAccessedAt)
        .bind(StabilityHours)
        .bind(FactType)
        .bind(IsForgotten)
        .bind(ForgottenAt)
        .bind(ForgetReason)
        .filter("nous_id = $nous_id")
        .filter("valid_from <= $now")
        .filter("valid_to > $now")
        .filter("is_null(superseded_by)")
        .filter("is_forgotten == false")
        .order("-confidence")
        .limit("$limit")
        .done()
        .build_script()
}

/// Extended query returning all `Fact` fields.
/// Params: `$nous_id`, `$now`, `$limit`.
#[must_use]
pub fn full_current_facts() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[
            Id,
            Content,
            Confidence,
            Tier,
            RecordedAt,
            NousId,
            ValidFrom,
            ValidTo,
            SupersededBy,
            SourceSessionId,
            AccessCount,
            LastAccessedAt,
            StabilityHours,
            FactType,
            IsForgotten,
            ForgottenAt,
            ForgetReason,
        ])
        .bind(Id)
        .bind(ValidFrom)
        .bind(Content)
        .bind(NousId)
        .bind(Confidence)
        .bind(Tier)
        .bind(ValidTo)
        .bind(SupersededBy)
        .bind(SourceSessionId)
        .bind(RecordedAt)
        .bind(AccessCount)
        .bind(LastAccessedAt)
        .bind(StabilityHours)
        .bind(FactType)
        .bind(IsForgotten)
        .bind(ForgottenAt)
        .bind(ForgetReason)
        .filter("nous_id = $nous_id")
        .filter("valid_from <= $now")
        .filter("valid_to > $now")
        .filter("is_null(superseded_by)")
        .filter("is_forgotten == false")
        .order("-confidence")
        .limit("$limit")
        .done()
        .build_script()
}

/// Point-in-time fact query. Params: `$time`.
#[must_use]
pub fn facts_at_time() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content, Confidence, Tier])
        .bind(Id)
        .bind(ValidFrom)
        .bind(Content)
        .bind(Confidence)
        .bind(Tier)
        .bind(ValidTo)
        .bind(IsForgotten)
        .filter("valid_from <= $time")
        .filter("valid_to > $time")
        .filter("is_forgotten == false")
        .done()
        .build_script()
}

/// Supersede a fact (close old, insert new). Two rows in one `:put`.
/// Params: `$old_id`, `$old_valid_from`, `$old_content`, `$nous_id`,
/// `$old_confidence`, `$old_tier`, `$now`, `$new_id`, `$old_source`,
/// `$old_recorded`, `$new_content`, `$new_confidence`, `$new_tier`,
/// `$source_session_id`.
#[must_use]
pub fn supersede_fact() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .put(Relation::Facts)
        .keys(&[Id, ValidFrom])
        .values(&[
            Content,
            NousId,
            Confidence,
            Tier,
            ValidTo,
            SupersededBy,
            SourceSessionId,
            RecordedAt,
            AccessCount,
            LastAccessedAt,
            StabilityHours,
            FactType,
            IsForgotten,
            ForgottenAt,
            ForgetReason,
        ])
        .row(&[
            "$old_id",
            "$old_valid_from",
            "$old_content",
            "$nous_id",
            "$old_confidence",
            "$old_tier",
            "$now",
            "$new_id",
            "$old_source",
            "$old_recorded",
            "$old_access_count",
            "$old_last_accessed_at",
            "$old_stability_hours",
            "$old_fact_type",
            "$old_is_forgotten",
            "$old_forgotten_at",
            "$old_forget_reason",
        ])
        .row(&[
            "$new_id",
            "$now",
            "$new_content",
            "$nous_id",
            "$new_confidence",
            "$new_tier",
            "\"9999-12-31\"",
            "null",
            "$source_session_id",
            "$now",
            "0",
            "\"\"",
            "$stability_hours",
            "$fact_type",
            "false",
            "null",
            "null",
        ])
        .done()
        .build_script()
}

/// Insert or update an entity.
/// Params: `$id`, `$name`, `$entity_type`, `$aliases`, `$created_at`, `$updated_at`.
#[must_use]
pub fn upsert_entity() -> String {
    use EntitiesField::*;
    QueryBuilder::new()
        .put(Relation::Entities)
        .keys(&[Id])
        .values(&[Name, EntityType, Aliases, CreatedAt, UpdatedAt])
        .done()
        .build_script()
}

/// Insert a relationship.
/// Params: `$src`, `$dst`, `$relation`, `$weight`, `$created_at`.
#[must_use]
pub fn upsert_relationship() -> String {
    use RelationshipsField::{CreatedAt, Dst, Relation as Rel, Src, Weight};
    QueryBuilder::new()
        .put(super::Relation::Relationships)
        .keys(&[Src, Dst])
        .values(&[Rel, Weight, CreatedAt])
        .done()
        .build_script()
}

/// Insert an embedding chunk.
/// Params: `$id`, `$content`, `$source_type`, `$source_id`, `$nous_id`,
/// `$embedding`, `$created_at`.
#[must_use]
pub fn upsert_embedding() -> String {
    use EmbeddingsField::*;
    QueryBuilder::new()
        .put(Relation::Embeddings)
        .keys(&[Id])
        .values(&[Content, SourceType, SourceId, NousId, Embedding, CreatedAt])
        .done()
        .build_script()
}

/// 2-hop entity neighborhood. Params: `$entity_id`.
pub const ENTITY_NEIGHBORHOOD: &str = r"
    hop1[dst, rel] := *relationships{src: $entity_id, dst, relation: rel}
    hop2[dst, rel] := hop1[mid, _], *relationships{src: mid, dst, relation: rel}
    ?[id, name, entity_type, relation, hop] :=
        hop1[id, relation], *entities{id, name, entity_type}, hop = 1
    ?[id, name, entity_type, relation, hop] :=
        hop2[id, relation], *entities{id, name, entity_type}, hop = 2
    :order hop, name
";

/// BM25 full-text recall (no vector embeddings required).
/// Returns rows in the same format as `SEMANTIC_SEARCH` (id, content, `source_type`, `source_id`, dist)
/// with synthetic distance derived from BM25 score.
/// Params: `$query_text`, `$k`.
pub const BM25_RECALL: &str = r"
    bm25[id, score] := ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}

    ?[id, content, source_type, source_id, dist] :=
        bm25[id, bm25_score],
        *facts{id, content, is_forgotten, superseded_by},
        is_forgotten == false,
        is_null(superseded_by),
        source_type = 'fact',
        source_id = id,
        dist = 1.0 / bm25_score
    :order dist
    :limit $k
";

/// KNN vector search. Params: `$query_vec`, `$k`, `$ef`.
pub const SEMANTIC_SEARCH: &str = r"
    ?[id, content, source_type, source_id, dist] :=
        ~embeddings:semantic_idx {id, content, source_type, source_id |
            query: $query_vec, k: $k, ef: $ef, bind_distance: dist}
";

/// Entity search by name or alias (prefix match). Params: `$prefix`, `$limit`.
pub const SEARCH_ENTITIES: &str = r"
    ?[id, name, entity_type] :=
        *entities{id, name, entity_type},
        starts_with(name, $prefix)
    ?[id, name, entity_type] :=
        *entities{id, name, entity_type, aliases},
        contains(aliases, $prefix)
    :limit $limit
";

/// Hybrid search: BM25 + HNSW vector + graph neighborhood fused via RRF.
/// Graph sub-rules are injected dynamically by `build_hybrid_query`.
/// Params: `$query_text`, `$query_vec`, `$k`, `$ef`, `$limit`.
pub const HYBRID_SEARCH_BASE: &str = r"
    bm25[id, score] := ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}

    vec[id, score] :=
        ~embeddings:semantic_idx{id | query: $query_vec, k: $k, ef: $ef, bind_distance: raw_dist},
        score = 1.0 - raw_dist

    {GRAPH_RULES}

    ?[id, rrf_score, bm25_rank, vec_rank, graph_rank] <~
        ReciprocalRankFusion(bm25[], vec[], graph[])

    :order -rrf_score
    :limit $limit
";

/// Bi-temporal point-in-time query with all fields. Params: `$nous_id`, `$at_time`.
/// Returns facts where `valid_from <= at_time` AND `valid_to > at_time` AND not forgotten.
#[must_use]
pub fn temporal_facts() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[
            Id,
            Content,
            Confidence,
            Tier,
            RecordedAt,
            NousId,
            ValidFrom,
            ValidTo,
            SupersededBy,
            SourceSessionId,
            AccessCount,
            LastAccessedAt,
            StabilityHours,
            FactType,
            IsForgotten,
            ForgottenAt,
            ForgetReason,
        ])
        .bind(Id)
        .bind(ValidFrom)
        .bind(Content)
        .bind(NousId)
        .bind(Confidence)
        .bind(Tier)
        .bind(ValidTo)
        .bind(SupersededBy)
        .bind(SourceSessionId)
        .bind(RecordedAt)
        .bind(AccessCount)
        .bind(LastAccessedAt)
        .bind(StabilityHours)
        .bind(FactType)
        .bind(IsForgotten)
        .bind(ForgottenAt)
        .bind(ForgetReason)
        .filter("nous_id = $nous_id")
        .filter("is_forgotten == false")
        .filter("valid_from <= $at_time")
        .filter("valid_to > $at_time")
        .order("-confidence")
        .done()
        .build_script()
}

/// Bi-temporal point-in-time query with optional content filter. Params: `$nous_id`, `$at_time`.
/// Same as `temporal_facts` but uses a raw script to support an optional `contains()` filter.
pub const TEMPORAL_FACTS_FILTERED: &str = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to,
      superseded_by, source_session_id,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason] :=
        *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
               superseded_by, source_session_id, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason},
        nous_id = $nous_id,
        is_forgotten == false,
        valid_from <= $at_time,
        valid_to > $at_time,
        str_includes(content, $filter)
    :order -confidence
";

/// Facts that changed (became valid or expired) in an interval.
/// Params: `$nous_id`, `$from_time`, `$to_time`.
/// Returns all facts where `valid_from` is in `(from_time, to_time]` OR
/// `valid_to` is in `(from_time, to_time]`.
pub const TEMPORAL_DIFF_ADDED: &str = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to,
      superseded_by, source_session_id,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason] :=
        *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
               superseded_by, source_session_id, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason},
        nous_id = $nous_id,
        is_forgotten == false,
        valid_from > $from_time,
        valid_from <= $to_time
";

/// Facts that expired (`valid_to` fell) in an interval.
/// Params: `$nous_id`, `$from_time`, `$to_time`.
pub const TEMPORAL_DIFF_REMOVED: &str = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to,
      superseded_by, source_session_id,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason] :=
        *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
               superseded_by, source_session_id, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason},
        nous_id = $nous_id,
        is_forgotten == false,
        valid_to > $from_time,
        valid_to <= $to_time,
        valid_to != '9999-12-31'
";

/// Query returning only forgotten facts. Params: `$nous_id`, `$limit`.
#[must_use]
pub fn forgotten_facts() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[
            Id,
            Content,
            Confidence,
            Tier,
            RecordedAt,
            NousId,
            ValidFrom,
            ValidTo,
            SupersededBy,
            SourceSessionId,
            AccessCount,
            LastAccessedAt,
            StabilityHours,
            FactType,
            IsForgotten,
            ForgottenAt,
            ForgetReason,
        ])
        .bind(Id)
        .bind(ValidFrom)
        .bind(Content)
        .bind(NousId)
        .bind(Confidence)
        .bind(Tier)
        .bind(ValidTo)
        .bind(SupersededBy)
        .bind(SourceSessionId)
        .bind(RecordedAt)
        .bind(AccessCount)
        .bind(LastAccessedAt)
        .bind(StabilityHours)
        .bind(FactType)
        .bind(IsForgotten)
        .bind(ForgottenAt)
        .bind(ForgetReason)
        .filter("nous_id = $nous_id")
        .filter("is_forgotten == true")
        .order("-forgotten_at")
        .limit("$limit")
        .done()
        .build_script()
}

/// Audit query returning all facts regardless of forgotten/superseded/temporal state.
/// Params: `$nous_id`, `$limit`.
#[must_use]
pub fn audit_all_facts() -> String {
    use FactsField::*;
    QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[
            Id,
            Content,
            Confidence,
            Tier,
            RecordedAt,
            NousId,
            ValidFrom,
            ValidTo,
            SupersededBy,
            SourceSessionId,
            AccessCount,
            LastAccessedAt,
            StabilityHours,
            FactType,
            IsForgotten,
            ForgottenAt,
            ForgetReason,
        ])
        .bind(Id)
        .bind(ValidFrom)
        .bind(Content)
        .bind(NousId)
        .bind(Confidence)
        .bind(Tier)
        .bind(ValidTo)
        .bind(SupersededBy)
        .bind(SourceSessionId)
        .bind(RecordedAt)
        .bind(AccessCount)
        .bind(LastAccessedAt)
        .bind(StabilityHours)
        .bind(FactType)
        .bind(IsForgotten)
        .bind(ForgottenAt)
        .bind(ForgetReason)
        .filter("nous_id = $nous_id")
        .order("-recorded_at")
        .limit("$limit")
        .done()
        .build_script()
}

/// Remove a fact-entity mapping.
/// Params: `$fact_id`, `$entity_id`.
#[must_use]
pub fn rm_fact_entity() -> String {
    use FactEntitiesField::{EntityId, FactId};
    QueryBuilder::new()
        .rm(Relation::FactEntities)
        .keys(&[FactId, EntityId])
        .done()
        .build_script()
}

/// Insert or update a fact-entity mapping.
/// Params: `$fact_id`, `$entity_id`, `$created_at`.
#[must_use]
pub fn upsert_fact_entity() -> String {
    use FactEntitiesField::*;
    QueryBuilder::new()
        .put(Relation::FactEntities)
        .keys(&[FactId, EntityId])
        .values(&[CreatedAt])
        .done()
        .build_script()
}

/// Remove an entity.
/// Params: `$id`.
#[must_use]
pub fn rm_entity() -> String {
    use EntitiesField::Id;
    QueryBuilder::new()
        .rm(Relation::Entities)
        .keys(&[Id])
        .done()
        .build_script()
}

/// Remove a relationship edge.
/// Params: `$src`, `$dst`.
#[must_use]
pub fn rm_relationship() -> String {
    use RelationshipsField::{Dst, Src};
    QueryBuilder::new()
        .rm(Relation::Relationships)
        .keys(&[Src, Dst])
        .done()
        .build_script()
}

/// Remove a pending-merge entry.
/// Params: `$entity_a`, `$entity_b`.
#[must_use]
pub fn rm_pending_merges() -> String {
    use PendingMergesField::{EntityA, EntityB};
    QueryBuilder::new()
        .rm(Relation::PendingMerges)
        .keys(&[EntityA, EntityB])
        .done()
        .build_script()
}

/// Insert or update a merge-audit record.
/// Params: `$canonical_id`, `$merged_id`, `$merged_name`, `$merge_score`,
/// `$facts_transferred`, `$relationships_redirected`, `$merged_at`.
#[must_use]
pub fn put_merge_audit() -> String {
    use MergeAuditField::*;
    QueryBuilder::new()
        .put(Relation::MergeAudit)
        .keys(&[CanonicalId, MergedId])
        .values(&[
            MergedName,
            MergeScore,
            FactsTransferred,
            RelationshipsRedirected,
            MergedAt,
        ])
        .done()
        .build_script()
}

/// Insert or update a pending-merge candidate.
/// Params: `$entity_a`, `$entity_b`, `$name_a`, `$name_b`, `$name_similarity`,
/// `$embed_similarity`, `$type_match`, `$alias_overlap`, `$merge_score`, `$created_at`.
#[must_use]
pub fn put_pending_merge() -> String {
    use PendingMergesField::*;
    QueryBuilder::new()
        .put(Relation::PendingMerges)
        .keys(&[EntityA, EntityB])
        .values(&[
            NameA,
            NameB,
            NameSimilarity,
            EmbedSimilarity,
            TypeMatch,
            AliasOverlap,
            MergeScore,
            CreatedAt,
        ])
        .done()
        .build_script()
}
