//! Builder-generated query scripts for `KnowledgeStore` operations.

use super::{
    CausalEdgesField, EmbeddingsField, EntitiesField, EntityFlagsField, FactEntitiesField,
    FactsField, MergeAuditField, PendingMergesField, QueryBuilder, Relation, RelationshipsField,
    ScanBuilder,
};

/// Canonical `?[...]` projection for full [`Fact`](crate::knowledge::Fact)
/// hydration. The column order MUST match the positional decoding in
/// `crate::knowledge_store::marshal::rows_to_facts`: `id`(0), `content`(1), …,
/// `scope`(17), `project_id`(18), `visibility`(19), `sensitivity`(20).
///
/// Every query whose rows are passed to `rows_to_facts` must project exactly
/// these fields in this order. Centralizing the list keeps temporal, forgotten,
/// audit, and current-fact reads from drifting out of the marshal contract and
/// silently mis-hydrating policy fields (#4677/#4549).
pub(crate) const FULL_FACT_SELECT: [FactsField; 21] = {
    use FactsField::{
        AccessCount, Confidence, Content, FactType, ForgetReason, ForgottenAt, Id, IsForgotten,
        LastAccessedAt, NousId, ProjectId, RecordedAt, Scope, Sensitivity, SourceSessionId,
        StabilityHours, SupersededBy, Tier, ValidFrom, ValidTo, Visibility,
    };
    [
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
        Scope,
        ProjectId,
        Visibility,
        Sensitivity,
    ]
};

/// Canonical `*facts{...}` relation bindings for full-fact scans, in schema
/// declaration order. Binding is by field name so the order is cosmetic, but a
/// single source avoids drift from [`FULL_FACT_SELECT`].
const FULL_FACT_BIND: [FactsField; 21] = {
    use FactsField::{
        AccessCount, Confidence, Content, FactType, ForgetReason, ForgottenAt, Id, IsForgotten,
        LastAccessedAt, NousId, ProjectId, RecordedAt, Scope, Sensitivity, SourceSessionId,
        StabilityHours, SupersededBy, Tier, ValidFrom, ValidTo, Visibility,
    };
    [
        Id,
        ValidFrom,
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
        Scope,
        ProjectId,
        Visibility,
        Sensitivity,
    ]
};

// The canonical projection length is the marshal hydration contract; keep the
// two in lockstep so a column added to one fails to compile until the other and
// `rows_to_facts` agree (#4677).
#[cfg(feature = "mneme-engine")]
const _: () = assert!(
    FULL_FACT_SELECT.len() == crate::knowledge_store::marshal::FULL_FACT_COLUMNS,
    "FULL_FACT_SELECT must match rows_to_facts FULL_FACT_COLUMNS"
);

/// Begin a `*facts{...}` scan projecting the canonical full-fact column set.
/// Callers add filters/order/limit and finish with `.done().build_script()`.
fn full_fact_scan() -> ScanBuilder {
    let mut scan = QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&FULL_FACT_SELECT);
    for field in FULL_FACT_BIND {
        scan = scan.bind(field);
    }
    scan
}

/// Insert or update a fact. Params: `$id`, `$valid_from`, `$content`,
/// `$nous_id`, `$confidence`, `$tier`, `$valid_to`, `$superseded_by`,
/// `$source_session_id`, `$recorded_at`.
#[must_use]
pub(crate) fn upsert_fact() -> String {
    use FactsField::{
        AccessCount, Confidence, Content, FactType, ForgetReason, ForgottenAt, Id, IsForgotten,
        LastAccessedAt, NousId, ProjectId, RecordedAt, Scope, Sensitivity, SourceSessionId,
        StabilityHours, SupersededBy, Tier, ValidFrom, ValidTo, Visibility,
    };
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
            Scope,
            ProjectId,
            Visibility,
            Sensitivity,
        ])
        .done()
        .build_script()
}

