//! Process-level crash-injection proof for migration atomicity
//! (aletheia#5779 §8.4).
//!
//! Spawns `migration_crash_child` against a real fjall-backed store seeded
//! at schema v1, aborts it (`SIGABRT`, not a panic — a panic unwinds and
//! may flush the very state the crash is supposed to interrupt) at a chosen
//! step inside `migrate_v1_to_v2`, then reopens the SAME on-disk store in
//! THIS process (no crash armed) and verifies it converges to exactly what
//! an uninterrupted run produces.
//!
//! This is a bounded, representative instantiation of plan §8.4's full
//! 8 migrations × 9 steps × 3 fault kinds = 216 matrix: one migration
//! (`v1->v2`, the first one any pre-v2 store climbs through), the abort
//! fault kind, across all 9 sequence steps. `STEPS` below is every step the
//! full matrix defines for a single migration; widening `MIGRATIONS` to the
//! other 7 destructive sites extends this toward the complete nightly
//! matrix without changing the harness shape. In-process step-failure and
//! recovery-sweep coverage (the other two fault kinds, and the "mem
//! control" plan §8.4 calls for — mem storage has no cross-process crash
//! story to test, since nothing survives losing the process) already lives
//! in `episteme::knowledge_store::migration_atomic`'s own test module,
//! which drives the identical private step functions against `open_mem`.
//!
//! Runs under `cargo nextest`, which retries flaky tests by default
//! (`.config/nextest.toml`'s `[profile.default]`/`[profile.ci]`
//! `retries = { backoff = "fixed", count = 2, delay = "1s" }`) — a retry
//! would mask exactly the nondeterminism this harness exists to find. Both
//! tests in this file have a per-test `retries = 0` override in that same
//! config, in both profiles; flakiness here is a real finding, never a
//! silently-retried-away transient.
#![expect(clippy::expect_used, reason = "test setup and assertions")]

use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;

use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

const V1_FACTS_DDL: &str = r":create facts {
    id: String, valid_from: String =>
    content: String, nous_id: String, confidence: Float, tier: String,
    valid_to: String, superseded_by: String?, source_session_id: String?,
    recorded_at: String
}";

const INSERT_V1_FACT: &str = r#"
?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by,
  source_session_id, recorded_at] <- [[
    "f-1", "2026-01-01T00:00:00Z", "seed fact", "alice", 0.8, "verified",
    "9999-12-31", null, "sess-1", "2026-01-01T00:00:00Z"
]]
:put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at}
"#;

/// Every step plan §8.2 numbers for a single migration.
const STEPS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9];

fn store_config() -> KnowledgeConfig {
    KnowledgeConfig {
        dim: 4,
        allow_assumed_embedding_meta: true,
        ..Default::default()
    }
}

/// Relations no migration function ever creates — present at every schema
/// version, including v1, because `init_schema`'s fresh-store branch is the
/// only place that creates them (episteme's
/// `knowledge_store::tests::migration_atomicity::FOUNDATIONAL_RELATIONS`
/// carries the same list; duplicated here because this crate has no access
/// to episteme's test-only module).
const FOUNDATIONAL_RELATIONS: &[&str] = &["facts", "entities", "relationships", "embeddings"];

/// Remove every relation except [`FOUNDATIONAL_RELATIONS`] and
/// `schema_version`. `open_fjall` on a nonexistent path creates everything
/// at the current shape up front; without this, climbing from a faked-v1
/// stamp hits "relation already exists" on the first unconditional-create
/// migration (v3->v4 onward) — a fixture artifact, not a production
/// scenario (a real store climbing from v1 never had these relations to
/// collide with).
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
        let indices = store
            .run_script_read_only(&format!("::indices {name}"), BTreeMap::new())
            .expect("list indices");
        for j in 0..indices.row_count() {
            let idx_name = indices.get_string(j, "name").expect("index name");
            let idx_kind = indices.get_string(j, "type").expect("index kind");
            let verb = match idx_kind.as_str() {
                "normal" => "::index drop",
                "hnsw" => "::hnsw drop",
                "fts" => "::fts drop",
                "lsh" => "::lsh drop",
                other => panic!("unknown index kind '{other}' on '{name}'"),
            };
            store
                .run_mut_query(&format!("{verb} {name}:{idx_name}"), BTreeMap::new())
                .unwrap_or_else(|e| panic!("drop index {idx_name} on {name}: {e}"));
        }
        store
            .run_mut_query(&format!("::remove {name}"), BTreeMap::new())
            .unwrap_or_else(|e| panic!("remove {name}: {e}"));
    }
}

