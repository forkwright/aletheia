//! Equivalence coverage for the staging+rename migration rewrite
//! (aletheia#5779). Each destructive site is exercised against a real
//! old-shape fixture (not a fresh store faking an old stamp) and asserted
//! against the exact backfill values the pre-rewrite code wrote by hand, so
//! this is a positive re-derivation of correctness, not merely a
//! self-consistency check against code that no longer exists.
#![expect(clippy::expect_used, reason = "test setup and assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions over known-shape rows"
)]

use std::collections::BTreeMap;

use crate::engine::DataValue;

use super::super::{KnowledgeConfig, KnowledgeStore};

fn make_store() -> std::sync::Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: 4,
        allow_assumed_embedding_meta: true,
        ..Default::default()
    })
    .expect("open in-memory knowledge store")
}

fn run(store: &KnowledgeStore, script: &str) {
    store
        .run_mut_query(script, BTreeMap::new())
        .unwrap_or_else(|e| panic!("script failed: {e}\n{script}"));
}

fn exported(store: &KnowledgeStore, relation: &str) -> crate::engine::NamedRows {
    store
        .db
        .export_relations(std::iter::once(relation))
        .expect("export")
        .remove(relation)
        .unwrap_or_else(|| panic!("relation '{relation}' present in export"))
}

fn col<'a>(headers: &[String], row: &'a [DataValue], name: &str) -> &'a DataValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("missing column '{name}' in {headers:?}"));
    row.get(idx)
        .unwrap_or_else(|| panic!("row shorter than headers at '{name}'"))
}

fn str_col<'a>(headers: &[String], row: &'a [DataValue], name: &str) -> &'a str {
    col(headers, row, name)
        .get_str()
        .unwrap_or_else(|| panic!("column '{name}' is not a string"))
}

fn is_null(headers: &[String], row: &[DataValue], name: &str) -> bool {
    matches!(col(headers, row, name), DataValue::Null)
}

fn indices_of(store: &KnowledgeStore, relation: &str) -> Vec<(String, String)> {
    store
        .run_script_read_only(&format!("::indices {relation}"), BTreeMap::new())
        .expect("list indices")
        .rows
        .into_iter()
        .map(|row| {
            let name = row[0]
                .get_str()
                .unwrap_or_else(|| panic!("index name"))
                .to_owned();
            let kind = row[1]
                .get_str()
                .unwrap_or_else(|| panic!("index kind"))
                .to_owned();
            (name, kind)
        })
        .collect()
}

// ---------------------------------------------------------------------
// facts
// ---------------------------------------------------------------------

const V1_FACTS_DDL: &str = r":create facts {
    id: String, valid_from: String =>
    content: String,
    nous_id: String,
    confidence: Float,
    tier: String,
    valid_to: String,
    superseded_by: String?,
    source_session_id: String?,
    recorded_at: String
}";

const INSERT_V1_FACT: &str = r#"
?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by,
  source_session_id, recorded_at] <- [[
    "f-v1", "2026-01-01T00:00:00Z", "v1 fact", "alice", 0.9, "verified",
    "9999-12-31", null, "sess-1", "2026-01-01T00:00:00Z"
]]
:put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at}
"#;

fn replace_facts_with(store: &KnowledgeStore, ddl: &str) {
    run(store, "::fts drop facts:content_fts");
    run(store, "::remove facts");
    run(store, ddl);
}