/// Query current facts for a nous (not superseded, currently valid).
/// Params: `$nous_id`, `$now`, `$limit`.
#[must_use]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Datalog query for current (non-superseded) facts")
)]
pub(crate) fn current_facts() -> String {
    use FactsField::{
        AccessCount, Confidence, Content, FactType, ForgetReason, ForgottenAt, Id, IsForgotten,
        LastAccessedAt, NousId, ProjectId, RecordedAt, Scope, Sensitivity, StabilityHours,
        SupersededBy, Tier, ValidFrom, ValidTo, Visibility,
    };
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
        .bind(Scope)
        .bind(ProjectId)
        .bind(Visibility)
        .bind(Sensitivity)
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
pub(crate) fn full_current_facts() -> String {
    full_fact_scan()
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
pub(crate) fn facts_at_time() -> String {
    use FactsField::{
        Confidence, Content, Id, IsForgotten, ProjectId, Scope, Sensitivity, Tier, ValidFrom,
        ValidTo, Visibility,
    };
    QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content, Confidence, Tier, Sensitivity])
        .bind(Id)
        .bind(ValidFrom)
        .bind(Content)
        .bind(Confidence)
        .bind(Tier)
        .bind(ValidTo)
        .bind(Sensitivity)
        .bind(IsForgotten)
        .bind(Scope)
        .bind(ProjectId)
        .bind(Visibility)
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
pub(crate) fn supersede_fact() -> String {
    use FactsField::{
        AccessCount, Confidence, Content, FactType, ForgetReason, ForgottenAt, Id, IsForgotten,
        LastAccessedAt, NousId, ProjectId, RecordedAt, Scope, Sensitivity, SourceSessionId,
        StabilityHours, SupersededBy, Tier, ValidFrom, ValidTo, Visibility,
    };
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
            Scope,
            ProjectId,
            Visibility,
            Sensitivity,
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
            "$old_scope",
            "$old_project_id",
            "$old_visibility",
            "$old_sensitivity",
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
            "$scope",
            "$project_id",
            "$visibility",
            "$sensitivity",
        ])
        .done()
        .build_script()
}

/// Insert or update an entity.
/// Params: `$id`, `$name`, `$entity_type`, `$aliases`, `$created_at`,
/// `$updated_at`, `$name_embedding`.
///
/// `$name_embedding` may be `DataValue::Null` for callers without an
/// `EmbeddingProvider` in scope; the dedup pipeline treats NULL as
/// `embed_sim = 0.0`. See `KnowledgeStore::update_entity_name_embedding`
/// and `KnowledgeStore::run_entity_dedup_with_embeddings` for the
/// backfill path (#4165 / Path A).
#[must_use]
pub(crate) fn upsert_entity() -> String {
    use EntitiesField::{Aliases, CreatedAt, EntityType, Id, Name, NameEmbedding, UpdatedAt};
    QueryBuilder::new()
        .put(Relation::Entities)
        .keys(&[Id])
        .values(&[
            Name,
            EntityType,
            Aliases,
            CreatedAt,
            UpdatedAt,
            NameEmbedding,
        ])
        .done()
        .build_script()
}

/// Insert a relationship.
/// Params: `$src`, `$dst`, `$relation`, `$weight`, `$created_at`.
#[must_use]
pub(crate) fn upsert_relationship() -> String {
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
pub(crate) fn upsert_embedding() -> String {
    use EmbeddingsField::{Content, CreatedAt, Embedding, Id, NousId, SourceId, SourceType};
    QueryBuilder::new()
        .put(Relation::Embeddings)
        .keys(&[Id])
        .values(&[Content, SourceType, SourceId, NousId, Embedding, CreatedAt])
        .done()
        .build_script()
}

/// 2-hop entity neighborhood. Params: `$entity_id`.
pub(crate) const ENTITY_NEIGHBORHOOD: &str = r"
    hop1[dst, rel] := *relationships{src: $entity_id, dst, relation: rel}
    hop2[dst, rel] := hop1[mid, _], *relationships{src: mid, dst, relation: rel}
    ?[id, name, entity_type, relation, hop] :=
        hop1[id, relation], *entities{id, name, entity_type}, hop = 1
    ?[id, name, entity_type, relation, hop] :=
        hop2[id, relation], *entities{id, name, entity_type}, hop = 2
    :order hop, name
";

/// BM25 full-text recall (no vector embeddings required).
/// Returns rows: id, content, `source_type`, `source_id`, dist, scope, `project_id`,
/// visibility, `nous_id`, sensitivity.
/// Params: `$query_text`, `$k`.
pub(crate) const BM25_RECALL: &str = r"
    bm25[id, score] := ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}

    ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
        bm25[id, bm25_score],
        *facts{id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity},
        is_forgotten == false,
        is_null(superseded_by),
        source_type = 'fact',
        source_id = id,
        dist = 1.0 / bm25_score
    :order dist
    :limit $k
