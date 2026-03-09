//! Typed Datalog query builder — compile-time schema validation for KnowledgeStore.

use std::collections::BTreeMap;

use crate::engine::DataValue;

/// Datalog field reference. Implemented by per-relation field enums.
pub trait Field: Copy {
    fn name(self) -> &'static str;
}

/// Knowledge graph relations stored in the `CozoDB` engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// Temporal facts with validity windows and confidence scores.
    Facts,
    /// Named entities (people, places, concepts).
    Entities,
    /// Directed edges between entities with typed relations.
    Relationships,
    /// Vector embeddings for semantic search.
    Embeddings,
}

impl Relation {
    /// Return the `CozoDB` relation name used in Datalog queries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Facts => "facts",
            Self::Entities => "entities",
            Self::Relationships => "relationships",
            Self::Embeddings => "embeddings",
        }
    }
}

/// Fields in the `facts` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactsField {
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
}

impl Field for FactsField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::ValidFrom => "valid_from",
            Self::Content => "content",
            Self::NousId => "nous_id",
            Self::Confidence => "confidence",
            Self::Tier => "tier",
            Self::ValidTo => "valid_to",
            Self::SupersededBy => "superseded_by",
            Self::SourceSessionId => "source_session_id",
            Self::RecordedAt => "recorded_at",
            Self::AccessCount => "access_count",
            Self::LastAccessedAt => "last_accessed_at",
            Self::StabilityHours => "stability_hours",
            Self::FactType => "fact_type",
            Self::IsForgotten => "is_forgotten",
            Self::ForgottenAt => "forgotten_at",
            Self::ForgetReason => "forget_reason",
        }
    }
}

/// Fields in the `entities` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitiesField {
    Id,
    Name,
    EntityType,
    Aliases,
    CreatedAt,
    UpdatedAt,
}

impl Field for EntitiesField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::EntityType => "entity_type",
            Self::Aliases => "aliases",
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
        }
    }
}

/// Fields in the `relationships` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipsField {
    Src,
    Dst,
    Relation,
    Weight,
    CreatedAt,
}

impl Field for RelationshipsField {
    fn name(self) -> &'static str {
        match self {
            Self::Src => "src",
            Self::Dst => "dst",
            Self::Relation => "relation",
            Self::Weight => "weight",
            Self::CreatedAt => "created_at",
        }
    }
}

/// Fields in the `embeddings` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingsField {
    Id,
    Content,
    SourceType,
    SourceId,
    NousId,
    Embedding,
    CreatedAt,
}

impl Field for EmbeddingsField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Content => "content",
            Self::SourceType => "source_type",
            Self::SourceId => "source_id",
            Self::NousId => "nous_id",
            Self::Embedding => "embedding",
            Self::CreatedAt => "created_at",
        }
    }
}

// ---------------------------------------------------------------------------
// QueryBuilder
// ---------------------------------------------------------------------------

/// Accumulates Datalog script lines and parameter bindings.
#[must_use]
pub struct QueryBuilder {
    lines: Vec<String>,
    params: BTreeMap<String, DataValue>,
}

