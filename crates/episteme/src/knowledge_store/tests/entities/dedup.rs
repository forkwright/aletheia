#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "dedup integration tests assert positional result rows"
)]

use crate::knowledge_store::KnowledgeStore;
use crate::test_fixtures::{make_entity, make_fact, make_store};

/// Wire an entity into a nous's scope by inserting a stub fact owned by
/// `nous_id` and a `fact_entities` row linking the fact to the entity.
///
/// `load_entity_infos` scopes via the `fact_entities → facts.nous_id` join
/// (#4165 E), so tests that insert via `insert_entity` alone must add a link
/// or the dedup pipeline cannot see them. Reusing this helper keeps the dedup
/// integration tests honest about the production path.
fn link_entity_to_nous(store: &KnowledgeStore, entity_id: &str, nous_id: &str) {
    let fact_id = format!("fact-{entity_id}-{nous_id}");
    let fact = make_fact(&fact_id, nous_id, "stub fact linking entity to nous");
    store.insert_fact(&fact).expect("insert stub fact");
    let fid = crate::id::FactId::new(&fact_id).expect("valid fact id");
    let eid = crate::id::EntityId::new(entity_id).expect("valid entity id");
    store
        .insert_fact_entity(&fid, &eid)
        .expect("link fact to entity");
}
// WHY(#4165): `dedup_tests.rs` covers `generate_candidates` +
// `make_embedding_lookup` in isolation; the tests below exercise the actual
// `KnowledgeStore` API — schema v13 column, `update_entity_name_embedding`,
// `find_duplicate_entities`, `run_entity_dedup`, and the `approve_merge`
// queue — proving the pipeline survives Cozo storage round-trips.

/// Pre-fix shape: insert two near-identical entities without populating
/// `name_embedding`, then run the production `run_entity_dedup`. The bug
/// the design doc described — no auto-merges possible — must reproduce.
#[test]
fn dedup_pipeline_without_embeddings_reproduces_bug_4165() {
    let store = make_store();
    let mut e1 = make_entity("e1", "Acme Corporation", "organization");
    e1.aliases = vec!["Acme".to_owned()];
    let mut e2 = make_entity("e2", "acme corporation", "organization");
    e2.aliases = vec!["Acme".to_owned()];
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");
    link_entity_to_nous(&store, "e1", "test-nous");
    link_entity_to_nous(&store, "e2", "test-nous");

    let records = store
        .run_entity_dedup("test-nous")
        .expect("dedup should succeed even with no embeddings");
    assert!(
        records.is_empty(),
        "pre-fix shape (no embeddings stored) must produce zero auto-merges — this is the unreachability bug #4165 documented"
    );

    // The pair should still land in the review queue (score 0.70).
    let pending = store
        .get_pending_merges("test-nous")
        .expect("read pending merges");
    assert_eq!(
        pending.len(),
        1,
        "the pair should be queued for review even without embeddings"
    );
}

/// Reachability: populate both entities' `name_embedding`s, run the
/// production `run_entity_dedup`, and assert a real auto-merge was
/// executed. This is the end-to-end proof that #4165 Path A reaches
/// `MergeDecision::AutoMerge` from production code.
#[test]
fn dedup_pipeline_with_embeddings_reaches_auto_merge() {
    let store = make_store();
    let mut e1 = make_entity("e1", "Acme Corporation", "organization");
    e1.aliases = vec!["Acme".to_owned()];
    let mut e2 = make_entity("e2", "acme corporation", "organization");
    e2.aliases = vec!["Acme".to_owned()];
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");
    link_entity_to_nous(&store, "e1", "test-nous");
    link_entity_to_nous(&store, "e2", "test-nous");

    // DIM=4 from `test_fixtures::DIM`; identical unit vectors yield
    // cosine = 1.0 — the test-shape equivalent of "the provider thinks
    // these names mean the same thing".
    let emb = vec![1.0_f32, 0.0, 0.0, 0.0];
    let e1_id = crate::id::EntityId::new("e1").expect("valid test id");
    let e2_id = crate::id::EntityId::new("e2").expect("valid test id");
    store
        .update_entity_name_embedding(&e1_id, Some(emb.clone()))
        .expect("write e1 name_embedding");
    store
        .update_entity_name_embedding(&e2_id, Some(emb))
        .expect("write e2 name_embedding");

    // Round-trip check that the embedding survives storage.
    let roundtrip = store
        .get_entity_name_embedding(&e1_id)
        .expect("read e1 name_embedding");
    assert!(
        matches!(roundtrip, Some(ref v) if v.len() == 4),
        "stored name_embedding must round-trip with the configured dim"
    );

    let records = store
        .run_entity_dedup("test-nous")
        .expect("dedup with embeddings");
    assert_eq!(
        records.len(),
        1,
        "with name embeddings stored, the production dedup pipeline must reach AutoMerge — proves #4165 Path A is wired end-to-end"
    );
    let record = &records[0];
    assert!(
        record.merge_score >= 0.90,
        "executed merge score must clear the AutoMerge threshold; got {}",
        record.merge_score
    );

    // After auto-merge, the merged entity is gone and only canonical remains.
    let surviving = store.list_entities().expect("list_entities");
    assert_eq!(
        surviving.len(),
        1,
        "auto-merge must collapse the duplicate pair to a single canonical entity"
    );
}

