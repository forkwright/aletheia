//! Deterministic HNSW delete-disconnection regression tests (#6952).
//!
//! The intermittent exact-0.00 recall failure needed an unlucky per-process
//! RNG draw to reproduce through the public insert path, because unseeded
//! level assignment decides whether an upper-level edge happens to bridge
//! the severed region. These tests remove the randomness at its root: they
//! fabricate the index rows directly (the on-disk layout is shared by both
//! HNSW trees and documented on `runtime::hnsw::types`), so the exact
//! topology the investigation reconstructed from failing-run graph dumps —
//! a thickened path in id order, severed by a contiguous band — exists
//! by construction, every run, under either tree.
//!
//! Compiled once against whichever implementation `runtime::hnsw` resolves
//! to, so the default gate exercises the derived tree and the
//! `krites_sovereign_hnsw` job exercises the sovereign one — both trees
//! showed the same dips, and both carry the same two fixes:
//!
//! - `band_delete_around_entry_point_must_not_sever_level0`: the repair
//!   pass in `hnsw_remove_vec` — deleting a contiguous band through the
//!   public `:rm` path must leave the level-0 graph connected. `ef` is kept
//!   *below* both would-be island sizes so the search-side escape hatch
//!   cannot mask a missing repair.
//! - `search_escapes_a_severed_island_around_the_entry_point`: the
//!   search-side escape hatch — on a graph that is *already* severed (no
//!   delete runs, so no repair pass can help), a beam that exhausts the
//!   entry point's island below `ef` must go find the other component
//!   instead of returning its own island as the answer.

#![cfg(test)]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions over fixed-shape index rows"
)]

use std::collections::HashMap;

use crate::data::value::Vector;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;
use crate::{DataValue, DbInstance};

/// Collinear, unit-spaced coordinates: under squared-L2 the diversity
/// heuristic on a line keeps exactly `{i-1, i+1}` per node, so the level-0
/// graph is a plain path in id order — the sharpest possible version of the
/// "thickened path" topology the #6952 graph dumps show, with every
/// pairwise distance distinct (no hash-order-dependent ties anywhere).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "test fixture, small integers"
)]
fn vec_for(i: usize) -> [f32; 4] {
    [i as f32, 0.0, 0.0, 0.0]
}

fn setup_db(m: usize) -> DbInstance {
    let db = DbInstance::default();
    db.run_default(":create v { id: Int => vec: <F32; 4> }")
        .unwrap();
    db.run_default(&format!(
        r"::hnsw create v:idx {{
            dim: 4, m: {m}, dtype: F32, fields: [vec], distance: L2, ef_construction: 20,
            extend_candidates: false, keep_pruned_connections: false,
        }}"
    ))
    .unwrap();
    db
}

/// The vector field's tuple position in `v {{ id => vec }}` (key columns
/// first), i.e. the `fr__field`/`to__field` value every fabricated index
/// row carries.
const VEC_FIELD: i64 = 1;
const SUB_IDX: i64 = -1;

fn id_val(id: usize) -> DataValue {
    DataValue::from(i64::try_from(id).unwrap())
}

/// `[level, key, field, subidx, key, field, subidx]` — the `fr == to`
/// self-entry shape.
fn self_entry_key(level: i64, id: usize) -> Vec<DataValue> {
    let mut out = vec![DataValue::from(level)];
    for _ in 0..2 {
        out.push(id_val(id));
        out.push(DataValue::from(VEC_FIELD));
        out.push(DataValue::from(SUB_IDX));
    }
    out
}

/// `[level, from-half, to-half]` — a directed edge row's key.
fn edge_key(level: i64, from: usize, to: usize) -> Vec<DataValue> {
    let mut out = vec![DataValue::from(level)];
    for id in [from, to] {
        out.push(id_val(id));
        out.push(DataValue::from(VEC_FIELD));
        out.push(DataValue::from(SUB_IDX));
    }
    out
}

fn put_row(tx: &mut SessionTx<'_>, handle: &RelationHandle, key: &[DataValue], val: &[DataValue]) {
    let key_bytes = handle
        .encode_key_for_store(key, crate::SourceSpan::default())
        .unwrap();
    let val_bytes = handle
        .encode_val_only_for_store(val, crate::SourceSpan::default())
        .unwrap();
    tx.store_tx.put(&key_bytes, &val_bytes).unwrap();
}

