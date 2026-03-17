#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::super::*;
use crate::knowledge::{Entity, EpistemicTier, Fact, Relationship};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

const DIM: usize = 4;

fn make_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM }).expect("open_mem")
}

fn test_ts(s: &str) -> jiff::Timestamp {
    crate::knowledge::parse_timestamp(s).expect("valid test timestamp in test helper")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: crate::id::FactId::new_unchecked(id),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: test_ts("2026-01-01"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: test_ts("2026-03-01T00:00:00Z"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    }
}

fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
    Entity {
        id: crate::id::EntityId::new_unchecked(id),
        name: name.to_owned(),
        entity_type: entity_type.to_owned(),
        aliases: vec![],
        created_at: test_ts("2026-03-01T00:00:00Z"),
        updated_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
    Relationship {
        src: crate::id::EntityId::new_unchecked(src),
        dst: crate::id::EntityId::new_unchecked(dst),
        relation: relation.to_owned(),
        weight,
        created_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

proptest! {
    #[test]
    fn fact_roundtrip(
        content in "[a-zA-Z0-9 ]{1,200}",
        confidence in 0.0_f64..=1.0,
    ) {
        let store = make_store();
        let mut fact = make_fact("prop-rt", "agent-prop", &content);
        fact.confidence = confidence;
        store.insert_fact(&fact).expect("insert");
        let results = store.query_facts("agent-prop", "2026-06-01", 10).expect("query");
        prop_assert_eq!(results.len(), 1);
        prop_assert_eq!(&results[0].content, &content);
        prop_assert!((results[0].confidence - confidence).abs() < 1e-10);
        prop_assert_eq!(results[0].tier, crate::knowledge::EpistemicTier::Inferred);
    }
}

// Entity merge invariants:
// - entity count drops by exactly 1
// - merged-from entity is gone
// - canonical entity survives
// - relationships are redirected (none reference the merged-from id)
// - relationship count does not increase (may decrease due to self-referential dedup)
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn execute_merge_invariants(
        n_entities in 2_usize..=20,
        rel_pairs in proptest::collection::vec(
            (0_usize..20, 0_usize..20),
            0..=30,
        ),
        canonical_idx in 0_usize..20,
        merged_idx in 0_usize..20,
    ) {
        let n = n_entities;
        let ci = canonical_idx % n;
        let mi = {
            let raw = merged_idx % n;
            if raw == ci { (ci + 1) % n } else { raw }
        };

        let store = make_store();

        for i in 0..n {
            store
                .insert_entity(&make_entity(
                    &format!("e{i}"),
                    &format!("Entity {i}"),
                    "concept",
                ))
                .expect("insert entity");
        }

        let relations = ["works_on", "knows", "depends_on", "uses"];
        for (si, di) in &rel_pairs {
            let src_i = si % n;
            let dst_i = di % n;
            // skip self-loops: {src, dst} is the compound key
            if src_i == dst_i {
                continue;
            }
            let rel_idx = (src_i + dst_i) % relations.len();
            store
                .insert_relationship(&make_relationship(
                    &format!("e{src_i}"),
                    &format!("e{dst_i}"),
                    relations[rel_idx],
                    0.7,
                ))
                .expect("insert relationship");
        }

        let count_rels = |s: &Arc<KnowledgeStore>| -> i64 {
            s.run_query(r"?[count(src)] := *relationships{src}", BTreeMap::new())
                .expect("count rels")
                .rows
                .first()
                .and_then(|r| r.first())
                .and_then(crate::engine::data::value::DataValue::get_int)
                .unwrap_or(0)
        };

        let rel_count_before = count_rels(&store);

        let canonical_id = crate::id::EntityId::new_unchecked(format!("e{ci}"));
        let merged_id = crate::id::EntityId::new_unchecked(format!("e{mi}"));

        store
            .execute_merge(&canonical_id, &merged_id)
            .expect("execute_merge must succeed");

        // 1. Entity count is N-1
        let entity_count_after = store
            .run_query(r"?[count(id)] := *entities{id}", BTreeMap::new())
            .expect("count entities after")
            .rows
            .first()
            .and_then(|r| r.first())
            .and_then(crate::engine::data::value::DataValue::get_int)
            .unwrap_or(0);
        prop_assert_eq!(
            entity_count_after,
            i64::try_from(n).expect("test value fits i64") - 1,
            "entity count must be N-1 after merge"
        );

        // 2. Merged-from entity is gone
        let mut check_params = BTreeMap::new();
        check_params.insert(
            "id".to_owned(),
            crate::engine::DataValue::Str(merged_id.as_str().into()),
        );
        let merged_rows = store
            .run_query(r"?[id] := *entities{id}, id = $id", check_params)
            .expect("check merged gone");
        prop_assert!(merged_rows.rows.is_empty(), "merged entity must be gone");

        // 3. Canonical entity survives
        let mut canon_params = BTreeMap::new();
        canon_params.insert(
            "id".to_owned(),
            crate::engine::DataValue::Str(canonical_id.as_str().into()),
        );
        let canon_rows = store
            .run_query(r"?[id] := *entities{id}, id = $id", canon_params)
            .expect("check canonical exists");
        prop_assert_eq!(canon_rows.rows.len(), 1, "canonical entity must survive");

        // 4. No relationship references the merged-from entity
        let mut ref_params = BTreeMap::new();
        ref_params.insert(
            "mid".to_owned(),
            crate::engine::DataValue::Str(merged_id.as_str().into()),
        );
        let orphan_rows = store
            .run_query(
                r"?[src, dst] := *relationships{src, dst}, (src = $mid or dst = $mid)",
                ref_params,
            )
            .expect("check no orphan edges");
        prop_assert!(
            orphan_rows.rows.is_empty(),
            "no relationship should reference the merged-from entity"
        );

        // 5. Relationship count does not increase; may decrease due to
        // self-referential dedup (canonical<->merged edges removed on redirect)
        let rel_count_after = count_rels(&store);
        prop_assert!(
            rel_count_after <= rel_count_before,
            "relationship count must not increase: before={rel_count_before}, after={rel_count_after}"
        );
    }
}

mod merge {
    use crate::engine::DataValue;
    use crate::id::EntityId;
    use crate::knowledge::{Entity, Relationship};
    use proptest::prelude::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::super::super::{KnowledgeConfig, KnowledgeStore};

    // WHY: small fixed set keeps entity IDs short and relationship types
    // valid without invoking vocab normalisation (storage layer accepts any string).
    const RELATION_TYPES: &[&str] = &["KNOWS", "WORKS_AT", "DEPENDS_ON", "USES", "PART_OF"];

    fn make_store() -> Arc<KnowledgeStore> {
        KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 })
            .expect("in-memory store should always open")
    }

    fn count_entities(store: &KnowledgeStore) -> usize {
        store
            .run_query("?[id] := *entities{id}", BTreeMap::new())
            .expect("entity count query")
            .rows
            .len()
    }

    fn entity_exists(store: &KnowledgeStore, id: &str) -> bool {
        let mut params = BTreeMap::new();
        params.insert("eid".to_owned(), DataValue::Str(id.into()));
        store
            .run_query("?[id] := *entities{id}, id = $eid", params)
            .expect("entity exists query")
            .rows
            .len()
            == 1
    }

    fn count_rels_touching(store: &KnowledgeStore, entity_id: &str) -> usize {
        let mut params = BTreeMap::new();
        params.insert("eid".to_owned(), DataValue::Str(entity_id.into()));
        store
            .run_query(
                "?[src, dst] := *relationships{src, dst}, (src = $eid or dst = $eid)",
                params,
            )
            .expect("relationships-touching query")
            .rows
            .len()
    }

    fn count_all_rels(store: &KnowledgeStore) -> usize {
        store
            .run_query("?[src, dst] := *relationships{src, dst}", BTreeMap::new())
            .expect("all-relationships count query")
            .rows
            .len()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Entity merge maintains structural invariants across random entity graphs.
        ///
        /// For any graph of N entities (2–20) with up to 30 directed edges,
        /// merging a randomly selected pair must:
        /// - reduce entity count to N-1
        /// - remove the merged-from entity
        /// - leave the surviving entity intact
        /// - redirect all edges away from the merged entity (no orphaned edges)
        /// - not increase the total edge count (deduplication may reduce it)
        #[test]
        fn execute_merge_maintains_invariants(
            n in 2_usize..=20,
            raw_rels in prop::collection::vec(
                (0_usize..20, 0_usize..20, 0_usize..5),
                0..=30,
            ),
            canonical_raw in 0_usize..20,
            merge_shift in 1_usize..20,
        ) {
            let canonical_idx = canonical_raw % n;
            // WHY: `1 + (merge_shift % (n-1))` is in 1..n-1, so adding it to
            // canonical_idx and wrapping modulo n can never yield canonical_idx
            // again: canonical and merged are always distinct.
            let merged_idx = (canonical_idx + 1 + (merge_shift % (n - 1))) % n;

            let store = make_store();
            let now = jiff::Timestamp::UNIX_EPOCH;

            for i in 0..n {
                let entity = Entity {
                    id: EntityId::new_unchecked(format!("e{i}")),
                    name: format!("entity-{i}"),
                    entity_type: "concept".to_owned(),
                    aliases: vec![],
                    created_at: now,
                    updated_at: now,
                };
                store.insert_entity(&entity).expect("insert entity");
            }

            for (s, d, rel_type_idx) in &raw_rels {
                let src_idx = s % n;
                let dst_idx = d % n;
                if src_idx == dst_idx {
                    continue;
                }
                let rel = Relationship {
                    src: EntityId::new_unchecked(format!("e{src_idx}")),
                    dst: EntityId::new_unchecked(format!("e{dst_idx}")),
                    relation: RELATION_TYPES[rel_type_idx % RELATION_TYPES.len()].to_owned(),
                    weight: 0.8,
                    created_at: now,
                };
                store.insert_relationship(&rel).expect("insert relationship");
            }

            let entity_count_before = count_entities(&store);
            prop_assert_eq!(
                entity_count_before, n,
                "entity count before merge must equal N"
            );
            let rel_count_before = count_all_rels(&store);

            let canonical_id = EntityId::new_unchecked(format!("e{canonical_idx}"));
            let merged_id = EntityId::new_unchecked(format!("e{merged_idx}"));

            store
                .execute_merge(&canonical_id, &merged_id)
                .expect("execute_merge should succeed on valid entity pair");

            // Invariant: entity count decreases by exactly 1
            let entity_count_after = count_entities(&store);
            prop_assert_eq!(
                entity_count_after,
                n - 1,
                "entity count after merge must be N-1"
            );

            // Invariant: merged entity no longer exists
            prop_assert!(
                !entity_exists(&store, &format!("e{merged_idx}")),
                "merged entity must not exist after merge"
            );

            // Invariant: canonical entity survives intact
            prop_assert!(
                entity_exists(&store, &format!("e{canonical_idx}")),
                "canonical entity must still exist after merge"
            );

            // Invariant: no orphaned edges: merged entity must have zero relationships
            let rels_touching_merged = count_rels_touching(&store, &format!("e{merged_idx}"));
            prop_assert_eq!(
                rels_touching_merged,
                0,
                "no relationship may reference the merged entity after merge"
            );

            // Invariant: relationship count does not increase (merge may deduplicate edges
            // when merged and canonical both had edges to the same third entity, or when
            // the merge would produce a self-loop)
            let rel_count_after = count_all_rels(&store);
            prop_assert!(
                rel_count_after <= rel_count_before,
                "relationship count must not increase after merge: before={rel_count_before}, after={rel_count_after}"
            );
        }
    }
}
