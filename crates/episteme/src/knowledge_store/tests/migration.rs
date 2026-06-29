#![expect(clippy::expect_used, reason = "test setup failures should panic")]

#[cfg(feature = "storage-fjall")]
use std::io::Write;

use super::super::{KnowledgeConfig, KnowledgeStore, migration};
use crate::test_fixtures::{make_entity, make_fact};

#[cfg(feature = "storage-fjall")]
#[test]
fn open_fjall_copies_legacy_root_into_shared_cohort() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy_root = dir.path().join("knowledge.fjall");
    let legacy_partition = legacy_root.join("relations");
    std::fs::create_dir_all(&legacy_partition).expect("create legacy partition");
    let mut marker = std::fs::File::create(legacy_partition.join("marker")).expect("create marker");
    marker.write_all(b"legacy").expect("write marker");

    let shared = legacy_root.join("shared");
    KnowledgeStore::migrate_to_cohort_layout(&shared).expect("migrate shared cohort");

    let migrated_marker = shared.join("relations").join("marker");
    assert_eq!(
        std::fs::metadata(&migrated_marker)
            .expect("stat migrated marker")
            .len(),
        6
    );
}

#[test]
fn migration_registry_is_sequential_and_current() {
    for (offset, step) in migration::MIGRATIONS.iter().enumerate() {
        let expected = i64::try_from(offset).expect("offset fits i64") + 2;
        assert_eq!(
            step.target_version, expected,
            "migration registry must allocate versions sequentially"
        );
    }
    assert_eq!(
        migration::MIGRATIONS
            .last()
            .expect("migration registry is nonempty")
            .target_version,
        KnowledgeStore::SCHEMA_VERSION,
        "latest migration target should match schema version"
    );
}

#[test]
fn missing_schema_version_row_fails_closed_without_remigrating() {
    let store = make_store();
    let fact = make_fact("f1", "alice", "schema integrity preserves facts");
    store.insert_fact(&fact).expect("insert fact");

    store
        .run_mut_query(
            r#"?[key] <- [["schema"]] :rm schema_version {key}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("remove schema row");

    let err = store
        .init_schema()
        .expect_err("missing schema row should fail closed");
    let msg = err.to_string();
    assert!(
        msg.contains("schema_version relation is present but row 'schema' is missing"),
        "error should name missing schema row, got: {msg}"
    );

    let facts = store
        .query_facts("alice", "2026-06-01", 10)
        .expect("query facts after failed init");
    assert_eq!(facts.len(), 1, "failed init must not drop facts");
}

#[test]
fn downgrade_is_detected_before_migration() {
    let store = make_store();
    store
        .stamp_schema_version(KnowledgeStore::SCHEMA_VERSION + 1, "test")
        .expect("stamp future version");

    let err = store
        .init_schema()
        .expect_err("newer store should fail closed");
    assert!(
        matches!(
            err,
            crate::error::Error::SchemaVersion {
                expected: KnowledgeStore::SCHEMA_VERSION,
                found,
                ..
            } if found == KnowledgeStore::SCHEMA_VERSION + 1
        ),
        "expected schema version mismatch, got: {err}"
    );
}

#[test]
fn missing_intermediate_stamp_is_detected_as_hole() {
    let store = make_store();
    store
        .run_mut_query(
            r#"?[key] <- [["migration:12"]] :rm schema_version {key}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("remove migration stamp");

    let err = store
        .init_schema()
        .expect_err("missing migration stamp should fail closed");
    let msg = err.to_string();
    assert!(
        msg.contains("schema version integrity hole"),
        "error should name integrity hole, got: {msg}"
    );
    assert!(
        msg.contains("version 12"),
        "error should name missing version, got: {msg}"
    );
}

#[test]
fn crash_mid_sequence_resume_applies_only_missing_tail() {
    let store = make_store_allowing_assumed_meta();
    store
        .run_mut_query(
            r#"?[key] <- [["migration:13"]] :rm schema_version {key}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("remove v13 stamp");
    store
        .run_mut_query(
            r#"?[key] <- [["migration:14"]] :rm schema_version {key}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("remove v14 stamp");
    store
        .stamp_schema_version(12, "test")
        .expect("stamp partial migration state");

    store
        .init_schema()
        .expect("partial migration sequence should resume");

    assert_eq!(
        store.schema_version().expect("schema version"),
        KnowledgeStore::SCHEMA_VERSION
    );
    assert_eq!(
        store
            .migration_stamp_version(13)
            .expect("read v13 stamp")
            .expect("v13 stamp present"),
        13
    );
    assert_eq!(
        store
            .migration_stamp_version(14)
            .expect("read v14 stamp")
            .expect("v14 stamp present"),
        14
    );
}

#[test]
fn rerun_current_schema_is_noop() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: 4,
        ..Default::default()
    })
    .expect("open store");
    let before = store.schema_version().expect("schema version before");

    store.init_schema().expect("re-run init schema");

    assert_eq!(
        store.schema_version().expect("schema version after"),
        before
    );
    for step in migration::MIGRATIONS {
        assert_eq!(
            store
                .migration_stamp_version(step.target_version)
                .expect("read migration stamp"),
            Some(step.target_version),
            "stamp should remain present"
        );
    }
}

fn make_store() -> std::sync::Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: 4,
        ..Default::default()
    })
    .expect("open in-memory knowledge store")
}