/// Fabricate the base rows plus a hand-built level-0 index graph: one
/// self-entry per node, bidirectional path edges exactly along `edges`, the
/// designated entry node lifted to level -1 (making it the topmost live row
/// the argmin-id entry-point scan lands on), and the level-1 marker row.
fn fabricate_index(db: &DbInstance, n: usize, edges: &[(usize, usize)], entry: usize) {
    let mut degrees: HashMap<usize, f64> = HashMap::new();
    for &(a, b) in edges {
        *degrees.entry(a).or_insert(0.0) += 1.0;
        *degrees.entry(b).or_insert(0.0) += 1.0;
    }

    let mut tx = db.transact_write().unwrap();
    let base = tx.get_relation("v", false).unwrap();
    let (idx_handle, _manifest) = base.hnsw_indices.get("idx").unwrap().clone();

    for i in 0..n {
        let v = vec_for(i);
        put_row(
            &mut tx,
            &base,
            &[id_val(i)],
            &[DataValue::Vec(Vector::F32(ndarray::Array1::from_vec(
                v.to_vec(),
            )))],
        );
        put_row(
            &mut tx,
            &idx_handle,
            &self_entry_key(0, i),
            &[
                DataValue::from(*degrees.get(&i).unwrap_or(&0.0)),
                DataValue::Null,
                DataValue::from(false),
            ],
        );
    }

    for &(a, b) in edges {
        let dist = f64::from(vec_for(a)[0] - vec_for(b)[0]).powi(2);
        for (from, to) in [(a, b), (b, a)] {
            put_row(
                &mut tx,
                &idx_handle,
                &edge_key(0, from, to),
                &[
                    DataValue::from(dist),
                    DataValue::Null,
                    DataValue::from(false),
                ],
            );
        }
    }

    // The entry node's top level is -1: a lone upper-level self-entry with
    // no upper-level edges, matching the confined shape ("new entry id @ -1,
    // live outdeg 0") every instrumented failing run showed.
    put_row(
        &mut tx,
        &idx_handle,
        &self_entry_key(-1, entry),
        &[
            DataValue::from(0.0),
            DataValue::Null,
            DataValue::from(false),
        ],
    );

    // The level-1 marker row (the canary node): present for parity with a
    // put-built index; search re-derives the entry point by scan and never
    // reads it.
    let mut marker_key = vec![DataValue::from(1_i64)];
    for _ in 0..6 {
        marker_key.push(DataValue::Null);
    }
    put_row(
        &mut tx,
        &idx_handle,
        &marker_key,
        &[
            DataValue::from(-1_i64),
            DataValue::Bytes(vec![]),
            DataValue::from(false),
        ],
    );

    tx.commit_tx().unwrap();
}

fn search(db: &DbInstance, query: [f32; 4], k: usize, ef: usize) -> Vec<i64> {
    let res = db
        .run_default(&format!(
            "?[id, dist] := ~v:idx{{id | query: vec([{},{},{},{}]), k: {k}, ef: {ef}, bind_distance: dist}} :order dist",
            query[0], query[1], query[2], query[3]
        ))
        .unwrap();
    res.rows.iter().filter_map(|r| r[0].get_int()).collect()
}

/// Union-find `find` with path compression, over a parent map keyed by id.
fn uf_find(parent: &mut HashMap<i64, i64>, x: i64) -> i64 {
    let p = parent[&x];
    if p == x {
        return x;
    }
    let root = uf_find(parent, p);
    parent.insert(x, root);
    root
}

