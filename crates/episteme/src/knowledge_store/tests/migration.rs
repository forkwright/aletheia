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

/// aletheia#7161: a store that migrated to its current version before
/// per-step stamping existed has no `migration:N` row for any of its
/// earliest steps -- not a hole, but the boundary where stamping began.
/// `init_schema` must backfill that contiguous prefix and open, while
/// `missing_intermediate_stamp_is_detected_as_hole` above proves a gap
/// *between* present stamps still fails closed.
#[test]
fn pre_stamp_era_prefix_is_backfilled_on_open() {
    let store = make_store();
    let fact = make_fact("f1", "alice", "pre-stamp-era backfill preserves facts");
    store.insert_fact(&fact).expect("insert fact");

    for version in 2..=5 {
        store
            .run_mut_query(
                &format!(r#"?[key] <- [["migration:{version}"]] :rm schema_version {{key}}"#),
                std::collections::BTreeMap::new(),
            )
            .expect("remove pre-stamp-era migration stamp");
    }

    store
        .init_schema()
        .expect("a contiguous pre-stamp-era prefix should backfill and open, not fail closed");

    for version in 2..=5 {
        assert_eq!(
            store
                .migration_stamp_version(version)
                .expect("read backfilled migration stamp"),
            Some(version),
            "migration:{version} should be backfilled as pre-stamp-era"
        );
    }
    assert_eq!(
        store.schema_version().expect("schema version"),
        KnowledgeStore::SCHEMA_VERSION,
        "backfill must not change the store's current schema version"
    );

    let facts = store
        .query_facts("alice", "2026-06-01", 10)
        .expect("query facts after backfilled open");
    assert_eq!(facts.len(), 1, "backfill must not touch existing data");
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

/// aletheia#6838 correction 2: a store whose pre-open manifest disagrees
/// with this binary's `SCHEMA_VERSION` must refuse before `open_fjall` ever
/// touches the fjall directory again, not merely after opening it. Before
/// the pre-open guard existed, rewriting this file had no effect at all --
/// `open_fjall` never read it, so the mismatch went undetected and the
/// store opened normally.
#[cfg(feature = "storage-fjall")]
#[test]
fn open_fjall_refuses_before_touching_fjall_on_manifest_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("knowledge");
    {
        let _store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
            .expect("open original store");
    } // dropped here: releases the fjall lock before the manifest is rewritten

    // Simulate a store last opened by a different binary version -- e.g.
    // an older build re-opening a store a newer one already migrated --
    // by rewriting the pre-open manifest directly, without touching fjall
    // at all.
    let manifest_path = path.join("schema_manifest.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "test rewrites the pre-open manifest out-of-band on purpose, simulating a \
                  version-mismatched store without touching fjall"
    )]
    std::fs::write(
        &manifest_path,
        format!(
            r#"{{"schema_version":{}}}"#,
            KnowledgeStore::SCHEMA_VERSION + 1
        ),
    )
    .expect("rewrite manifest to simulate a version-mismatched store");

    let Err(err) = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding")) else {
        panic!("a store whose manifest disagrees with SCHEMA_VERSION must refuse to open");
    };

    assert!(
        matches!(
            err,
            crate::error::Error::SchemaVersion {
                expected: KnowledgeStore::SCHEMA_VERSION,
                found,
                ..
            } if found == KnowledgeStore::SCHEMA_VERSION + 1
        ),
        "expected a typed schema version refusal, got: {err}"
    );
}

/// A store with no manifest yet (one only ever opened by binaries that
/// predate this guard) must still open normally via the existing in-band
/// check, and gets a manifest backfilled so its *next* open gets the fast,
/// pre-fjall refusal too. The guard must never introduce a new refusal for
/// a store nothing has stamped.
#[cfg(feature = "storage-fjall")]
#[test]
fn open_fjall_backfills_manifest_for_a_store_that_predates_the_guard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("knowledge");
    {
        let _store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
            .expect("open original store");
    }

    let manifest_path = path.join("schema_manifest.json");
    std::fs::remove_file(&manifest_path).expect("remove manifest to simulate a pre-guard store");

    let _store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
        .expect("a store with no manifest yet must still open via the existing in-band check");

    let stamped = std::fs::read_to_string(&manifest_path).expect("manifest should be backfilled");
    assert!(
        stamped.contains(&KnowledgeStore::SCHEMA_VERSION.to_string()),
        "backfilled manifest should record the current schema version, got: {stamped}"
    );
}