#[test]
fn v1_to_v2_backfills_new_columns_and_preserves_old_data() {
    let store = make_store();
    replace_facts_with(&store, V1_FACTS_DDL);
    run(&store, INSERT_V1_FACT);
    store.stamp_schema_version(1, "test").expect("stamp v1");

    store.migrate_v1_to_v2().expect("v1->v2 should succeed");

    let facts = exported(&store, "facts");
    assert_eq!(facts.rows.len(), 1, "seed row must survive the rebuild");
    let row = &facts.rows[0];
    assert_eq!(str_col(&facts.headers, row, "content"), "v1 fact");
    assert_eq!(str_col(&facts.headers, row, "source_session_id"), "sess-1");
    assert_eq!(
        col(&facts.headers, row, "access_count"),
        &DataValue::from(0_i64)
    );
    assert_eq!(str_col(&facts.headers, row, "last_accessed_at"), "");
    assert_eq!(
        col(&facts.headers, row, "stability_hours"),
        &DataValue::from(720.0_f64)
    );
    assert_eq!(str_col(&facts.headers, row, "fact_type"), "");
    assert!(
        is_null(&facts.headers, row, "forgotten_at"),
        "columns v1 never had must backfill null, not error"
    );
    assert!(is_null(&facts.headers, row, "scope"));
    assert!(is_null(&facts.headers, row, "project_id"));
    assert_eq!(
        store.schema_version().expect("schema version"),
        2,
        "target version must be stamped"
    );
    assert_eq!(
        indices_of(&store, "facts"),
        vec![("content_fts".to_owned(), "fts".to_owned())],
        "the FTS index must be recreated on the rebuilt relation"
    );
}

#[test]
fn v2_to_v3_backfills_forgotten_columns() {
    const V2_FACTS_DDL: &str = r":create facts {
        id: String, valid_from: String =>
        content: String, nous_id: String, confidence: Float, tier: String,
        valid_to: String, superseded_by: String?, source_session_id: String?,
        recorded_at: String, access_count: Int, last_accessed_at: String,
        stability_hours: Float, fact_type: String
    }";
    let store = make_store();
    replace_facts_with(&store, V2_FACTS_DDL);
    run(
        &store,
        r#"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
             superseded_by, source_session_id, recorded_at,
             access_count, last_accessed_at, stability_hours, fact_type] <- [[
            "f-v2", "2026-01-01T00:00:00Z", "v2 fact", "alice", 0.5, "inferred",
            "9999-12-31", null, null, "2026-01-01T00:00:00Z", 3, "2026-01-02T00:00:00Z",
            48.0, "preference"
        ]]
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type}"#,
    );
    store.stamp_schema_version(2, "test").expect("stamp v2");

    store.migrate_v2_to_v3().expect("v2->v3 should succeed");

    let facts = exported(&store, "facts");
    let row = &facts.rows[0];
    assert_eq!(
        col(&facts.headers, row, "access_count"),
        &DataValue::from(3_i64),
        "columns already present must carry over unchanged, not get reset to a default"
    );
    assert_eq!(
        col(&facts.headers, row, "is_forgotten"),
        &DataValue::Bool(false)
    );
    assert!(is_null(&facts.headers, row, "forgotten_at"));
    assert!(is_null(&facts.headers, row, "forget_reason"));
    assert_eq!(store.schema_version().expect("schema version"), 3);
}

#[test]
fn v10_to_v11_backfills_scope_project_id_visibility() {
    const V10_FACTS_DDL: &str = r":create facts {
        id: String, valid_from: String =>
        content: String, nous_id: String, confidence: Float, tier: String,
        valid_to: String, superseded_by: String?, source_session_id: String?,
        recorded_at: String, access_count: Int, last_accessed_at: String,
        stability_hours: Float, fact_type: String,
        is_forgotten: Bool default false, forgotten_at: String?, forget_reason: String?
    }";
    let store = make_store();
    replace_facts_with(&store, V10_FACTS_DDL);
    run(
        &store,
        r#"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
             superseded_by, source_session_id, recorded_at,
             access_count, last_accessed_at, stability_hours, fact_type,
             is_forgotten, forgotten_at, forget_reason] <- [[
            "f-v10", "2026-01-01T00:00:00Z", "v10 fact", "alice", 0.5, "inferred",
            "9999-12-31", null, null, "2026-01-01T00:00:00Z", 0, "", 720.0, "",
            false, null, null
        ]]
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type, is_forgotten,
            forgotten_at, forget_reason}"#,
    );
    store.stamp_schema_version(10, "test").expect("stamp v10");

    store.migrate_v10_to_v11().expect("v10->v11 should succeed");

    let facts = exported(&store, "facts");
    let row = &facts.rows[0];
    assert!(is_null(&facts.headers, row, "scope"));
    assert!(is_null(&facts.headers, row, "project_id"));
    assert_eq!(str_col(&facts.headers, row, "visibility"), "private");
    assert_eq!(store.schema_version().expect("schema version"), 11);
}