fn make_store_allowing_assumed_meta() -> std::sync::Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: 4,
        allow_assumed_embedding_meta: true,
        ..Default::default()
    })
    .expect("open in-memory knowledge store")
}

fn mock_config(model: &str) -> KnowledgeConfig {
    KnowledgeConfig {
        dim: 4,
        embedding_model: model.to_owned(),
        ..Default::default()
    }
}

#[test]
fn fresh_create_writes_embedding_meta() {
    let store = KnowledgeStore::open_mem_with_config(mock_config("mock-embedding"))
        .expect("open in-memory knowledge store");

    let meta = store.embedding_meta().expect("read embedding metadata");

    assert_eq!(meta.model, "mock-embedding");
    assert_eq!(meta.dim, 4);
}

#[test]
fn v14_migration_backfills_existing_fact_sensitivity_to_public() {
    let store = make_store_allowing_assumed_meta();
    store
        .run_mut_query(
            "::fts drop facts:content_fts",
            std::collections::BTreeMap::new(),
        )
        .expect("drop facts FTS index");
    store
        .run_mut_query("::remove facts", std::collections::BTreeMap::new())
        .expect("remove current facts relation");
    store
        .run_mut_query(V13_FACTS_DDL, std::collections::BTreeMap::new())
        .expect("create v13 facts relation");
    store
        .run_mut_query(INSERT_V13_FACT, std::collections::BTreeMap::new())
        .expect("insert v13 fact");
    store
        .stamp_schema_version(13, "test")
        .expect("stamp v13 schema");

    store.init_schema().expect("apply v14 migration");

    let facts = store.read_facts_by_id("f-v13").expect("read migrated fact");
    assert_eq!(facts.len(), 1, "migration should preserve the fact row");
    let fact = facts.first().expect("migrated fact present");
    assert_eq!(
        fact.sensitivity,
        crate::knowledge::FactSensitivity::Public,
        "v14 migration must explicitly backfill the documented default"
    );
    assert_eq!(
        store.schema_version().expect("schema version"),
        KnowledgeStore::SCHEMA_VERSION
    );
}