/// Reachability via `find_duplicate_entities`: returns candidates whose
/// `embed_similarity` is the real cosine of the stored vectors, not 0.0.
#[test]
fn find_duplicate_entities_returns_real_embed_similarity() {
    let store = make_store();
    let e1 = make_entity("e1", "Differential Equation", "concept");
    let e2 = make_entity("e2", "Difference Equation", "concept");
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");
    link_entity_to_nous(&store, "e1", "test-nous");
    link_entity_to_nous(&store, "e2", "test-nous");

    let e1_id = crate::id::EntityId::new("e1").expect("valid test id");
    let e2_id = crate::id::EntityId::new("e2").expect("valid test id");
    // 30° apart → cosine ≈ 0.866 (well above 0 but below 1).
    store
        .update_entity_name_embedding(&e1_id, Some(vec![1.0_f32, 0.0, 0.0, 0.0]))
        .expect("e1 emb");
    store
        .update_entity_name_embedding(&e2_id, Some(vec![0.866_025_4_f32, 0.5, 0.0, 0.0]))
        .expect("e2 emb");

    let candidates = store
        .find_duplicate_entities("test-nous")
        .expect("find_duplicate_entities");
    assert_eq!(candidates.len(), 1, "JW similar names form one candidate");
    let c = &candidates[0];
    assert!(
        c.embed_similarity > 0.85 && c.embed_similarity < 0.95,
        "find_duplicate_entities must surface real cosine similarity from stored embeddings; got {}",
        c.embed_similarity
    );
}

/// `update_entity_name_embedding` must reject vectors whose length does
/// not match `KnowledgeConfig::dim`. A silent wrong-dim write would
/// corrupt the typed column and break every subsequent dedup run.
#[test]
fn update_entity_name_embedding_rejects_wrong_dimension() {
    let store = make_store();
    let entity = make_entity("e1", "Alice", "person");
    store.insert_entity(&entity).expect("insert entity");
    let id = crate::id::EntityId::new("e1").expect("valid test id");
    let wrong_dim = vec![1.0_f32; 7]; // DIM is 4 in tests
    let err = store
        .update_entity_name_embedding(&id, Some(wrong_dim))
        .expect_err("must reject wrong-dim embedding");
    let msg = format!("{err}");
    assert!(
        msg.contains("dimension"),
        "error message should mention dimension; got: {msg}"
    );
}

/// `update_entity_name_embedding(_, None)` clears a stored embedding —
/// useful for tests, operators reverting a bad backfill, and admission
/// policies that disqualify a stored vector after the fact.
#[test]
fn update_entity_name_embedding_clears_with_none() {
    let store = make_store();
    let entity = make_entity("e1", "Alice", "person");
    store.insert_entity(&entity).expect("insert entity");
    let id = crate::id::EntityId::new("e1").expect("valid test id");
    store
        .update_entity_name_embedding(&id, Some(vec![1.0_f32, 0.0, 0.0, 0.0]))
        .expect("set embedding");
    assert!(
        store
            .get_entity_name_embedding(&id)
            .expect("get embedding")
            .is_some(),
        "embedding should be present after set"
    );
    store
        .update_entity_name_embedding(&id, None)
        .expect("clear embedding");
    assert!(
        store
            .get_entity_name_embedding(&id)
            .expect("get embedding")
            .is_none(),
        "embedding should be cleared after None write"
    );
}