#[test]
fn v11_to_v12_backfills_project_id_and_preserves_scope_visibility() {
    const V11_FACTS_DDL: &str = r":create facts {
        id: String, valid_from: String =>
        content: String, nous_id: String, confidence: Float, tier: String,
        valid_to: String, superseded_by: String?, source_session_id: String?,
        recorded_at: String, access_count: Int, last_accessed_at: String,
        stability_hours: Float, fact_type: String,
        is_forgotten: Bool default false, forgotten_at: String?, forget_reason: String?,
        scope: String?, visibility: String default 'private'
    }";
    let store = make_store();
    replace_facts_with(&store, V11_FACTS_DDL);
    run(
        &store,
        r#"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
             superseded_by, source_session_id, recorded_at,
             access_count, last_accessed_at, stability_hours, fact_type,
             is_forgotten, forgotten_at, forget_reason, scope, visibility] <- [[
            "f-v11", "2026-01-01T00:00:00Z", "v11 fact", "alice", 0.5, "inferred",
            "9999-12-31", null, null, "2026-01-01T00:00:00Z", 0, "", 720.0, "",
            false, null, null, "team-scope", "protected"
        ]]
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type, is_forgotten,
            forgotten_at, forget_reason, scope, visibility}"#,
    );
    store.stamp_schema_version(11, "test").expect("stamp v11");

    store.migrate_v11_to_v12().expect("v11->v12 should succeed");

    let facts = exported(&store, "facts");
    let row = &facts.rows[0];
    assert!(is_null(&facts.headers, row, "project_id"));
    assert_eq!(
        str_col(&facts.headers, row, "scope"),
        "team-scope",
        "pre-existing columns must be preserved, not overwritten by a default"
    );
    assert_eq!(str_col(&facts.headers, row, "visibility"), "protected");
    assert_eq!(store.schema_version().expect("schema version"), 12);
}

#[test]
fn v13_to_v14_backfills_sensitivity_public() {
    const V13_FACTS_DDL: &str = r":create facts {
        id: String, valid_from: String =>
        content: String, nous_id: String, confidence: Float, tier: String,
        valid_to: String, superseded_by: String?, source_session_id: String?,
        recorded_at: String, access_count: Int, last_accessed_at: String,
        stability_hours: Float, fact_type: String,
        is_forgotten: Bool default false, forgotten_at: String?, forget_reason: String?,
        scope: String?, project_id: String?, visibility: String default 'private'
    }";
    let store = make_store();
    replace_facts_with(&store, V13_FACTS_DDL);
    run(
        &store,
        r#"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
             superseded_by, source_session_id, recorded_at,
             access_count, last_accessed_at, stability_hours, fact_type,
             is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility] <- [[
            "f-v13", "2026-01-01T00:00:00Z", "v13 fact", "alice", 0.5, "inferred",
            "9999-12-31", null, null, "2026-01-01T00:00:00Z", 0, "", 720.0, "",
            false, null, null, null, "proj-1", "private"
        ]]
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type, is_forgotten,
            forgotten_at, forget_reason, scope, project_id, visibility}"#,
    );
    store.stamp_schema_version(13, "test").expect("stamp v13");

    store.migrate_v13_to_v14().expect("v13->v14 should succeed");

    let facts = exported(&store, "facts");
    let row = &facts.rows[0];
    assert_eq!(str_col(&facts.headers, row, "sensitivity"), "public");
    assert_eq!(
        str_col(&facts.headers, row, "project_id"),
        "proj-1",
        "pre-existing project_id must survive"
    );
    assert_eq!(store.schema_version().expect("schema version"), 14);
}

// ---------------------------------------------------------------------
// causal_edges — v6->v7 fixes a real omission bug (aletheia#5779 audit
// finding): the pre-rewrite `:put` never bound `id`, a non-nullable
// no-default column on the current `CAUSAL_EDGES_DDL` shape it was already
// recreating against. `make_extractor`
// (`krites/src/runtime/relation/extractors.rs:84-101`) errors on a missing
// binding with no `default_gen`, so that path would have failed against any
// real non-empty `causal_edges` relation — untested until now (no prior
// coverage existed for either causal_edges migration with real data).
// ---------------------------------------------------------------------