#[test]
fn v15_migration_backfills_embedding_meta_as_assumed() {
    let store = make_store_allowing_assumed_meta();
    store
        .run_mut_query("::remove embedding_meta", std::collections::BTreeMap::new())
        .expect("remove embedding metadata relation");
    store
        .run_mut_query(
            r#"?[key] <- [["migration:15"]] :rm schema_version {key}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("remove v15 stamp");
    store
        .stamp_schema_version(14, "test")
        .expect("stamp v14 schema");

    store.init_schema().expect("apply v15 migration");

    let meta = store.embedding_meta().expect("read embedding metadata");
    assert_eq!(meta.model, KnowledgeStore::ASSUMED_EMBEDDING_MODEL);
    assert_eq!(meta.dim, 4);
}

#[test]
fn v18_migration_backfills_fact_entities_from_content() {
    let store = make_store();

    // Two entities whose ids are slug tokens of the fact content, plus one
    // whose id does not appear in the content and must not be linked.
    store
        .insert_entity(&make_entity("alice", "Alice", "person"))
        .expect("insert alice");
    store
        .insert_entity(&make_entity("rust", "Rust", "tool"))
        .expect("insert rust");
    store
        .insert_entity(&make_entity("postgres", "Postgres", "tool"))
        .expect("insert postgres");

    // insert_fact does not create fact_entities edges — that link is made by
    // extraction (#4675) or, for pre-existing rows, by this backfill.
    let fact = make_fact("f-backfill", "alice", "alice prefers rust");
    store.insert_fact(&fact).expect("insert fact");

    let before = store
        .list_entities_for_facts(std::slice::from_ref(&fact.id))
        .expect("list before backfill");
    assert!(
        before.is_empty(),
        "fact starts with no entity edges before the backfill"
    );

    store
        .migrate_v17_to_v18()
        .expect("v17->v18 backfill should succeed");

    let after = store
        .list_entities_for_facts(&[fact.id])
        .expect("list after backfill");
    let mut names: Vec<&str> = after.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["Alice", "Rust"],
        "backfill links only entities whose id appears as a content token; \
         'Postgres' is absent from the content and must not be linked"
    );
}

#[test]
fn v20_migration_scopes_existing_pending_merges_by_common_nous() {
    const V19_PENDING_MERGES_DDL: &str = r":create pending_merges {
        entity_a: String, entity_b: String =>
        name_a: String,
        name_b: String,
        name_similarity: Float,
        embed_similarity: Float,
        type_match: Bool,
        alias_overlap: Bool,
        merge_score: Float,
        created_at: String
    }";
    const V19_MERGE_AUDIT_DDL: &str = r":create merge_audit {
        canonical_id: String, merged_id: String =>
        merged_name: String,
        merge_score: Float,
        facts_transferred: Int,
        relationships_redirected: Int,
        merged_at: String
    }";

    let store = make_store();
    let entity_a = make_entity("migration-a", "Migration A", "topic");
    let entity_b = make_entity("migration-b", "migration a", "topic");
    store.insert_entity(&entity_a).expect("insert entity a");
    store.insert_entity(&entity_b).expect("insert entity b");
    let fact_a = make_fact("migration-fact-a", "alice", "migration fact a");
    let fact_b = make_fact("migration-fact-b", "alice", "migration fact b");
    store.insert_fact(&fact_a).expect("insert fact a");
    store.insert_fact(&fact_b).expect("insert fact b");
    store
        .insert_fact_entity(&fact_a.id, &entity_a.id)
        .expect("link entity a");
    store
        .insert_fact_entity(&fact_b.id, &entity_b.id)
        .expect("link entity b");

    store
        .run_mut_query("::remove pending_merges", std::collections::BTreeMap::new())
        .expect("remove v20 pending_merges");
    store
        .run_mut_query("::remove merge_audit", std::collections::BTreeMap::new())
        .expect("remove v20 merge_audit");
    store
        .run_mut_query(V19_PENDING_MERGES_DDL, std::collections::BTreeMap::new())
        .expect("create v19 pending_merges");
    store
        .run_mut_query(V19_MERGE_AUDIT_DDL, std::collections::BTreeMap::new())
        .expect("create v19 merge_audit");
    store
        .run_mut_query(
            r#"?[entity_a, entity_b, name_a, name_b, name_similarity, embed_similarity,
                type_match, alias_overlap, merge_score, created_at] <- [[
                    "migration-a", "migration-b", "Migration A", "migration a",
                    1.0, 0.0, true, false, 0.70, "2026-06-01T00:00:00Z"
                ]]
              :put pending_merges {
                  entity_a, entity_b => name_a, name_b, name_similarity,
                  embed_similarity, type_match, alias_overlap, merge_score, created_at
              }"#,
            std::collections::BTreeMap::new(),
        )
        .expect("insert v19 pending merge");
    store
        .run_mut_query(
            r#"?[key] <- [["migration:20"]] :rm schema_version {key}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("remove v20 stamp");
    store
        .stamp_schema_version(19, "test")
        .expect("stamp v19 schema");

    store.init_schema().expect("apply v20 migration");

    assert_eq!(
        store
            .get_pending_merges("alice")
            .expect("alice pending after migration")
            .len(),
        1,
        "v19 row should migrate under its unique common nous"
    );
    assert!(
        store
            .get_pending_merges("bob")
            .expect("bob pending after migration")
            .is_empty(),
        "migrated row must not be visible to a foreign nous"
    );
    assert_eq!(
        store.schema_version().expect("schema version"),
        KnowledgeStore::SCHEMA_VERSION
    );
}