/// `approve_merge` is the operational half of #4165 Path A. Insert two
/// entities that land in the review queue (score in `[0.70, 0.90)`),
/// then approve the merge and assert that:
///   1. The merged entity is gone.
///   2. The canonical entity carries the merged name as an alias.
///   3. `pending_merges` no longer contains the pair.
///   4. `merge_audit` records the resolution.
#[test]
fn approve_merge_drains_review_queue() {
    let store = make_store();
    // Two entities that match on every non-embedding signal: cap at 0.70
    // → review tier, not auto-merge.
    let mut e1 = make_entity("e1", "Acme Corporation", "organization");
    e1.aliases = vec!["Acme".to_owned()];
    let mut e2 = make_entity("e2", "acme corporation", "organization");
    e2.aliases = vec!["Acme".to_owned()];
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");
    link_entity_to_nous(&store, "e1", "test-nous");
    link_entity_to_nous(&store, "e2", "test-nous");

    let records = store
        .run_entity_dedup("test-nous")
        .expect("dedup populates pending_merges");
    assert!(
        records.is_empty(),
        "no auto-merge expected for embed=null pair"
    );
    let pending = store
        .get_pending_merges("test-nous")
        .expect("pending merges");
    assert_eq!(pending.len(), 1, "pair should be queued for review");

    let e1_id = crate::id::EntityId::new("e1").expect("valid test id");
    let e2_id = crate::id::EntityId::new("e2").expect("valid test id");
    let record = store
        .approve_merge_for_nous("test-nous", &e1_id, &e2_id)
        .expect("approve_merge must succeed for queued pair");
    assert_eq!(record.canonical_entity_id, e1_id);
    assert_eq!(record.merged_entity_id, e2_id);

    let surviving = store.list_entities().expect("list_entities");
    assert_eq!(surviving.len(), 1, "approved merge must collapse the pair");
    assert_eq!(surviving[0].id, e1_id);
    // #4165: when the merged name differs only by case, `add_alias_to_entity`
    // skips that duplicate spelling and preserves the existing "Acme" alias
    // as the audit trail for the merged identity.
    assert!(
        surviving[0]
            .aliases
            .iter()
            .any(|a| a.eq_ignore_ascii_case("Acme")),
        "canonical entity must preserve its existing aliases through merge: got {:?}",
        surviving[0].aliases
    );

    let pending_after = store
        .get_pending_merges("test-nous")
        .expect("pending merges after approve");
    assert!(
        pending_after.is_empty(),
        "approved row must be removed from pending_merges; got {} remaining",
        pending_after.len()
    );

    let history = store
        .get_merge_history("test-nous")
        .expect("merge_audit history");
    assert_eq!(
        history.len(),
        1,
        "approved merge must be recorded in merge_audit"
    );
}

#[test]
fn pending_merge_review_queue_is_scoped_by_nous() {
    let store = make_store();
    let mut a1 = make_entity("a1", "Acme Corporation", "organization");
    a1.aliases = vec!["Acme".to_owned()];
    let mut a2 = make_entity("a2", "acme corporation", "organization");
    a2.aliases = vec!["Acme".to_owned()];
    let mut b1 = make_entity("b1", "Globex Corporation", "organization");
    b1.aliases = vec!["Globex".to_owned()];
    let mut b2 = make_entity("b2", "globex corporation", "organization");
    b2.aliases = vec!["Globex".to_owned()];

    for entity in [&a1, &a2, &b1, &b2] {
        store.insert_entity(entity).expect("insert entity");
    }
    link_entity_to_nous(&store, "a1", "nous-A");
    link_entity_to_nous(&store, "a2", "nous-A");
    link_entity_to_nous(&store, "b1", "nous-B");
    link_entity_to_nous(&store, "b2", "nous-B");

    store.run_entity_dedup("nous-A").expect("dedup A");
    store.run_entity_dedup("nous-B").expect("dedup B");

    let pending_a = store.get_pending_merges("nous-A").expect("pending A");
    let pending_b = store.get_pending_merges("nous-B").expect("pending B");
    let pending_c = store.get_pending_merges("nous-C").expect("pending C");
    assert_eq!(pending_a.len(), 1, "nous-A sees only its review row");
    assert_eq!(pending_b.len(), 1, "nous-B sees only its review row");
    assert!(pending_c.is_empty(), "foreign nous sees no review rows");
    assert_eq!(pending_a[0].entity_a.as_str(), "a1");
    assert_eq!(pending_b[0].entity_a.as_str(), "b1");

    let a1_id = crate::id::EntityId::new("a1").expect("valid id");
    let a2_id = crate::id::EntityId::new("a2").expect("valid id");
    store
        .approve_merge_for_nous("nous-A", &a1_id, &a2_id)
        .expect("approve A");

    let history_a = store.get_merge_history("nous-A").expect("history A");
    let history_b = store.get_merge_history("nous-B").expect("history B");
    assert_eq!(history_a.len(), 1, "nous-A history contains its approval");
    assert!(
        history_b.is_empty(),
        "nous-B history must not include nous-A approval"
    );
    assert_eq!(
        store
            .get_pending_merges("nous-B")
            .expect("pending B after A approval")
            .len(),
        1,
        "approving nous-A must not drain nous-B review row"
    );
}