fn replace_causal_edges_with(store: &KnowledgeStore, ddl: &str) {
    run(store, "::remove causal_edges");
    run(store, ddl);
}

#[test]
fn v6_to_v7_mints_id_and_sets_relationship_type_caused() {
    const V6_CAUSAL_EDGES_DDL: &str = r":create causal_edges {
        cause: String, effect: String =>
        ordering: String, confidence: Float, created_at: String
    }";
    let store = make_store();
    replace_causal_edges_with(&store, V6_CAUSAL_EDGES_DDL);
    run(
        &store,
        r#"?[cause, effect, ordering, confidence, created_at] <- [[
            "c1", "e1", "before", 0.7, "2026-01-01T00:00:00Z"
        ]]
        :put causal_edges {cause, effect => ordering, confidence, created_at}"#,
    );
    store.stamp_schema_version(6, "test").expect("stamp v6");

    store
        .migrate_v6_to_v7()
        .expect("v6->v7 must succeed against a real non-empty causal_edges relation");

    let edges = exported(&store, "causal_edges");
    assert_eq!(edges.rows.len(), 1);
    let row = &edges.rows[0];
    assert_eq!(str_col(&edges.headers, row, "relationship_type"), "caused");
    assert!(
        !str_col(&edges.headers, row, "id").is_empty(),
        "id must be minted, not left unbound"
    );
    assert!(is_null(&edges.headers, row, "evidence_session_id"));
    assert_eq!(store.schema_version().expect("schema version"), 7);
}

#[test]
fn v16_to_v17_mints_unique_ulid_per_row_masked_equivalence() {
    const V16_CAUSAL_EDGES_DDL: &str = r":create causal_edges {
        cause: String, effect: String =>
        ordering: String, relationship_type: String, confidence: Float, created_at: String
    }";
    let store = make_store();
    replace_causal_edges_with(&store, V16_CAUSAL_EDGES_DDL);
    run(
        &store,
        r#"?[cause, effect, ordering, relationship_type, confidence, created_at] <- [
            ["c1", "e1", "before", "enables", 0.7, "2026-01-01T00:00:00Z"],
            ["c2", "e2", "after", "blocks", 0.4, "2026-01-02T00:00:00Z"]
        ]
        :put causal_edges {cause, effect => ordering, relationship_type, confidence, created_at}"#,
    );
    store.stamp_schema_version(16, "test").expect("stamp v16");

    store.migrate_v16_to_v17().expect("v16->v17 should succeed");

    let edges = exported(&store, "causal_edges");
    assert_eq!(edges.rows.len(), 2);
    // Masked-column canonicalization (plan §8.4): cardinality equality +
    // byte-identity of the deterministic remainder + uniqueness of the
    // minted column, rather than a global byte-identity check.
    let mut ids = std::collections::BTreeSet::new();
    for row in &edges.rows {
        let id = str_col(&edges.headers, row, "id");
        assert!(!id.is_empty(), "every row must get a minted id");
        assert!(
            ids.insert(id.to_owned()),
            "minted ids must be unique per row: {id}"
        );
        assert!(is_null(&edges.headers, row, "evidence_session_id"));
    }
    let cause1 = edges
        .rows
        .iter()
        .find(|r| str_col(&edges.headers, r, "cause") == "c1")
        .expect("row for c1");
    assert_eq!(
        str_col(&edges.headers, cause1, "relationship_type"),
        "enables"
    );
    assert_eq!(store.schema_version().expect("schema version"), 17);
}

// ---------------------------------------------------------------------
// entities
// ---------------------------------------------------------------------

#[test]
fn v12_to_v13_backfills_name_embedding_null() {
    let store = make_store();
    run(&store, "::remove entities");
    run(
        &store,
        r":create entities {
            id: String =>
            name: String, entity_type: String, aliases: String,
            created_at: String, updated_at: String
        }",
    );
    run(
        &store,
        r#"?[id, name, entity_type, aliases, created_at, updated_at] <- [[
            "alice", "Alice", "person", "", "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z"
        ]]
        :put entities {id => name, entity_type, aliases, created_at, updated_at}"#,
    );
    store.stamp_schema_version(12, "test").expect("stamp v12");

    store.migrate_v12_to_v13().expect("v12->v13 should succeed");

    let entities = exported(&store, "entities");
    assert_eq!(entities.rows.len(), 1);
    let row = &entities.rows[0];
    assert_eq!(str_col(&entities.headers, row, "name"), "Alice");
    assert!(is_null(&entities.headers, row, "name_embedding"));
    assert_eq!(store.schema_version().expect("schema version"), 13);
}