/// Seed a fresh fjall store at `path`, then downgrade its `facts` relation
/// to the v1 shape and stamp schema version 1 — matching exactly what
/// `migrate_v1_to_v2` expects to find, via the public API only (this crate
/// has no access to episteme's private `stamp_schema_version`).
fn seed_v1_store(path: &Path) {
    let store = KnowledgeStore::open_fjall(path, store_config()).expect("open fresh store");
    let _ = store.run_mut_query("::fts drop facts:content_fts", BTreeMap::new());
    store
        .run_mut_query("::remove facts", BTreeMap::new())
        .expect("remove fresh v19-shape facts");
    store
        .run_mut_query(V1_FACTS_DDL, BTreeMap::new())
        .expect("create v1-shape facts");
    store
        .run_mut_query(INSERT_V1_FACT, BTreeMap::new())
        .expect("insert v1 seed row");
    strip_to_v1_shape(&store);
    store
        .run_mut_query(
            r#"?[key, version] <- [["schema", 1], ["migration:1", 1]] :put schema_version {key => version}"#,
            BTreeMap::new(),
        )
        .expect("stamp schema version 1");
    // WHY: drop the handle explicitly (rather than relying on end-of-scope)
    // to make the lock-release ordering visible at the call site — the
    // child process cannot open this path until fjall's lock is released.
    drop(store);
}

/// Read every current-shape `facts` column via the public query API (this
/// crate has no access to episteme's crate-internal `export_relations`),
/// as JSON rows sorted by `id` — Datalog result order is not guaranteed, so
/// row order must not be part of the equivalence claim.
fn exported_facts(path: &Path) -> Vec<Vec<serde_json::Value>> {
    let store = KnowledgeStore::open_fjall(path, store_config()).expect("open for export");
    let result = store
        .run_script_read_only(
            r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                superseded_by, source_session_id, recorded_at,
                access_count, last_accessed_at, stability_hours, fact_type,
                is_forgotten, forgotten_at, forget_reason, scope, project_id,
                visibility, sensitivity] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason, scope, project_id,
                       visibility, sensitivity}",
            BTreeMap::new(),
        )
        .expect("read facts");
    let mut rows = result.rows_to_json();
    rows.sort_by(|a, b| {
        a.first()
            .map(serde_json::Value::to_string)
            .cmp(&b.first().map(serde_json::Value::to_string))
    });
    rows
}

fn run_child(path: &Path, crash_at: Option<u32>) -> std::process::ExitStatus {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_migration_crash_child"));
    cmd.arg(path).arg("4");
    if let Some(step) = crash_at {
        cmd.env("ALETHEIA_MIGRATION_CRASH_AT", step.to_string());
    } else {
        cmd.env_remove("ALETHEIA_MIGRATION_CRASH_AT");
    }
    cmd.status().expect("spawn migration_crash_child")
}

#[test]
fn baseline_uninterrupted_migration_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store");
    seed_v1_store(&path);

    let status = run_child(&path, None);
    assert!(
        status.success(),
        "an uninterrupted run must exit cleanly, got: {status:?}"
    );
}

/// For every step in the sequence: abort the child exactly there, then
/// reopen the SAME on-disk store (no crash armed) and assert it converges
/// to schema v19 with the seed row intact and correctly backfilled — the
/// crash-safety table in plan §8.3 claims zero loss for every prefix.
#[test]
fn crash_at_every_step_resumes_to_identical_final_state() {
    // Baseline: an independent, uninterrupted seed+migrate for comparison.
    let baseline_dir = tempfile::tempdir().expect("baseline tempdir");
    let baseline_path = baseline_dir.path().join("store");
    seed_v1_store(&baseline_path);
    assert!(
        run_child(&baseline_path, None).success(),
        "baseline run must succeed"
    );
    let baseline = exported_facts(&baseline_path);

    for &step in STEPS {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir for step {step}: {e}"));
        let path = dir.path().join("store");
        seed_v1_store(&path);

        let crashed_status = run_child(&path, Some(step));
        assert!(
            !crashed_status.success(),
            "step {step}: child must not exit cleanly when a crash is armed, got {crashed_status:?}"
        );
        assert_eq!(
            crashed_status.signal(),
            Some(6), // SIGABRT
            "step {step}: child must die by SIGABRT (process::abort), not some other failure mode — got {crashed_status:?}"
        );

        // Resume: reopen the SAME path with no crash armed. This is the
        // entire correctness claim of §8.3 — a plain retry must converge.
        let resumed_status = run_child(&path, None);
        assert!(
            resumed_status.success(),
            "step {step}: resume after crash must succeed, got {resumed_status:?}"
        );

        let resumed = exported_facts(&path);
        assert_eq!(
            resumed, baseline,
            "step {step}: resumed data must be byte-identical to the uninterrupted baseline — \
             a crash at this step must lose nothing"
        );
    }
}