";

/// KNN vector search. Params: `$query_vec`, `$k`, `$ef`.
/// Returns rows: id, content, `source_type`, `source_id`, dist, scope(null), `project_id`(null),
/// visibility(empty), `nous_id`, sensitivity(empty). The `scope`, `project_id`, `visibility`,
/// and `sensitivity` columns are placeholders hydrated by `search_vectors` from the facts table;
/// `nous_id` comes directly from the embeddings relation.
pub(crate) const SEMANTIC_SEARCH: &str = r"
    ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
        ~embeddings:semantic_idx {id, content, source_type, source_id, nous_id |
            query: $query_vec, k: $k, ef: $ef, bind_distance: dist},
        scope = null,
        project_id = null,
        visibility = '',
        sensitivity = ''
";

#[expect(
    dead_code,
    reason = "Datalog query for entity prefix search — no callers yet"
)]
/// Entity search by name or alias (prefix match). Params: `$prefix`, `$limit`.
pub(crate) const SEARCH_ENTITIES: &str = r"
    ?[id, name, entity_type] :=
        *entities{id, name, entity_type},
        starts_with(name, $prefix)
    ?[id, name, entity_type] :=
        *entities{id, name, entity_type, aliases},
        contains(aliases, $prefix)
    :limit $limit
";

/// Hybrid search: BM25 + HNSW vector + graph neighborhood fused via RRF.
/// The BM25 and graph sub-rules are injected dynamically by `build_hybrid_query`
/// (`{BM25_RULE}` is the full-text rule, or an empty relation when the query has
/// no text terms; `{GRAPH_RULES}` is the entity-graph expansion).
/// Params: `$query_text` (only when text terms are present), `$query_vec`, `$k`, `$ef`, `$limit`.
pub(crate) const HYBRID_SEARCH_BASE: &str = r"
    {BM25_RULE}

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
pub(crate) fn temporal_facts() -> String {
    full_fact_scan()
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
pub(crate) const TEMPORAL_FACTS_FILTERED: &str = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to,
      superseded_by, source_session_id,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility, sensitivity] :=
        *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
               superseded_by, source_session_id, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility, sensitivity},
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
pub(crate) const TEMPORAL_DIFF_ADDED: &str = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to,
      superseded_by, source_session_id,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility, sensitivity] :=
        *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
               superseded_by, source_session_id, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility, sensitivity},
        nous_id = $nous_id,
        is_forgotten == false,
        valid_from > $from_time,
        valid_from <= $to_time
";

/// Facts that expired (`valid_to` fell) in an interval.
/// Params: `$nous_id`, `$from_time`, `$to_time`.
pub(crate) const TEMPORAL_DIFF_REMOVED: &str = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to,
      superseded_by, source_session_id,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility, sensitivity] :=
        *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
               superseded_by, source_session_id, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility, sensitivity},
        nous_id = $nous_id,
        is_forgotten == false,
        valid_to > $from_time,
        valid_to <= $to_time,
        valid_to != '9999-12-31'
";