// ---------------------------------------------------------------------
// Full climb: on-disk format delta must be none (plan §8, acceptance §8.6)
// ---------------------------------------------------------------------

/// Relations no migration function ever creates — present in a store at
/// every schema version, including v1, because `init_schema`'s fresh-store
/// branch is the only place that creates them. Everything else `::relations`
/// enumerates was added by some migration (v3->v4 onward, several
/// unconditionally), so it's both safe and correct to strip for a faked-v1
/// fixture: a genuine v1-era store never had it either.
const FOUNDATIONAL_RELATIONS: &[&str] = &["facts", "entities", "relationships", "embeddings"];

/// Remove every relation except [`FOUNDATIONAL_RELATIONS`] and
/// `schema_version`. `make_store()` is a *fresh* store, which creates
/// everything at the current shape up front; without this, climbing from a
/// faked-v1 stamp hits "relation already exists" on the first
/// unconditional-create migration (v3->v4 onward) — a test-fixture
/// artifact, not a production scenario (a real store climbing from v1 never
/// had these relations to collide with). Enumerates live relations via
/// `::relations` rather than hand-listing DDL constants, so this can't
/// silently drift as the schema grows new relations — only
/// `FOUNDATIONAL_RELATIONS` needs to stay accurate, and it's the same set
/// for every schema version by construction.
fn strip_to_v1_shape(store: &KnowledgeStore) {
    let relations = store
        .run_script_read_only("::relations", BTreeMap::new())
        .expect("list relations");
    for i in 0..relations.row_count() {
        let name = relations.get_string(i, "name").expect("relation name");
        if name == "schema_version"
            || FOUNDATIONAL_RELATIONS.contains(&name.as_str())
            || name.contains(':')
        {
            continue; // index sub-relations are cleaned up via their owner below
        }
        let indices = indices_of(store, &name);
        for (idx_name, idx_kind) in indices {
            let verb = match idx_kind.as_str() {
                "normal" => "::index drop",
                "hnsw" => "::hnsw drop",
                "fts" => "::fts drop",
                "lsh" => "::lsh drop",
                other => panic!("unknown index kind '{other}' on '{name}'"),
            };
            run(store, &format!("{verb} {name}:{idx_name}"));
        }
        run(store, &format!("::remove {name}"));
    }
    let _ = store.run_mut_query("::fts drop facts:content_fts", BTreeMap::new());
}