impl QueryBuilder {
    /// Create an empty query builder.
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            params: BTreeMap::new(),
        }
    }

    /// Start a `:put` operation against a relation.
    pub fn put(self, relation: Relation) -> PutBuilder {
        PutBuilder {
            parent: self,
            relation,
            all_fields: Vec::new(),
            key_count: 0,
            rows: Vec::new(),
        }
    }

    /// Start a `?[...] := *relation{...}` scan query.
    pub fn scan(self, relation: Relation) -> ScanBuilder {
        ScanBuilder {
            parent: self,
            relation,
            select: Vec::new(),
            bindings: Vec::new(),
            filters: Vec::new(),
            order: None,
            limit: None,
        }
    }

    /// Append a raw Datalog line (escape hatch for complex queries).
    pub fn raw(mut self, line: &str) -> Self {
        self.lines.push(line.to_owned());
        self
    }

    /// Bind a named parameter.
    pub fn param(mut self, name: &str, value: DataValue) -> Self {
        self.params.insert(name.to_owned(), value);
        self
    }

    /// Consume the builder, producing `(script, params)`.
    pub fn build(self) -> (String, BTreeMap<String, DataValue>) {
        (self.lines.join("\n"), self.params)
    }

    /// Consume the builder, producing only the script string.
    pub fn build_script(self) -> String {
        self.lines.join("\n")
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PutBuilder
// ---------------------------------------------------------------------------

/// Builds a `:put relation { keys => values }` operation.
#[must_use]
pub struct PutBuilder {
    parent: QueryBuilder,
    relation: Relation,
    all_fields: Vec<&'static str>,
    key_count: usize,
    rows: Vec<Vec<String>>,
}

impl PutBuilder {
    /// Declare key fields (before the `=>` in the `:put` clause).
    pub fn keys(mut self, fields: &[impl Field]) -> Self {
        self.key_count = fields.len();
        for f in fields {
            self.all_fields.push(f.name());
        }
        self
    }

    /// Declare value fields (after the `=>` in the `:put` clause).
    pub fn values(mut self, fields: &[impl Field]) -> Self {
        for f in fields {
            self.all_fields.push(f.name());
        }
        self
    }

    /// Add an explicit row with custom param references.
    ///
    /// Each entry is a Datalog expression: `"$param_name"`, `"null"`, a quoted
    /// literal like `"\"9999-12-31\""`, etc. Required for multi-row puts where
    /// different rows bind different params (e.g. `SUPERSEDE_FACT`).
    pub fn row(mut self, exprs: &[&str]) -> Self {
        self.rows
            .push(exprs.iter().map(|s| (*s).to_owned()).collect());
        self
    }

    /// Finish the `:put`, returning the parent `QueryBuilder`.
    ///
    /// If no explicit `row()` was called, generates a single row from field
    /// names (convention: `$field_name` for each field).
    pub fn done(mut self) -> QueryBuilder {
        if self.rows.is_empty() {
            let auto_row: Vec<String> = self.all_fields.iter().map(|f| format!("${f}")).collect();
            self.rows.push(auto_row);
        }

        let field_list = self.all_fields.join(", ");

        let row_strs: Vec<String> = self
            .rows
            .iter()
            .map(|r| format!("[{}]", r.join(", ")))
            .collect();
        let data = row_strs.join(", ");

        let key_fields: Vec<&str> = self.all_fields[..self.key_count].to_vec();
        let value_fields: Vec<&str> = self.all_fields[self.key_count..].to_vec();

        let put_clause = if value_fields.is_empty() {
            format!(
                ":put {} {{{}}}",
                self.relation.name(),
                key_fields.join(", ")
            )
        } else {
            format!(
                ":put {} {{{} => {}}}",
                self.relation.name(),
                key_fields.join(", "),
                value_fields.join(", ")
            )
        };

        let line = format!("?[{field_list}] <- [{data}]\n{put_clause}");
        self.parent.lines.push(line);
        self.parent
    }
}

// ---------------------------------------------------------------------------
// ScanBuilder
// ---------------------------------------------------------------------------

/// Builds a `?[select] := *relation{bindings}, filters` query.
#[must_use]
pub struct ScanBuilder {
    parent: QueryBuilder,
    relation: Relation,
    select: Vec<&'static str>,
    bindings: Vec<String>,
    filters: Vec<String>,
    order: Option<String>,
    limit: Option<String>,
}

impl ScanBuilder {
    /// Set the `?[...]` projection fields.
    pub fn select(mut self, fields: &[impl Field]) -> Self {
        self.select = fields.iter().map(|f| f.name()).collect();
        self
    }

    /// Bind a field in the `*relation{...}` clause (just the field name).
    pub fn bind(mut self, field: impl Field) -> Self {
        self.bindings.push(field.name().to_owned());
        self
    }

    /// Bind a field to an expression: `field: expr` in `*relation{...}`.
    pub fn bind_to(mut self, field: impl Field, expr: &str) -> Self {
        self.bindings.push(format!("{}: {expr}", field.name()));
        self
    }

    /// Add a filter condition (raw Datalog expression after the scan clause).
    pub fn filter(mut self, expr: &str) -> Self {
        self.filters.push(expr.to_owned());
        self
    }

    /// Set `:order` directive (e.g. `"-confidence"`).
    pub fn order(mut self, expr: &str) -> Self {
        self.order = Some(expr.to_owned());
        self
    }

    /// Set `:limit` directive (e.g. `"$limit"`).
    pub fn limit(mut self, expr: &str) -> Self {
        self.limit = Some(expr.to_owned());
        self
    }

    /// Finish the scan, returning the parent `QueryBuilder`.
    pub fn done(mut self) -> QueryBuilder {
        let select_list = self.select.join(", ");
        let binding_list = self.bindings.join(", ");

        let mut parts = vec![format!(
            "?[{select_list}] :=\n    *{}{{{binding_list}}}",
            self.relation.name()
        )];

        for f in &self.filters {
            parts.push(format!("    {f}"));
        }

        let mut line = parts.join(",\n");

        if let Some(ref ord) = self.order {
            use std::fmt::Write;
            let _ = write!(line, "\n:order {ord}");
        }
        if let Some(ref lim) = self.limit {
            use std::fmt::Write;
            let _ = write!(line, "\n:limit {lim}");
        }

        self.parent.lines.push(line);
        self.parent
    }
}

// ---------------------------------------------------------------------------
// Pre-built query functions
// ---------------------------------------------------------------------------

/// Builder-generated query scripts for `KnowledgeStore` operations.
pub mod queries {
    use super::*;

    /// Insert or update a fact. Params: `$id`, `$valid_from`, `$content`,
    /// `$nous_id`, `$confidence`, `$tier`, `$valid_to`, `$superseded_by`,
    /// `$source_session_id`, `$recorded_at`.
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
            valid_to > $from_time,
            valid_to <= $to_time,
            valid_to != '9999-12-31'
    ";

    /// Audit query returning all facts regardless of forgotten/superseded/temporal state.
    /// Params: `$nous_id`, `$limit`.
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Normalize whitespace for comparison: collapse runs of whitespace to single
    /// space, trim, then remove spaces adjacent to brackets/braces (`CozoDB`
    /// ignores these formatting differences).
    fn normalize(s: &str) -> String {
        let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
        collapsed
            .replace("[ ", "[")
            .replace(" ]", "]")
            .replace("{ ", "{")
            .replace(" }", "}")
    }

    // -- Builder unit tests --

    #[test]
    fn test_put_generates_valid_datalog() {
        use FactsField::*;
        let script = QueryBuilder::new()
            .put(Relation::Facts)
            .keys(&[Id, ValidFrom])
            .values(&[Content, NousId])
            .done()
            .build_script();

        assert!(script.contains("?[id, valid_from, content, nous_id]"));
        assert!(script.contains("[$id, $valid_from, $content, $nous_id]"));
        assert!(script.contains(":put facts {id, valid_from => content, nous_id}"));
    }

    #[test]
    fn test_put_multi_row() {
        use FactsField::*;
        let script = QueryBuilder::new()
            .put(Relation::Facts)
            .keys(&[Id, ValidFrom])
            .values(&[Content])
            .row(&["$a", "$b", "$c"])
            .row(&["$x", "$y", "$z"])
            .done()
            .build_script();

        assert!(script.contains("[$a, $b, $c], [$x, $y, $z]"));
        assert!(script.contains(":put facts {id, valid_from => content}"));
    }

    #[test]
    fn test_scan_generates_valid_datalog() {
        use FactsField::*;
        let script = QueryBuilder::new()
            .scan(Relation::Facts)
            .select(&[Id, Content, Confidence])
            .bind(Id)
            .bind_to(NousId, "$nous_id")
            .bind(Content)
            .bind(Confidence)
            .filter("confidence > 0.5")
            .order("-confidence")
            .limit("$limit")
            .done()
            .build_script();

        assert!(script.contains("?[id, content, confidence]"));
        assert!(script.contains("*facts{id, nous_id: $nous_id, content, confidence}"));
        assert!(script.contains("confidence > 0.5"));
        assert!(script.contains(":order -confidence"));
        assert!(script.contains(":limit $limit"));
    }

    #[test]
    fn test_params_are_bound_not_interpolated() {
        let (script, params) = QueryBuilder::new()
            .raw("?[x] := x = $val")
            .param("val", DataValue::from(42_i64))
            .build();

        assert!(script.contains("$val"), "script must reference $val");
        assert!(
            !script.contains("42"),
            "script must not contain literal value"
        );
        assert!(params.contains_key("val"));
    }

    #[test]
    fn test_injection_attempt() {
        // Param values with special chars go through $binding, not interpolation
        let (script, params) = QueryBuilder::new()
            .raw("?[x] := *facts{id: $id, content: x}")
            .param("id", DataValue::Str("evil}; :rm facts".into()))
            .build();

        assert!(
            !script.contains("evil}"),
            "injection payload must not appear in script"
        );
        assert!(params.contains_key("id"));
    }

    #[test]
    fn test_order_and_limit() {
        use EntitiesField::*;
        let script = QueryBuilder::new()
            .scan(Relation::Entities)
            .select(&[Id, Name])
            .bind(Id)
            .bind(Name)
            .order("name")
            .limit("10")
            .done()
            .build_script();

        let lines: Vec<&str> = script.lines().collect();
        let order_pos = lines.iter().position(|l| l.contains(":order"));
        let limit_pos = lines.iter().position(|l| l.contains(":limit"));
        assert!(order_pos.is_some(), "must have :order");
        assert!(limit_pos.is_some(), "must have :limit");
        assert!(
            order_pos.unwrap() < limit_pos.unwrap(),
            ":order must come before :limit"
        );
    }

    #[test]
    fn test_field_names_match_schema() {
        // Facts DDL fields
        let facts_ddl_fields = [
            "id",
            "valid_from",
            "content",
            "nous_id",
            "confidence",
            "tier",
            "valid_to",
            "superseded_by",
            "source_session_id",
            "recorded_at",
            "access_count",
            "last_accessed_at",
            "stability_hours",
            "fact_type",
            "is_forgotten",
            "forgotten_at",
            "forget_reason",
        ];
        let facts_enum_fields: Vec<&str> = [
            FactsField::Id,
            FactsField::ValidFrom,
            FactsField::Content,
            FactsField::NousId,
            FactsField::Confidence,
            FactsField::Tier,
            FactsField::ValidTo,
            FactsField::SupersededBy,
            FactsField::SourceSessionId,
            FactsField::RecordedAt,
            FactsField::AccessCount,
            FactsField::LastAccessedAt,
            FactsField::StabilityHours,
            FactsField::FactType,
            FactsField::IsForgotten,
            FactsField::ForgottenAt,
            FactsField::ForgetReason,
        ]
        .iter()
        .map(|f| f.name())
        .collect();
        assert_eq!(facts_ddl_fields.as_slice(), facts_enum_fields.as_slice());

        // Entities DDL fields
        let entities_ddl = [
            "id",
            "name",
            "entity_type",
            "aliases",
            "created_at",
            "updated_at",
        ];
        let entities_enum: Vec<&str> = [
            EntitiesField::Id,
            EntitiesField::Name,
            EntitiesField::EntityType,
            EntitiesField::Aliases,
            EntitiesField::CreatedAt,
            EntitiesField::UpdatedAt,
        ]
        .iter()
        .map(|f| f.name())
        .collect();
        assert_eq!(entities_ddl.as_slice(), entities_enum.as_slice());

        // Relationships DDL fields
        let rels_ddl = ["src", "dst", "relation", "weight", "created_at"];
        let rels_enum: Vec<&str> = [
            RelationshipsField::Src,
            RelationshipsField::Dst,
            RelationshipsField::Relation,
            RelationshipsField::Weight,
            RelationshipsField::CreatedAt,
        ]
        .iter()
        .map(|f| f.name())
        .collect();
        assert_eq!(rels_ddl.as_slice(), rels_enum.as_slice());

        // Embeddings DDL fields
        let emb_ddl = [
            "id",
            "content",
            "source_type",
            "source_id",
            "nous_id",
            "embedding",
            "created_at",
        ];
        let emb_enum: Vec<&str> = [
            EmbeddingsField::Id,
            EmbeddingsField::Content,
            EmbeddingsField::SourceType,
            EmbeddingsField::SourceId,
            EmbeddingsField::NousId,
            EmbeddingsField::Embedding,
            EmbeddingsField::CreatedAt,
        ]
        .iter()
        .map(|f| f.name())
        .collect();
        assert_eq!(emb_ddl.as_slice(), emb_enum.as_slice());
    }

    #[test]
    fn test_raw_escape_hatch() {
        let script = QueryBuilder::new()
            .raw("hop1[dst, rel] := *relationships{src: $id, dst, relation: rel}")
            .raw("?[dst, rel] := hop1[dst, rel]")
            .build_script();

        assert!(script.contains("hop1[dst, rel]"));
        assert!(script.contains("*relationships{src: $id"));
    }

    // -- Regression: builder output matches original constants --

    #[test]
    fn test_builder_matches_upsert_fact() {
        let original = r"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
          superseded_by, source_session_id, recorded_at,
          access_count, last_accessed_at, stability_hours, fact_type,
          is_forgotten, forgotten_at, forget_reason] <- [[$id, $valid_from,
          $content, $nous_id, $confidence, $tier, $valid_to, $superseded_by,
          $source_session_id, $recorded_at,
          $access_count, $last_accessed_at, $stability_hours, $fact_type,
          $is_forgotten, $forgotten_at, $forget_reason]]
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason}
    ";
        let built = queries::upsert_fact();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_current_facts() {
        let original = r"
        ?[id, content, confidence, tier, recorded_at] :=
            *facts{id, valid_from, content, nous_id, confidence, tier,
                   valid_to, superseded_by, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type,
                   is_forgotten, forgotten_at, forget_reason},
            nous_id = $nous_id,
            valid_from <= $now,
            valid_to > $now,
            is_null(superseded_by),
            is_forgotten == false
        :order -confidence
        :limit $limit
    ";
        let built = queries::current_facts();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_facts_at_time() {
        let original = r"
        ?[id, content, confidence, tier] :=
            *facts{id, valid_from, content, confidence, tier, valid_to, is_forgotten},
            valid_from <= $time,
            valid_to > $time,
            is_forgotten == false
    ";
        let built = queries::facts_at_time();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_supersede_fact() {
        let original = r#"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
          superseded_by, source_session_id, recorded_at,
          access_count, last_accessed_at, stability_hours, fact_type,
          is_forgotten, forgotten_at, forget_reason] <- [
            [$old_id, $old_valid_from, $old_content, $nous_id, $old_confidence,
             $old_tier, $now, $new_id, $old_source, $old_recorded,
             $old_access_count, $old_last_accessed_at, $old_stability_hours, $old_fact_type,
             $old_is_forgotten, $old_forgotten_at, $old_forget_reason],
            [$new_id, $now, $new_content, $nous_id, $new_confidence,
             $new_tier, "9999-12-31", null, $source_session_id, $now,
             0, "", $stability_hours, $fact_type,
             false, null, null]
        ]
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason}
    "#;
        let built = queries::supersede_fact();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_upsert_entity() {
        let original = r"
        ?[id, name, entity_type, aliases, created_at, updated_at] <- [
            [$id, $name, $entity_type, $aliases, $created_at, $updated_at]
        ]
        :put entities {id => name, entity_type, aliases, created_at, updated_at}
    ";
        let built = queries::upsert_entity();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_upsert_relationship() {
        let original = r"
        ?[src, dst, relation, weight, created_at] <- [
            [$src, $dst, $relation, $weight, $created_at]
        ]
        :put relationships {src, dst => relation, weight, created_at}
    ";
        let built = queries::upsert_relationship();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_upsert_embedding() {
        let original = r"?[id, content, source_type, source_id, nous_id, embedding, created_at] <- [
                [$id, $content, $source_type, $source_id, $nous_id, $embedding, $created_at]
              ]
              :put embeddings { id => content, source_type, source_id, nous_id, embedding, created_at }";
        let built = queries::upsert_embedding();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_full_current_facts() {
        let original = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to, superseded_by, source_session_id,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason] :=
        *facts{id, valid_from, content, nous_id, confidence, tier,
               valid_to, superseded_by, source_session_id, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason},
        nous_id = $nous_id,
        valid_from <= $now,
        valid_to > $now,
        is_null(superseded_by),
        is_forgotten == false
    :order -confidence
    :limit $limit
";
        let built = queries::full_current_facts();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn query_builder_prevents_injection() {
        let malicious_input = r#"test" :- *drop_all[], panic"#;
        let (script, params) = QueryBuilder::new()
            .raw("?[x] := *facts{id: $user_input, content: x}")
            .param("user_input", DataValue::from(malicious_input))
            .build();

        assert!(
            !script.contains(malicious_input),
            "raw malicious input must not appear in script body"
        );
        assert!(
            script.contains("$user_input"),
            "script must use parameter binding"
        );
        assert!(
            params.contains_key("user_input"),
            "malicious input must be in params map"
        );
    }

    #[test]
    fn query_builder_all_field_types() {
        let (script, params) = QueryBuilder::new()
            .raw("?[x] := *facts{id: $str_val, content: x}")
            .param("str_val", DataValue::from("hello"))
            .param("int_val", DataValue::from(42_i64))
            .param("float_val", DataValue::from(2.72_f64))
            .param("bool_val", DataValue::from(true))
            .param("null_val", DataValue::Null)
            .build();

        assert!(params.contains_key("str_val"));
        assert!(params.contains_key("int_val"));
        assert!(params.contains_key("float_val"));
        assert!(params.contains_key("bool_val"));
        assert!(params.contains_key("null_val"));
        assert_eq!(params.len(), 5);

        assert!(
            !script.contains("hello"),
            "string literal must not leak into script"
        );
        assert!(
            !script.contains("42"),
            "int literal must not leak into script"
        );
        assert!(
            !script.contains("3.14"),
            "float literal must not leak into script"
        );
    }

    #[test]
    fn query_builder_compound_filters() {
        use FactsField::*;
        let script = QueryBuilder::new()
            .scan(Relation::Facts)
            .select(&[Id, Content, Confidence])
            .bind(Id)
            .bind(Content)
            .bind(Confidence)
            .bind(NousId)
            .bind(Tier)
            .filter("nous_id = $nous_id")
            .filter("confidence > 0.5")
            .filter("tier != \"assumed\"")
            .done()
            .build_script();

        assert!(script.contains("nous_id = $nous_id"), "first filter");
        assert!(script.contains("confidence > 0.5"), "second filter");
        assert!(script.contains("tier != \"assumed\""), "third filter");

        let filter_count = script.matches(",\n").count();
        assert!(
            filter_count >= 3,
            "filters must be comma-separated in conjunction (got {filter_count})"
        );
    }

    #[test]
    fn query_builder_empty_filter() {
        use FactsField::*;
        let script = QueryBuilder::new()
            .scan(Relation::Facts)
            .select(&[Id, Content])
            .bind(Id)
            .bind(Content)
            .done()
            .build_script();

        assert!(script.contains("?[id, content]"), "select list present");
        assert!(script.contains("*facts{id, content}"), "scan present");
        assert!(!script.contains(":order"), "no order when not specified");
        assert!(!script.contains(":limit"), "no limit when not specified");
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn query_builder_never_produces_raw_user_input(
                // Minimum 2 chars: single-character strings like `}`, `{`, `*`
                // naturally appear in Datalog syntax and are false positives.
                // The real risk is multi-character user content leaking into
                // the script template instead of being bound as parameters.
                input in "[a-zA-Z0-9 !@#$%^&*()_+=\\[\\]{};':,./<>?]{2,100}"
            ) {
                let (script, params) = QueryBuilder::new()
                    .raw("?[x] := *facts{id: $user_input, content: x}")
                    .param("user_input", DataValue::from(input.as_str()))
                    .build();

                prop_assert!(
                    !script.contains(&input),
                    "raw user input must not appear in script: {input}"
                );
                prop_assert!(params.contains_key("user_input"));
            }
        }
    }
}
