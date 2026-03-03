//! Typed Datalog query builder — compile-time schema validation for KnowledgeStore.

use std::collections::BTreeMap;

use aletheia_mneme_engine::DataValue;

/// Datalog field reference. Implemented by per-relation field enums.
pub(crate) trait Field: Copy {
    fn name(self) -> &'static str;
}

/// Knowledge graph relations — the top-level CozoDB stored relations.
///
/// Used with [`QueryBuilder`] to name the relation in `:put` and `*relation{...}`
/// expressions. Each variant maps to a CozoDB relation defined in
/// [`crate::schema::DDL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum Relation {
    /// Bi-temporal fact store.
    Facts,
    /// Named entity graph nodes.
    Entities,
    /// Typed edges between entities.
    Relationships,
    /// HNSW vector index for semantic recall.
    Embeddings,
}

impl Relation {
    /// Returns the CozoDB relation name used in Datalog scripts.
    ///
    /// For example, `Relation::Facts.name()` returns `"facts"` — the name
    /// used in `*facts{...}` pattern clauses.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Facts => "facts",
            Self::Entities => "entities",
            Self::Relationships => "relationships",
            Self::Embeddings => "embeddings",
        }
    }
}

/// Fields in the `facts` CozoDB relation.
///
/// Implements [`Field`] — use with [`QueryBuilder::scan`] / [`PutBuilder::keys`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum FactsField {
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
        }
    }
}

/// Fields in the `entities` CozoDB relation.
///
/// Implements [`Field`] — use with [`QueryBuilder::scan`] / [`PutBuilder::keys`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum EntitiesField {
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

/// Fields in the `relationships` CozoDB relation.
///
/// Implements [`Field`] — use with [`QueryBuilder::scan`] / [`PutBuilder::keys`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum RelationshipsField {
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

/// Fields in the `embeddings` CozoDB relation.
///
/// Implements [`Field`] — use with [`QueryBuilder::scan`] / [`PutBuilder::keys`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum EmbeddingsField {
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
pub(crate) struct QueryBuilder {
    lines: Vec<String>,
    params: BTreeMap<String, DataValue>,
}

impl QueryBuilder {
    /// Create an empty query builder.
    ///
    /// Call [`put`](QueryBuilder::put) to start an upsert operation or
    /// [`scan`](QueryBuilder::scan) to start a query. Use [`build`](QueryBuilder::build)
    /// or [`build_script`](QueryBuilder::build_script) to finalise.
    pub(crate) fn new() -> Self {
        Self {
            lines: Vec::new(),
            params: BTreeMap::new(),
        }
    }

    /// Start a `:put` operation against a relation.
    pub(crate) fn put(self, relation: Relation) -> PutBuilder {
        PutBuilder {
            parent: self,
            relation,
            all_fields: Vec::new(),
            key_count: 0,
            rows: Vec::new(),
        }
    }