#[test]
fn reembed_all_updates_embedding_meta() {
    let store = KnowledgeStore::open_mem_with_config(mock_config("old-model"))
        .expect("open in-memory knowledge store");
    let fact = make_fact(
        "reembed-fact",
        "alice",
        "reembed updates embedding metadata",
    );
    store.insert_fact(&fact).expect("insert fact");
    let provider = crate::embedding::MockEmbeddingProvider::new(4);

    let written = store.reembed_all(&provider).expect("reembed facts");

    assert_eq!(written, 1);
    let meta = store.embedding_meta().expect("read embedding metadata");
    assert_eq!(meta.model, "mock-embedding");
    assert_eq!(meta.dim, 4);
}

#[cfg(feature = "storage-fjall")]
#[test]
fn open_fjall_detects_embedding_drift() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("knowledge");
    {
        let _store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
            .expect("open original store");
    }

    let Err(err) = KnowledgeStore::open_fjall(&path, mock_config("other-model")) else {
        panic!("embedding model drift should fail closed");
    };

    assert!(
        matches!(err, crate::error::Error::EmbeddingDrift { .. }),
        "expected embedding drift error, got: {err}"
    );
    assert!(
        err.to_string().contains("aletheia memory reembed"),
        "error should direct operator to reembed, got: {err}"
    );
}

#[cfg(feature = "storage-fjall")]
#[test]
fn open_fjall_passes_matching_embedding_meta() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("knowledge");
    {
        let _store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
            .expect("open original store");
    }

    KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
        .expect("matching embedding metadata should open");
}

const V13_FACTS_DDL: &str = r":create facts {
    id: String, valid_from: String =>
    content: String,
    nous_id: String,
    confidence: Float,
    tier: String,
    valid_to: String,
    superseded_by: String?,
    source_session_id: String?,
    recorded_at: String,
    access_count: Int,
    last_accessed_at: String,
    stability_hours: Float,
    fact_type: String,
    is_forgotten: Bool default false,
    forgotten_at: String?,
    forget_reason: String?,
    scope: String?,
    project_id: String?,
    visibility: String default 'private'
}";

const INSERT_V13_FACT: &str = r#"
?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by,
  source_session_id, recorded_at, access_count, last_accessed_at,
  stability_hours, fact_type, is_forgotten, forgotten_at, forget_reason,
  scope, project_id, visibility] <- [[
    "f-v13", "2026-01-01T00:00:00Z", "legacy fact", "alice", 0.8,
    "inferred", "9999-12-31", null, null, "2026-01-01T00:00:00Z",
    0, "", 720.0, "knowledge", false, null, null, null, null, "private"
]]
:put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type, is_forgotten,
            forgotten_at, forget_reason, scope, project_id, visibility}
"#;
