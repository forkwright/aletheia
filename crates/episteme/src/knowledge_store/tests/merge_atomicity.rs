//! Regression coverage for aletheia#7289: `execute_merge` used to write
//! relationship redirects, `fact_entities` transfers, the alias upsert, the
//! entity deletion, and the audit row as N independent single-statement
//! writes. A failure partway through that sequence left the graph
//! checker's orphaned-entity / dangling-edge signals genuinely wrong —
//! surfacing as the Memory tab's "Inconsistency detected" badge — instead
//! of merely leaving the merge unapplied.
//!
//! This file proves the fix (`super::super::merge_commit`): a failing
//! write at any step aborts the whole merge with nothing written, and a
//! retry after the failure converges cleanly.

#![expect(clippy::expect_used, reason = "test setup and assertions")]

use std::collections::BTreeMap;

use crate::engine::DataValue;
use crate::id::EntityId;
use crate::knowledge_store::KnowledgeStore;
use crate::knowledge_store::merge_commit::{MergeWriteStep, failpoint};
use crate::test_fixtures::{make_entity, make_fact, make_relationship, make_store};

/// Mirrors pylon's `count_orphaned_entities` (`GET /api/v1/knowledge/check`):
/// entities with no relationship and no `fact_entities` link.
fn count_orphaned_entities(store: &KnowledgeStore) -> usize {
    store
        .run_query(
            r"?[id] :=
                *entities{id},
                not *relationships{src: id},
                not *relationships{dst: id},
                not *fact_entities{entity_id: id}",
            BTreeMap::new(),
        )
        .expect("orphan query")
        .row_count()
}

/// Mirrors pylon's `count_dangling_edges`: relationships whose src or dst
/// no longer names an existing entity.
fn count_dangling_edges(store: &KnowledgeStore) -> usize {
    store
        .run_query(
            r"?[src, dst, relation] :=
                *relationships{src, dst, relation},
                not *entities{id: src}

              ?[src, dst, relation] :=
                *relationships{src, dst, relation},
                not *entities{id: dst}",
            BTreeMap::new(),
        )
        .expect("dangling edge query")
        .row_count()
}

fn entity_exists(store: &KnowledgeStore, id: &str) -> bool {
    let mut params = BTreeMap::new();
    params.insert("id".to_owned(), DataValue::Str(id.into()));
    !store
        .run_query(r"?[id] := *entities{id}, id = $id", params)
        .expect("entity existence query")
        .is_empty()
}

fn relationship_count_from(store: &KnowledgeStore, src: &str) -> usize {
    let mut params = BTreeMap::new();
    params.insert("src".to_owned(), DataValue::Str(src.into()));
    store
        .run_query(r"?[dst] := *relationships{src, dst}, src = $src", params)
        .expect("relationship query")
        .row_count()
}

/// Arm `step`, attempt a merge that touches a relationship, a
/// `fact_entities` link, and an alias-worthy name difference, and assert:
/// the attempt fails, nothing landed (source graph untouched, no orphan,
/// no dangling edge), and a retry without the failpoint converges cleanly.
fn assert_step_failure_is_atomic_and_retry_converges(step: MergeWriteStep) {
    let store = make_store();

    store
        .insert_entity(&make_entity("canonical", "Canonical", "concept"))
        .expect("insert canonical entity");
    store
        .insert_entity(&make_entity("merged", "Merged", "concept"))
        .expect("insert merged entity");
    store
        .insert_entity(&make_entity("third", "Third", "concept"))
        .expect("insert third entity");
    store
        .insert_relationship(&make_relationship("merged", "third", "knows", 0.8))
        .expect("insert relationship");

    let fact = make_fact("f1", "agent-a", "some fact content");
    store.insert_fact(&fact).expect("insert fact");
    let merged_id = EntityId::new("merged").expect("valid test id");
    let canonical_id = EntityId::new("canonical").expect("valid test id");
    store
        .insert_fact_entity(&fact.id, &merged_id)
        .expect("link fact to merged entity");

    // WHY: ground `canonical` with a link of its own, independent of
    // anything this merge redirects (mirrors the self-loop test's own
    // fixture below). Without this, `canonical` starts with zero
    // relationships and zero `fact_entities` rows, so
    // `count_orphaned_entities` reads 1 *before* `execute_merge` is even
    // called — the orphan assertions below would then be asserting a
    // pre-existing fixture gap, not the merge's own atomicity.
    let ground_fact = make_fact("ground", "agent-a", "canonical's own fact");
    store
        .insert_fact(&ground_fact)
        .expect("insert canonical's grounding fact");
    store
        .insert_fact_entity(&ground_fact.id, &canonical_id)
        .expect("link grounding fact to canonical entity");

    failpoint::arm(step);
    let result = store.execute_merge(&canonical_id, &merged_id);
    assert!(result.is_err(), "merge must fail when {step:?} is armed");

    // Nothing must have landed: the merged entity survives untouched, its
    // relationship and fact_entities link are exactly as before, and the
    // graph checker's two consistency signals both read zero.
    assert!(
        entity_exists(&store, "merged"),
        "merged entity must survive a failed merge ({step:?})"
    );
    assert!(
        entity_exists(&store, "canonical"),
        "canonical entity must survive a failed merge ({step:?})"
    );
    assert_eq!(
        relationship_count_from(&store, "merged"),
        1,
        "merged entity's relationship must survive a failed merge ({step:?})"
    );
    assert_eq!(
        count_orphaned_entities(&store),
        0,
        "a failed merge must not orphan any entity ({step:?})"
    );
    assert_eq!(
        count_dangling_edges(&store),
        0,
        "a failed merge must not dangle any edge ({step:?})"
    );

    // Retry without the failpoint: the merge now converges cleanly.
    let record = store
        .execute_merge(&canonical_id, &merged_id)
        .unwrap_or_else(|e| panic!("retry after {step:?} failure must succeed: {e}"));
    assert_eq!(record.relationships_redirected, 1, "step={step:?}");
    assert_eq!(record.facts_transferred, 1, "step={step:?}");
    assert!(
        !entity_exists(&store, "merged"),
        "merged entity must be gone after a successful retry ({step:?})"
    );
    assert_eq!(
        relationship_count_from(&store, "canonical"),
        1,
        "the relationship must be redirected onto the canonical entity ({step:?})"
    );
    assert_eq!(
        count_orphaned_entities(&store),
        0,
        "the merged graph must have no orphaned entities ({step:?})"
    );
    assert_eq!(
        count_dangling_edges(&store),
        0,
        "the merged graph must have no dangling edges ({step:?})"
    );
}