/// aletheia#6838 correction 2 required fix: `check_before_open` refuses
/// only a manifest *newer* than `SCHEMA_VERSION`, never one older. A
/// manifest stamped below `SCHEMA_VERSION` is the ordinary upgrade path --
/// a newer binary opening a store a prior release left behind mid-lineage
/// -- and must migrate forward through the existing in-band machinery
/// (`protect_pre_migration` + `init_schema`/`apply_pending_migrations`),
/// the same as a store with no manifest at all, then have the manifest
/// re-stamped at the new `SCHEMA_VERSION`. Refusing here instead would
/// stop every future schema bump for every existing instance at its very
/// next start.
#[cfg(feature = "storage-fjall")]
#[test]
fn open_fjall_migrates_a_store_whose_manifest_predates_schema_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("knowledge");
    // `migration_registry_is_sequential_and_current` (above) guarantees
    // `MIGRATIONS` is contiguous from target 2 up to `SCHEMA_VERSION`, so
    // `SCHEMA_VERSION - 1` is a real, registered migration target -- not
    // an arbitrary number this test invented.
    let previous_target = KnowledgeStore::SCHEMA_VERSION - 1;

    {
        let store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
            .expect("open original store");
        // Roll the in-band schema_version relation back to the last real
        // migration target below current, simulating a store a prior
        // release of this binary left behind mid-lineage.
        store
            .stamp_schema_version(previous_target, "test: simulate a store predating a bump")
            .expect("stamp store back to a real prior migration target");
    } // dropped here: releases the fjall lock before the manifest is rewritten

    // Roll the pre-open manifest back to match -- the same store's
    // manifest as it would have been left by that older release.
    let manifest_path = path.join("schema_manifest.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "test rewrites the pre-open manifest out-of-band on purpose, simulating a \
                  store an older release last stamped, without touching fjall"
    )]
    std::fs::write(
        &manifest_path,
        format!(r#"{{"schema_version":{previous_target}}}"#),
    )
    .expect("rewrite manifest to simulate a store predating the current schema version");

    let store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
        .expect("a manifest older than SCHEMA_VERSION must migrate forward, not refuse");

    assert_eq!(
        store
            .schema_version()
            .expect("read schema version after reopen"),
        KnowledgeStore::SCHEMA_VERSION,
        "reopening must run the pending migration(s) up to the current schema version"
    );

    let stamped = std::fs::read_to_string(&manifest_path).expect("read re-stamped manifest");
    assert!(
        stamped.contains(&KnowledgeStore::SCHEMA_VERSION.to_string()),
        "manifest must be re-stamped at SCHEMA_VERSION once the migration completes, got: {stamped}"
    );
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

/// aletheia#7162 F1: the pre-migration snapshot must never land as a
/// `<cohort>.pre-migration-snapshot` sibling directly inside the
/// knowledge root -- the same directory every cohort-directory walker
/// (`is_cohort_dir`, the recall recovery walk, `memory reembed`)
/// enumerates. Before this fix, `protect_pre_migration` used
/// `path.with_extension(...)`, which does exactly that; only relying on
/// every current and future walker remembering to call `is_cohort_dir`
/// kept it from being mistaken for a live cohort (aletheia#7165's own
/// near-miss). Nesting it under `.pre-migration-snapshots` instead removes
/// the hazard structurally rather than depending on that convention being
/// followed correctly everywhere, forever.
#[cfg(feature = "storage-fjall")]
#[test]
fn pre_migration_snapshot_lands_outside_the_enumerable_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("knowledge.fjall");
    let path = root.join("shared");

    {
        let store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
            .expect("create fresh store");
        store
            .stamp_schema_version(13, "test")
            .expect("force a stale stamp so a migration reads as pending");
    } // dropped here: releases the fjall lock before protect_pre_migration reopens it

    let taken = KnowledgeStore::protect_pre_migration(&path)
        .expect("protect_pre_migration should not error")
        .expect("a stale stamp must be protected with a snapshot");

    assert!(taken.path.exists(), "the snapshot must exist on disk");
    assert_eq!(
        taken.path,
        KnowledgeStore::pre_migration_snapshot_dir(&path),
        "protect_pre_migration must use its own documented snapshot location"
    );
    assert_ne!(
        taken.path.parent(),
        Some(root.as_path()),
        "the snapshot's parent must not be the knowledge root that \
         cohort-directory walkers enumerate, got {}",
        taken.path.display()
    );
    assert!(
        !root.join("shared.pre-migration-snapshot").exists(),
        "the snapshot must never land at the old `<cohort>.pre-migration-snapshot` \
         sibling path inside the knowledge root"
    );
}

/// aletheia#7162 F2 / aletheia#5779 F1: a cohort literally named `psyche`
/// is the copy *root* of its own pre-migration snapshot, not a refused
/// descendant -- [`snapshot::REFUSED_COMPONENT`]'s policy only refuses a
/// nested `psyche` directory found *below* a different cohort's root.
/// Confirms this holds at the `protect_pre_migration` entry point named in
/// aletheia#7162, not only at `copy_excluding_psyche`'s own unit level: the
/// psyche cohort's own snapshot must be a full copy, never a silent no-op
/// that still reports success.
#[cfg(feature = "storage-fjall")]
#[test]
fn a_cohort_named_psyche_gets_its_own_complete_pre_migration_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("knowledge.fjall");
    let path = root.join("psyche");

    {
        let store = KnowledgeStore::open_fjall(&path, mock_config("mock-embedding"))
            .expect("create fresh psyche cohort");
        store
            .insert_fact(&make_fact(
                "f-psyche",
                "alice",
                "identity-continuity content",
            ))
            .expect("insert fact into psyche cohort");
        store
            .stamp_schema_version(13, "test")
            .expect("force a stale stamp so a migration reads as pending");
    }

    let source_rows = crate::knowledge_store::snapshot::count_data_keyspace_rows(&path)
        .expect("count source rows");
    assert!(source_rows > 0, "sanity: the source cohort must hold data");

    let taken = KnowledgeStore::protect_pre_migration(&path)
        .expect("protect_pre_migration should not error")
        .expect("a stale stamp must be protected with a snapshot");

    let snapshot_rows = crate::knowledge_store::snapshot::count_data_keyspace_rows(&taken.path)
        .expect("count snapshot rows");
    assert_eq!(
        snapshot_rows, source_rows,
        "the psyche cohort's own pre-migration snapshot must be a complete copy"
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