/// aletheia#5779 F9: this proves the full v1->v19 climb converges to the
/// fresh-store shape, but it does **not** prove every one of the 8
/// destructive `rebuild_relation_atomically` sites runs its real transform
/// on this (or any) climb. `facts`'s five sites all stage the TERMINAL
/// `FACTS_DDL` as `live_ddl` (not each site's own historical shape,
/// pre-dating this rewrite — `migration_old.rs`'s `migrate_v1_to_v2`
/// already did the same), so `migrate_v1_to_v2`'s backfill jumps `facts`
/// straight to its final column set; every later facts site's
/// `column_probe` then finds `expected == live` immediately and resumes at
/// `Rebuilt`, running only index recreation + the stamp — verified by the
/// column/index/version/content assertions below, which is exactly what
/// makes them insufficient to prove those later sites' `transform`
/// closures ever execute for real. Structurally the same is true of
/// `causal_edges`: `migrate_v5_to_v6` creates it fresh with the same
/// terminal `CAUSAL_EDGES_DDL` that `migrate_v6_to_v7`'s `live_ddl` also
/// names, so v6->v7's `column_probe` short-circuits too — on a genuine
/// historical v1 store, not just this fixture, because `causal_edges` never
/// existed before v5->v6 created it at the terminal shape. `entities` is
/// `FOUNDATIONAL_RELATIONS` (created at the terminal `entities_ddl(dim)`
/// shape by `make_store`'s own fresh-store bootstrap, never downgraded by
/// `strip_to_v1_shape`), so `migrate_v12_to_v13`'s `column_probe` also
/// short-circuits in this fixture. On a genuine full v1 climb, only
/// `migrate_v1_to_v2` is provably exercising steps 2-7 for real; the other
/// seven sites are exercised for real only by the per-site fixture tests
/// above (each of which forces `CleanStart` directly) and by this module's
/// own equivalence checks against pre-rewrite output — never simultaneously
/// with each other on one climb. Do not read a passing run of this test as
/// proof that all 8 sites' transforms executed.
#[test]
fn full_climb_from_v1_matches_fresh_store_schema() {
    let climbed = make_store();
    replace_facts_with(&climbed, V1_FACTS_DDL);
    run(&climbed, INSERT_V1_FACT);
    strip_to_v1_shape(&climbed);
    climbed.stamp_schema_version(1, "test").expect("stamp v1");

    climbed
        .init_schema()
        .expect("climbing from v1 through every migration must succeed");

    let fresh = make_store();

    // WHY: extends the facts-only column/index parity check below to the
    // other two relations `rebuild_relation_atomically` sites touch
    // (`causal_edges` via v6->v7/v16->v17, `entities` via v12->v13) — a
    // gross shape regression on either (e.g. a dropped column, a missing
    // index) would otherwise be invisible to this test even though it
    // exercises both relations' migration sites on the climb.
    for relation in ["causal_edges", "entities"] {
        let mut climbed_cols = climbed
            .run_script_read_only(&format!("::columns {relation}"), BTreeMap::new())
            .unwrap_or_else(|e| panic!("climbed columns for '{relation}': {e}"))
            .rows
            .into_iter()
            .map(|row| row[0].get_str().expect("column name").to_owned())
            .collect::<Vec<_>>();
        let mut fresh_cols = fresh
            .run_script_read_only(&format!("::columns {relation}"), BTreeMap::new())
            .unwrap_or_else(|e| panic!("fresh columns for '{relation}': {e}"))
            .rows
            .into_iter()
            .map(|row| row[0].get_str().expect("column name").to_owned())
            .collect::<Vec<_>>();
        climbed_cols.sort_unstable();
        fresh_cols.sort_unstable();
        assert_eq!(
            climbed_cols, fresh_cols,
            "'{relation}': a store climbed from v1 must reach the identical column set a fresh store gets"
        );
        assert_eq!(
            indices_of(&climbed, relation),
            indices_of(&fresh, relation),
            "'{relation}': index set must match too"
        );
    }

    let mut climbed_facts_cols = climbed
        .run_script_read_only("::columns facts", BTreeMap::new())
        .expect("climbed columns")
        .rows
        .into_iter()
        .map(|row| row[0].get_str().expect("column name").to_owned())
        .collect::<Vec<_>>();
    let mut fresh_facts_cols = fresh
        .run_script_read_only("::columns facts", BTreeMap::new())
        .expect("fresh columns")
        .rows
        .into_iter()
        .map(|row| row[0].get_str().expect("column name").to_owned())
        .collect::<Vec<_>>();
    climbed_facts_cols.sort_unstable();
    fresh_facts_cols.sort_unstable();
    assert_eq!(
        climbed_facts_cols, fresh_facts_cols,
        "a store climbed from v1 must reach the identical facts column set a fresh store gets"
    );

    assert_eq!(
        indices_of(&climbed, "facts"),
        indices_of(&fresh, "facts"),
        "index set must match too — a snapshot column diff alone hides a missing FTS index"
    );

    assert_eq!(
        climbed.schema_version().expect("climbed schema version"),
        KnowledgeStore::SCHEMA_VERSION
    );

    let facts = exported(&climbed, "facts");
    assert_eq!(
        facts.rows.len(),
        1,
        "the v1 seed row must survive the full climb"
    );
    let row = &facts.rows[0];
    assert_eq!(str_col(&facts.headers, row, "content"), "v1 fact");
    assert_eq!(str_col(&facts.headers, row, "sensitivity"), "public");
}