    /// Start a `?[...] := *relation{...}` scan query.
    pub(crate) fn scan(self, relation: Relation) -> ScanBuilder {
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
    pub(crate) fn raw(mut self, line: &str) -> Self {
        self.lines.push(line.to_owned());
        self
    }

    /// Bind a named parameter.
    pub(crate) fn param(mut self, name: &str, value: DataValue) -> Self {
        self.params.insert(name.to_owned(), value);
        self
    }

    /// Consume the builder, producing `(script, params)`.
    pub(crate) fn build(self) -> (String, BTreeMap<String, DataValue>) {
        (self.lines.join("\n"), self.params)
    }

    /// Consume the builder, producing only the script string.
    pub(crate) fn build_script(self) -> String {
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
pub(crate) struct PutBuilder {
    parent: QueryBuilder,
    relation: Relation,
    all_fields: Vec<&'static str>,
    key_count: usize,
    rows: Vec<Vec<String>>,
}

impl PutBuilder {
    /// Declare key fields (before the `=>` in the `:put` clause).
    pub(crate) fn keys(mut self, fields: &[impl Field]) -> Self {
        self.key_count = fields.len();
        for f in fields {
            self.all_fields.push(f.name());
        }
        self
    }

    /// Declare value fields (after the `=>` in the `:put` clause).
    pub(crate) fn values(mut self, fields: &[impl Field]) -> Self {
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
    pub(crate) fn row(mut self, exprs: &[&str]) -> Self {
        self.rows.push(exprs.iter().map(|s| (*s).to_owned()).collect());
        self
    }

    /// Finish the `:put`, returning the parent `QueryBuilder`.
    ///
    /// If no explicit `row()` was called, generates a single row from field
    /// names (convention: `$field_name` for each field).
    pub(crate) fn done(mut self) -> QueryBuilder {
        if self.rows.is_empty() {
            let auto_row: Vec<String> = self
                .all_fields
                .iter()
                .map(|f| format!("${f}"))
                .collect();
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
            format!(":put {} {{{}}}", self.relation.name(), key_fields.join(", "))
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
pub(crate) struct ScanBuilder {
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
    pub(crate) fn select(mut self, fields: &[impl Field]) -> Self {
        self.select = fields.iter().map(|f| f.name()).collect();
        self
    }

    /// Bind a field in the `*relation{...}` clause (just the field name).
    pub(crate) fn bind(mut self, field: impl Field) -> Self {
        self.bindings.push(field.name().to_owned());
        self
    }

    /// Bind a field to an expression: `field: expr` in `*relation{...}`.
    pub(crate) fn bind_to(mut self, field: impl Field, expr: &str) -> Self {
        self.bindings.push(format!("{}: {expr}", field.name()));
        self
    }

    /// Add a filter condition (raw Datalog expression after the scan clause).
    pub(crate) fn filter(mut self, expr: &str) -> Self {
        self.filters.push(expr.to_owned());
        self
    }

    /// Set `:order` directive (e.g. `"-confidence"`).
    pub(crate) fn order(mut self, expr: &str) -> Self {
        self.order = Some(expr.to_owned());
        self
    }

    /// Set `:limit` directive (e.g. `"$limit"`).
    pub(crate) fn limit(mut self, expr: &str) -> Self {
        self.limit = Some(expr.to_owned());
        self
    }

    /// Finish the scan, returning the parent `QueryBuilder`.
    pub(crate) fn done(mut self) -> QueryBuilder {
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
            line.push_str(&format!("\n:order {ord}"));
        }
        if let Some(ref lim) = self.limit {
            line.push_str(&format!("\n:limit {lim}"));
        }

        self.parent.lines.push(line);
        self.parent
    }
}

// ---------------------------------------------------------------------------
// Pre-built query functions
// ---------------------------------------------------------------------------

/// Builder-generated query scripts for KnowledgeStore operations.
pub(crate) mod queries {
    use super::*;

    /// Insert or update a fact. Params: `$id`, `$valid_from`, `$content`,
    /// `$nous_id`, `$confidence`, `$tier`, `$valid_to`, `$superseded_by`,
    /// `$source_session_id`, `$recorded_at`.
    pub(crate) fn upsert_fact() -> String {
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
            ])
            .done()
            .build_script()
    }

    /// Query current facts for a nous (not superseded, currently valid).
    /// Params: `$nous_id`, `$now`, `$limit`.
    pub(crate) fn current_facts() -> String {
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
            .filter("nous_id = $nous_id")
            .filter("valid_from <= $now")
            .filter("valid_to > $now")
            .filter("is_null(superseded_by)")
            .order("-confidence")
            .limit("$limit")
            .done()
            .build_script()
    }

    /// Extended query returning all `Fact` fields.
    /// Params: `$nous_id`, `$now`, `$limit`.
    pub(crate) fn full_current_facts() -> String {
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
            .filter("nous_id = $nous_id")
            .filter("valid_from <= $now")
            .filter("valid_to > $now")
            .filter("is_null(superseded_by)")
            .order("-confidence")
            .limit("$limit")
            .done()
            .build_script()
    }

    /// Point-in-time fact query. Params: `$time`.
    pub(crate) fn facts_at_time() -> String {
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
            .filter("valid_from <= $time")
            .filter("valid_to > $time")
            .done()
            .build_script()
    }

    /// Supersede a fact (close old, insert new). Two rows in one `:put`.
    /// Params: `$old_id`, `$old_valid_from`, `$old_content`, `$nous_id`,
    /// `$old_confidence`, `$old_tier`, `$now`, `$new_id`, `$old_source`,
    /// `$old_recorded`, `$new_content`, `$new_confidence`, `$new_tier`,
    /// `$source_session_id`.
    pub(crate) fn supersede_fact() -> String {
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
            ])
            .done()
            .build_script()
    }

    /// Insert or update an entity.
    /// Params: `$id`, `$name`, `$entity_type`, `$aliases`, `$created_at`, `$updated_at`.
    pub(crate) fn upsert_entity() -> String {
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
    pub(crate) fn upsert_embedding() -> String {
        use EmbeddingsField::*;
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

    /// KNN vector search. Params: `$query_vec`, `$k`, `$ef`.
    pub(crate) const SEMANTIC_SEARCH: &str = r"
        ?[id, content, source_type, source_id, dist] :=
            ~embeddings:semantic_idx {id, content, source_type, source_id |
                query: $query_vec, k: $k, ef: $ef, bind_distance: dist}
    ";

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
    /// Graph sub-rules are injected dynamically by `build_hybrid_query`.
    /// Params: `$query_text`, `$query_vec`, `$k`, `$ef`, `$limit`.
    pub(crate) const HYBRID_SEARCH_BASE: &str = r"
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Normalize whitespace for comparison: collapse runs of whitespace to single
    /// space, trim, then remove spaces adjacent to brackets/braces (CozoDB
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
        assert!(!script.contains("42"), "script must not contain literal value");
        assert!(params.contains_key("val"));
    }

    #[test]
    fn test_injection_attempt() {
        // Param values with special chars go through $binding, not interpolation
        let (script, params) = QueryBuilder::new()
            .raw("?[x] := *facts{id: $id, content: x}")
            .param("id", DataValue::Str("evil}; :rm facts".into()))
            .build();

        assert!(!script.contains("evil}"), "injection payload must not appear in script");
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
        ]
        .iter()
        .map(|f| f.name())
        .collect();
        assert_eq!(facts_ddl_fields.as_slice(), facts_enum_fields.as_slice());

        // Entities DDL fields
        let entities_ddl = ["id", "name", "entity_type", "aliases", "created_at", "updated_at"];
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
          superseded_by, source_session_id, recorded_at] <- [[$id, $valid_from,
          $content, $nous_id, $confidence, $tier, $valid_to, $superseded_by,
          $source_session_id, $recorded_at]]
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at}
    ";
        let built = queries::upsert_fact();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_current_facts() {
        let original = r"
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
        let built = queries::current_facts();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_facts_at_time() {
        let original = r"
        ?[id, content, confidence, tier] :=
            *facts{id, valid_from, content, confidence, tier, valid_to},
            valid_from <= $time,
            valid_to > $time
    ";
        let built = queries::facts_at_time();
        assert_eq!(normalize(&built), normalize(original));
    }

    #[test]
    fn test_builder_matches_supersede_fact() {
        let original = r#"
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
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to, superseded_by, source_session_id] :=
        *facts{id, valid_from, content, nous_id, confidence, tier,
               valid_to, superseded_by, source_session_id, recorded_at},
        nous_id = $nous_id,
        valid_from <= $now,
        valid_to > $now,
        is_null(superseded_by)
    :order -confidence
    :limit $limit
";
        let built = queries::full_current_facts();
        assert_eq!(normalize(&built), normalize(original));
    }
}