/// Query returning only forgotten facts. Params: `$nous_id`, `$limit`.
#[must_use]
pub(crate) fn forgotten_facts() -> String {
    full_fact_scan()
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
pub(crate) fn audit_all_facts() -> String {
    full_fact_scan()
        .filter("nous_id = $nous_id")
        .order("-recorded_at")
        .limit("$limit")
        .done()
        .build_script()
}

/// Remove a fact-entity mapping.
/// Params: `$fact_id`, `$entity_id`.
#[must_use]
pub(crate) fn rm_fact_entity() -> String {
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
pub(crate) fn upsert_fact_entity() -> String {
    use FactEntitiesField::{CreatedAt, EntityId, FactId};
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
pub(crate) fn rm_entity() -> String {
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
pub(crate) fn rm_relationship() -> String {
    use RelationshipsField::{Dst, Src};
    QueryBuilder::new()
        .rm(Relation::Relationships)
        .keys(&[Src, Dst])
        .done()
        .build_script()
}

/// Remove a pending-merge entry.
/// Params: `$nous_id`, `$entity_a`, `$entity_b`.
#[must_use]
pub(crate) fn rm_pending_merges() -> String {
    use PendingMergesField::{EntityA, EntityB, NousId};
    QueryBuilder::new()
        .rm(Relation::PendingMerges)
        .keys(&[NousId, EntityA, EntityB])
        .done()
        .build_script()
}

/// Insert or update a merge-audit record.
/// Params: `$nous_id`, `$canonical_id`, `$merged_id`, `$merged_name`, `$merge_score`,
/// `$facts_transferred`, `$relationships_redirected`, `$merged_at`.
#[must_use]
pub(crate) fn put_merge_audit() -> String {
    use MergeAuditField::{
        CanonicalId, FactsTransferred, MergeScore, MergedAt, MergedId, MergedName, NousId,
        RelationshipsRedirected,
    };
    QueryBuilder::new()
        .put(Relation::MergeAudit)
        .keys(&[NousId, CanonicalId, MergedId])
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
/// Params: `$nous_id`, `$entity_a`, `$entity_b`, `$name_a`, `$name_b`,
/// `$name_similarity`, `$embed_similarity`, `$type_match`, `$alias_overlap`,
/// `$merge_score`, `$created_at`.
#[must_use]
pub(crate) fn put_pending_merge() -> String {
    use PendingMergesField::{
        AliasOverlap, CreatedAt, EmbedSimilarity, EntityA, EntityB, MergeScore, NameA, NameB,
        NameSimilarity, NousId, TypeMatch,
    };
    QueryBuilder::new()
        .put(Relation::PendingMerges)
        .keys(&[NousId, EntityA, EntityB])
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

/// Insert or update an entity review flag.
/// Params: `$entity_id`, `$reason`, `$severity`, `$flagged_by`, `$flagged_at`.
#[must_use]
pub(crate) fn upsert_entity_flag() -> String {
    use EntityFlagsField::{EntityId, FlaggedAt, FlaggedBy, Reason, Severity};
    QueryBuilder::new()
        .put(Relation::EntityFlags)
        .keys(&[EntityId])
        .values(&[Reason, Severity, FlaggedBy, FlaggedAt])
        .done()
        .build_script()
}

/// Remove an entity review flag.
/// Params: `$entity_id`.
#[must_use]
pub(crate) fn rm_entity_flag() -> String {
    use EntityFlagsField::EntityId;
    QueryBuilder::new()
        .rm(Relation::EntityFlags)
        .keys(&[EntityId])
        .done()
        .build_script()
}

/// Insert or update a causal edge.
/// Params: `$cause`, `$effect`, `$id`, `$ordering`, `$relationship_type`,
/// `$confidence`, `$evidence_session_id`, `$created_at`.
#[must_use]
pub(crate) fn upsert_causal_edge() -> String {
    use CausalEdgesField::{
        Cause, Confidence, CreatedAt, Effect, EvidenceSessionId, Id, Ordering, RelationshipType,
    };
    QueryBuilder::new()
        .put(Relation::CausalEdges)
        .keys(&[Cause, Effect])
        .values(&[
            Id,
            Ordering,
            RelationshipType,
            Confidence,
            EvidenceSessionId,
            CreatedAt,
        ])
        .done()
        .build_script()
}

/// Remove a causal edge.
/// Params: `$cause`, `$effect`.
#[must_use]
pub(crate) fn rm_causal_edge() -> String {
    use CausalEdgesField::{Cause, Effect};
    QueryBuilder::new()
        .rm(Relation::CausalEdges)
        .keys(&[Cause, Effect])
        .done()
        .build_script()
}
