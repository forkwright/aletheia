#![expect(clippy::expect_used, reason = "test setup failures should panic")]

#[cfg(feature = "storage-fjall")]
use std::io::Write;

use super::super::{KnowledgeConfig, KnowledgeStore, migration};
use crate::test_fixtures::make_fact;

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
    let store = make_store();
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

#[test]
fn v14_migration_backfills_existing_fact_sensitivity_to_public() {
    let store = make_store();
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