/// Connected components of the live level-0 graph, by scanning the index
/// relation the same way the traversal does: edge rows (`fr != to`) whose
/// deletion flag is unset.
fn level0_component_count(db: &DbInstance, survivors: &[usize]) -> usize {
    let tx = db.transact().unwrap();
    let base = tx.get_relation("v", false).unwrap();
    let (idx_handle, _manifest) = base.hnsw_indices.get("idx").unwrap().clone();

    let mut parent: HashMap<i64, i64> = survivors
        .iter()
        .map(|&s| {
            let s = i64::try_from(s).unwrap();
            (s, s)
        })
        .collect();

    for row in idx_handle.scan_prefix(&tx, &vec![DataValue::from(0_i64)]) {
        let row = row.unwrap();
        let (fr, to) = (row[1].get_int().unwrap(), row[4].get_int().unwrap());
        let deleted = row[9].get_bool().unwrap();
        if fr == to || deleted {
            continue;
        }
        assert!(
            parent.contains_key(&fr) && parent.contains_key(&to),
            "live level-0 edge {fr}->{to} references a deleted node"
        );
        let (fr_root, to_root) = (uf_find(&mut parent, fr), uf_find(&mut parent, to));
        if fr_root != to_root {
            parent.insert(fr_root, to_root);
        }
    }

    let mut roots: Vec<i64> = survivors
        .iter()
        .map(|&s| uf_find(&mut parent, i64::try_from(s).unwrap()))
        .collect();
    roots.sort_unstable();
    roots.dedup();
    roots.len()
}

/// #6952 half A (the repair pass): deleting a contiguous band of the entry
/// point's nearest neighbours through the public `:rm` path must leave the
/// level-0 graph in one piece.
///
/// On a path graph every band member's only paths run through the band, so
/// without a repair pass the delete provably severs the graph into
/// `{0..14}` and `{25..39}` — this is the severing scenario from the issue
/// with the RNG dependence removed. `ef: 10` stays below both island sizes
/// (15), so the search assertion cannot be rescued by the search-side
/// escape hatch: only real reconnection makes the far side reachable.
#[test]
fn band_delete_around_entry_point_must_not_sever_level0() {
    let n = 40;
    let db = setup_db(4);
    let edges: Vec<(usize, usize)> = (0..n - 1).map(|i| (i, i + 1)).collect();
    fabricate_index(&db, n, &edges, 0);

    for id in 15..=24 {
        db.run_default(&format!("?[id] <- [[{id}]] :rm v {{}}"))
            .unwrap();
    }
    let survivors: Vec<usize> = (0..15).chain(25..n).collect();

    assert_eq!(
        level0_component_count(&db, &survivors),
        1,
        "deleting a contiguous band must not sever the level-0 graph: \
         hnsw_remove_vec's repair pass has to reconnect each removed node's \
         surviving neighbours (#6952)"
    );

    let ids = search(&db, vec_for(30), 5, 10);
    assert!(
        ids.contains(&30),
        "query across the deleted band missed its own exact match (got {ids:?}): \
         the entry point's side of the cut is walled off from the query's side (#6952)"
    );
    for id in &ids {
        assert!(
            !(15..=24).contains(id),
            "deleted id {id} reappeared in search results"
        );
    }
}

/// #6952 half B (entry-point hardening): on an index whose level-0 graph is
/// *already* severed — fabricated directly, so no delete ever runs and the
/// repair pass never gets a chance — the argmin-id entry point sits in the
/// 10-node island `{0..9}` while the query's targets live in `{10..49}`.
///
/// The level-0 beam exhausts the island with `found_nn` far below `ef`,
/// which proves (by the expansion rule: every neighbour is pushed while the
/// beam is under `ef`, and nothing is evicted below `ef`) that the seeds'
/// components are exhausted. The escape hatch must then seed the unreached
/// component instead of presenting 10 wrong ids as the answer — the exact
/// confinement that measured as recall 0.00.
#[test]
fn search_escapes_a_severed_island_around_the_entry_point() {
    let n = 50;
    let db = setup_db(4);
    let edges: Vec<(usize, usize)> = (0..n - 1).filter(|&i| i != 9).map(|i| (i, i + 1)).collect();
    fabricate_index(&db, n, &edges, 0);

    let ids = search(&db, vec_for(30), 5, 20);
    assert!(
        ids.contains(&30),
        "search stayed confined to the entry point's severed island (got {ids:?}): \
         a beam that ends below ef has exhausted its component and must seed the \
         unreached one (#6952)"
    );
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![28, 29, 30, 31, 32],
        "the true top-5 neighbours of the query all live outside the island"
    );
}