#[test]
fn approve_merge_for_nous_rejects_foreign_review_row() {
    let store = make_store();
    let mut e1 = make_entity("foreign-a", "Acme Corporation", "organization");
    e1.aliases = vec!["Acme".to_owned()];
    let mut e2 = make_entity("foreign-b", "acme corporation", "organization");
    e2.aliases = vec!["Acme".to_owned()];
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");
    link_entity_to_nous(&store, "foreign-a", "alice");
    link_entity_to_nous(&store, "foreign-b", "alice");

    store.run_entity_dedup("alice").expect("dedup alice");
    let e1_id = crate::id::EntityId::new("foreign-a").expect("valid id");
    let e2_id = crate::id::EntityId::new("foreign-b").expect("valid id");
    let err = store
        .approve_merge_for_nous("bob", &e1_id, &e2_id)
        .expect_err("bob must not approve alice row");
    assert!(
        err.to_string().contains("pending merge not found"),
        "error should name missing scoped pending row, got: {err}"
    );

    assert_eq!(
        store
            .list_entities()
            .expect("entities after rejected approve")
            .len(),
        2,
        "foreign approval must not merge either entity"
    );
    assert_eq!(
        store
            .get_pending_merges("alice")
            .expect("alice pending after rejected approve")
            .len(),
        1,
        "foreign approval must leave alice review row queued"
    );
}

/// #4165 E regression: `find_duplicate_entities("nous-A")` must not see
/// entities linked exclusively to `nous-B`. Pre-fix, `load_entity_infos`
/// loaded every row of the `entities` relation, so dedup could merge
/// across tenant boundaries the moment Path A's embedding wiring made
/// `AutoMerge` reachable. With the tenant-scoped query in place, the
/// nous-B entity must be invisible to a nous-A dedup scan even though
/// the names + types + aliases are identical.
#[test]
fn find_duplicate_entities_does_not_cross_nous_boundary() {
    let store = make_store();
    let mut e1 = make_entity("e1", "Acme Corporation", "organization");
    e1.aliases = vec!["Acme".to_owned()];
    let mut e2 = make_entity("e2", "Acme Corporation", "organization");
    e2.aliases = vec!["Acme".to_owned()];
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");

    // e1 belongs to nous-A; e2 belongs to nous-B. A nous-A dedup scan
    // must never propose merging them despite the identical surface
    // form (the very leak issue #4165 (F) called out as latent).
    link_entity_to_nous(&store, "e1", "nous-A");
    link_entity_to_nous(&store, "e2", "nous-B");

    let scan_a = store
        .find_duplicate_entities("nous-A")
        .expect("nous-A dedup scan");
    assert!(
        scan_a.is_empty(),
        "nous-A scan must not surface a candidate involving an entity owned exclusively by nous-B"
    );

    let scan_b = store
        .find_duplicate_entities("nous-B")
        .expect("nous-B dedup scan");
    assert!(
        scan_b.is_empty(),
        "nous-B scan must not surface a candidate involving an entity owned exclusively by nous-A"
    );

    let scan_unlinked = store
        .find_duplicate_entities("nous-C")
        .expect("nous-C dedup scan");
    assert!(
        scan_unlinked.is_empty(),
        "a nous with no linked entities must return no dedup candidates"
    );
}

/// `run_entity_dedup_with_tuning` must observe operator-tuned weights
/// and thresholds end-to-end through the production store path (#4165 D).
/// Inserts a Review-tier pair, asserts the default tuning produces no
/// auto-merge, then re-runs under a tuning that lowers
/// `auto_merge_threshold` below the pair's composite score and asserts
/// the same store data now executes a real merge.
#[test]
fn run_entity_dedup_with_tuning_honours_lowered_auto_merge_threshold() {
    let store = make_store();
    let mut e1 = make_entity("e1", "Acme Corporation", "organization");
    e1.aliases = vec!["Acme".to_owned()];
    let mut e2 = make_entity("e2", "acme corporation", "organization");
    e2.aliases = vec!["Acme".to_owned()];
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");
    link_entity_to_nous(&store, "e1", "test-nous");
    link_entity_to_nous(&store, "e2", "test-nous");

    let default_records = store
        .run_entity_dedup_with_tuning("test-nous", &crate::dedup::DedupTuning::DEFAULT)
        .expect("dedup under default tuning");
    assert!(
        default_records.is_empty(),
        "default tuning must keep the embed=null pair in Review (preserves #4165 path-a-reachable_pre_fix_regression contract)"
    );
    assert_eq!(
        store.list_entities().expect("list entities").len(),
        2,
        "no merge executed under default tuning"
    );

    let permissive = crate::dedup::DedupTuning {
        auto_merge_threshold: 0.65,
        ..crate::dedup::DedupTuning::DEFAULT
    };
    let records = store
        .run_entity_dedup_with_tuning("test-nous", &permissive)
        .expect("dedup under permissive tuning");
    assert_eq!(
        records.len(),
        1,
        "lowering auto_merge_threshold to 0.65 must execute the queued Review-tier merge — proves DedupTuning reaches run_entity_dedup_with_tuning end-to-end"
    );

    let surviving = store.list_entities().expect("list entities");
    assert_eq!(
        surviving.len(),
        1,
        "permissive tuning must collapse the pair to a single canonical entity"
    );
}