#[test]
fn merge_relationship_failure_is_atomic() {
    assert_step_failure_is_atomic_and_retry_converges(MergeWriteStep::Relationship);
}

#[test]
fn merge_fact_entity_failure_is_atomic() {
    assert_step_failure_is_atomic_and_retry_converges(MergeWriteStep::FactEntity);
}

#[test]
fn merge_alias_failure_is_atomic() {
    assert_step_failure_is_atomic_and_retry_converges(MergeWriteStep::Alias);
}

#[test]
fn merge_delete_entity_failure_is_atomic() {
    // WHY: this is the exact reported defect (#7289) — relationships and
    // fact_entities were already redirected by independent writes, but the
    // merged entity row survived because its own deletion failed, reading
    // as a genuine orphaned entity on the next `GET /api/v1/knowledge/check`.
    assert_step_failure_is_atomic_and_retry_converges(MergeWriteStep::DeleteEntity);
}

#[test]
fn merge_audit_failure_is_atomic() {
    assert_step_failure_is_atomic_and_retry_converges(MergeWriteStep::Audit);
}

/// WHY(#7289 B4 regression): a relationship where `merged_id` is *both*
/// endpoints (a self-loop on the entity being merged) satisfies both of
/// `plan_relationship_redirects`' queries — `src = merged_id` and
/// `dst = merged_id` — and, before the plan was keyed on `(src, dst)`, was
/// staged twice: once with `redirect_src = true` and once with
/// `redirect_src = false`. Neither half's self-loop check considered the
/// *other* half's redirected side, so both proceeded to upsert —
/// `(canonical, merged)` and `(merged, canonical)` — and both then dangled
/// once `merged` itself was deleted at the end of the same transaction,
/// while `relationships_redirected` double-counted the single row.
///
/// This also covers a direct `(merged, canonical)` edge — already a
/// same-day existing edge between the two entities being merged — to prove
/// the pre-existing "would redirect onto a self-loop on canonical" drop
/// path still holds alongside the new one.
#[test]
fn merge_relationship_self_loop_on_merged_entity_yields_no_dangling_edge() {
    let store = make_store();

    store
        .insert_entity(&make_entity("canonical", "Canonical", "concept"))
        .expect("insert canonical entity");
    store
        .insert_entity(&make_entity("merged", "Merged", "concept"))
        .expect("insert merged entity");

    // Ground `canonical` with a link of its own, independent of anything
    // this merge redirects, so the orphan assertion below is about the
    // merge's own effect on the graph, not an unrelated entity that simply
    // never had any connection.
    let fact = make_fact("f1", "agent-a", "some fact content");
    store.insert_fact(&fact).expect("insert fact");
    let canonical_id = EntityId::new("canonical").expect("valid test id");
    let merged_id = EntityId::new("merged").expect("valid test id");
    store
        .insert_fact_entity(&fact.id, &canonical_id)
        .expect("link fact to canonical entity");

    // A self-loop entirely on the entity being merged...
    store
        .insert_relationship(&make_relationship("merged", "merged", "self", 0.5))
        .expect("insert self-loop relationship");
    // ...and a direct edge between the merged and canonical entities.
    store
        .insert_relationship(&make_relationship("merged", "canonical", "knows", 0.7))
        .expect("insert merged-to-canonical relationship");

    let record = store
        .execute_merge(&canonical_id, &merged_id)
        .expect("merge with a self-loop on the merged entity must succeed");

    assert_eq!(
        record.relationships_redirected, 2,
        "two distinct relationship rows were staged and touched -- the \
         self-loop must count once, not twice"
    );
    assert_eq!(
        count_orphaned_entities(&store),
        0,
        "the merge must not orphan the surviving entity"
    );
    assert_eq!(
        count_dangling_edges(&store),
        0,
        "neither dropped row may survive as an edge pointing at the \
         now-deleted merged entity"
    );
    assert!(
        !entity_exists(&store, "merged"),
        "merged entity must be gone after the merge"
    );
    assert_eq!(
        relationship_count_from(&store, "canonical"),
        0,
        "neither row is redirected onto canonical: the merged-entity \
         self-loop is dropped outright, and the merged-canonical edge \
         would become a self-loop on canonical and is dropped by the \
         existing rule"
    );

    // No (canonical, canonical) row exists: this merge never deliberately
    // chose to create a self-loop on the surviving entity.
    let mut params = BTreeMap::new();
    params.insert("src".to_owned(), DataValue::Str("canonical".into()));
    params.insert("dst".to_owned(), DataValue::Str("canonical".into()));
    assert!(
        store
            .run_query(
                r"?[src, dst] := *relationships{src, dst}, src = $src, dst = $dst",
                params,
            )
            .expect("canonical self-loop query")
            .is_empty(),
        "merge must never fabricate a (canonical, canonical) relationship row"
    );
}
