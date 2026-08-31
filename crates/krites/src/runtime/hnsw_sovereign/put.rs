//! `SessionTx` methods for HNSW vector insertion.
//!
//! - [`SessionTx::hnsw_put`]: public entry point — extracts vector(s) from a
//!   tuple, enforces `max_vectors`, applies the index filter, and dispatches
//!   to [`SessionTx::hnsw_put_vector`] per extracted vector.
//! - [`SessionTx::hnsw_put_vector`]: inserts one vector by drawing a random
//!   level, greedily descending from the entry point through the upper
//!   layers, then at the target layers running the construction beam width
//!   to find and wire neighbour connections.
//! - [`SessionTx::hnsw_put_fresh_at_levels`]: initializes a brand-new node
//!   (no prior entry point, or a node whose level exceeds the current top)
//!   and — in both cases — repoints the entry-point marker (the canary
//!   node) at it.
//!
//! Graph traversal and neighbour selection live in the sibling [`super::graph`]
//! module.

use std::cmp::{Reverse, max};

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use tracing::warn;

use super::idx_to_i64;
use super::types::{
    CompoundKey, DEFAULT_VECTOR_CACHE_CAPACITY, HnswIndexManifest, VectorCache, decode_edge_value,
    edge_key, entry_point_key, scan_indexed_keys, self_entry_key,
};
use crate::DataValue;
use crate::data::expr::{Bytecode, eval_bytecode_pred};
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl SessionTx<'_> {
    /// Insert one vector into the graph.
    ///
    /// If a level-0 self-entry already exists for this compound key and its
    /// stored content hash matches, the insert is a no-op (content-addressed
    /// dedup). If it exists with a different hash, the stale entry is fully
    /// removed first, then reinserted as new.
    ///
    /// # Complexity
    ///
    /// `O(log n * ef_construction * m)`.
    fn hnsw_put_vector(
        &mut self,
        tuple: &[DataValue],
        q: &Vector,
        idx: usize,
        subidx: i32,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        let key_len = orig_table.metadata.keys.len();
        let tuple_key = &tuple[..key_len];
        vec_cache.insert((tuple_key.to_vec(), idx, subidx), q.clone());
        let hash = q.get_hash();

        let existence_key = self_entry_key(0, tuple_key, idx, subidx);
        if let Some(existing) = idx_table.get(self, &existence_key)? {
            if let DataValue::Bytes(b) = &existing[key_len * 2 + 6]
                && b == hash.as_ref()
            {
                return Ok(());
            }
            self.hnsw_remove_vec(tuple_key, idx, subidx, manifest, orig_table, idx_table)?;
        }

        let Some(ep) = idx_table
            .scan_bounded_prefix(
                self,
                &[],
                &[DataValue::from(i64::MIN)],
                &[DataValue::from(0)],
            )
            .next()
            .transpose()?
        else {
            // Empty index: this vector becomes the sole node and the entry point.
            let level = manifest.get_random_level();
            return self.hnsw_put_fresh_at_levels(
                hash.as_ref(),
                tuple_key,
                idx,
                subidx,
                orig_table,
                idx_table,
                level,
                0,
            );
        };

        let bottom_level = ep[0].get_int().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_read",
                reason: "entry point level is not an integer".to_string(),
            }
            .build()
        })?;
        let ep_field_raw = ep[key_len + 1].get_int().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_read",
                reason: "entry point field is not an integer".to_string(),
            }
            .build()
        })?;
        let ep_field = usize::try_from(ep_field_raw).map_err(|_e| {
            InvalidOperationSnafu {
                op: "hnsw_read",
                reason: "entry point field out of range",
            }
            .build()
        })?;
        let ep_subidx_raw = ep[key_len + 2].get_int().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_read",
                reason: "entry point sub-index is not an integer".to_string(),
            }
            .build()
        })?;
        let ep_subidx = i32::try_from(ep_subidx_raw).map_err(|_e| {
            InvalidOperationSnafu {
                op: "hnsw_read",
                reason: "entry point sub-index out of range",
            }
            .build()
        })?;
        let ep_key: CompoundKey = (ep[1..key_len + 1].to_vec(), ep_field, ep_subidx);
        vec_cache.ensure_key(&ep_key, orig_table, self)?;
        let mut found_nn = PriorityQueue::new();
        found_nn.push(ep_key.clone(), OrderedFloat(vec_cache.v_dist(q, &ep_key)?));

        let target_level = manifest.get_random_level();
        if target_level < bottom_level {
            self.hnsw_put_fresh_at_levels(
                hash.as_ref(),
                tuple_key,
                idx,
                subidx,
                orig_table,
                idx_table,
                target_level,
                bottom_level - 1,
            )?;
        }

        for level in bottom_level..target_level {
            self.hnsw_search_level(q, 1, level, orig_table, idx_table, &mut found_nn, vec_cache)?;
        }

        let target: CompoundKey = (tuple_key.to_vec(), idx, subidx);
        for level in max(target_level, bottom_level)..=0 {
            let m_max = if level == 0 {
                manifest.m_max0
            } else {
                manifest.m_max
            };
            self.hnsw_search_level(
                q,
                manifest.ef_construction,
                level,
                orig_table,
                idx_table,
                &mut found_nn,
                vec_cache,
            )?;
            let neighbours = self.hnsw_select_neighbours_heuristic(
                q, &found_nn, m_max, level, manifest, idx_table, orig_table, vec_cache,
            )?;
            self.hnsw_write_self_entry(level, &target, neighbours.len(), hash.as_ref(), idx_table)?;
            for (neighbour, Reverse(OrderedFloat(dist))) in &neighbours {
                self.hnsw_connect_at_level(
                    level, &target, neighbour, *dist, m_max, manifest, idx_table, orig_table,
                    vec_cache,
                )?;
            }
        }
        Ok(())
    }

    /// Write (or overwrite) `target`'s self-entry at `level`: `[level,
    /// target.., target..] -> [degree, hash, false]`.
    fn hnsw_write_self_entry(
        &mut self,
        level: i64,
        target: &CompoundKey,
        degree: usize,
        hash: &[u8],
        idx_table: &RelationHandle,
    ) -> Result<()> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "HNSW degree bounded by m_max (< 2^53)"
        )]
        let val = [
            DataValue::from(degree as f64),
            DataValue::Bytes(hash.to_vec()),
            DataValue::from(false),
        ];
        let key = self_entry_key(level, &target.0, target.1, target.2);
        let key_bytes = idx_table.encode_key_for_store(&key, Default::default())?;
        let val_bytes = idx_table.encode_val_only_for_store(&val, Default::default())?;
        self.store_tx.put(&key_bytes, &val_bytes)?;
        Ok(())
    }

    /// Wire `target -> neighbour` and `neighbour -> target` (both live) at
    /// `level`, then bump `neighbour`'s own self-entry degree — shrinking
    /// `neighbour`'s outbound adjacency first if that pushes it past
    /// `m_max`. (`pub(super)`: also the edge-writing primitive for
    /// [`super::remove`]'s post-delete repair pass, #6952.)
    pub(super) fn hnsw_connect_at_level(
        &mut self,
        level: i64,
        target: &CompoundKey,
        neighbour: &CompoundKey,
        dist: f64,
        m_max: usize,
        manifest: &HnswIndexManifest,
        idx_table: &RelationHandle,
        orig_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        let edge_val = [
            DataValue::from(dist),
            DataValue::Null,
            DataValue::from(false),
        ];
        let edge_val_bytes = idx_table.encode_val_only_for_store(&edge_val, Default::default())?;
        for (from, to) in [(target, neighbour), (neighbour, target)] {
            let key_bytes =
                idx_table.encode_key_for_store(&edge_key(level, from, to), Default::default())?;
            self.store_tx.put(&key_bytes, &edge_val_bytes)?;
        }

        let neighbour_self = self_entry_key(level, &neighbour.0, neighbour.1, neighbour.2);
        let neighbour_self_bytes =
            idx_table.encode_key_for_store(&neighbour_self, Default::default())?;
        let existing = self
            .store_tx
            .get(&neighbour_self_bytes, false)?
            .ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_index",
                    reason: "neighbour self-entry missing during connect — index is corrupted"
                        .to_string(),
                }
                .build()
            })?;
        let mut neighbour_val = decode_edge_value(&existing)?;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "degree is a small non-negative integer stored as f64"
        )]
        let mut degree = neighbour_val[0].get_float().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_index",
                reason: "neighbour degree is not a float".to_string(),
            }
            .build()
        })? as usize
            + 1;
        if degree > m_max {
            degree = self.hnsw_shrink_neighbour(
                neighbour, m_max, level, manifest, idx_table, orig_table, vec_cache,
            )?;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "HNSW degree bounded by m_max (< 2^53)"
        )]
        {
            neighbour_val[0] = DataValue::from(degree as f64);
        }
        let neighbour_val_bytes =
            idx_table.encode_val_only_for_store(&neighbour_val, Default::default())?;
        self.store_tx
            .put(&neighbour_self_bytes, &neighbour_val_bytes)?;
        Ok(())
    }

    /// Populate a brand-new node's self-entries from `bottom_level` through
    /// `top_level` (inclusive, both `<= 0`) with zero neighbours, and
    /// repoint the entry-point marker (the canary node) at it.
    ///
    /// Called both when the index is empty (levels `random..=0`) and when
    /// an insert's drawn level exceeds the current top (levels
    /// `random..=old_top - 1`, extending the graph upward with a lone new
    /// top node before the main insertion loop connects the rest).
    ///
    /// WARNING: the canary's opaque back-reference (value slot 1) encodes
    /// the fresh node's self-entry key *before* its level placeholder is
    /// filled in — it is write-only bookkeeping, never decoded back into a
    /// lookup, so this is harmless; it is called out because byte-compat
    /// requires reproducing it exactly, not "correcting" it.
    fn hnsw_put_fresh_at_levels(
        &mut self,
        hash: &[u8],
        tuple_key: &[DataValue],
        idx: usize,
        subidx: i32,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        bottom_level: i64,
        top_level: i64,
    ) -> Result<()> {
        let key_len = orig_table.metadata.keys.len();
        let mut self_key = vec![DataValue::Null];
        for _ in 0..2 {
            self_key.extend_from_slice(tuple_key);
            self_key.push(DataValue::from(idx_to_i64(idx)));
            self_key.push(DataValue::from(i64::from(subidx)));
        }
        let self_val = [
            DataValue::from(0.0),
            DataValue::Bytes(hash.to_vec()),
            DataValue::from(false),
        ];

        let opaque_ref = idx_table.encode_key_for_store(&self_key, Default::default())?;
        let entry_point_val = [
            DataValue::from(bottom_level),
            DataValue::Bytes(opaque_ref),
            DataValue::from(false),
        ];
        let entry_point_key_bytes =
            idx_table.encode_key_for_store(&entry_point_key(key_len), Default::default())?;
        let entry_point_val_bytes =
            idx_table.encode_val_only_for_store(&entry_point_val, Default::default())?;
        self.store_tx
            .put(&entry_point_key_bytes, &entry_point_val_bytes)?;

        for level in bottom_level..=top_level {
            self_key[0] = DataValue::from(level);
            let key_bytes = idx_table.encode_key_for_store(&self_key, Default::default())?;
            let val_bytes = idx_table.encode_val_only_for_store(&self_val, Default::default())?;
            self.store_tx.put(&key_bytes, &val_bytes)?;
        }
        Ok(())
    }

    /// Count vectors currently in the index (for `max_vectors` accounting).
    ///
    /// NOTE: the derived implementation counted rows with a `level = 1`
    /// prefix — but level 1 holds only the single entry-point marker row
    /// (see [`super::types::ENTRY_POINT_LEVEL`]), never a per-vector row, so
    /// that scan always returned 0 or 1 regardless of index size. Fixed
    /// here via [`scan_indexed_keys`], which enumerates real per-vector
    /// self-entries.
    fn hnsw_count_vectors(&self, orig_table: &RelationHandle, idx_table: &RelationHandle) -> usize {
        scan_indexed_keys(self, orig_table, idx_table).count()
    }

    /// Public entry point for HNSW insertion.
    ///
    /// Applies the index filter (removing a now-disqualified tuple from the
    /// index rather than inserting it), enforces `max_vectors`, extracts
    /// every vector field named by `manifest.vec_fields` (a plain vector
    /// field, or each vector inside a `List` field), and inserts each one.
    /// Returns `true` if anything was inserted, `false` if the tuple was
    /// filtered out or carried no vector fields.
    ///
    /// # Complexity
    ///
    /// `O(v * log n * ef_construction * m)`, `v` = vectors in this tuple.
    pub(crate) fn hnsw_put(
        &mut self,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        filter: Option<&Vec<Bytecode>>,
        stack: &mut Vec<DataValue>,
        tuple: &[DataValue],
    ) -> Result<bool> {
        if let Some(code) = filter
            && !eval_bytecode_pred(code, tuple, stack, Default::default())?
        {
            self.hnsw_remove(manifest, orig_table, idx_table, tuple)?;
            return Ok(false);
        }

        // WHY: caps unbounded memory/disk growth (#1722). Warn at 80%
        // utilisation, reject once the cap is reached.
        if let Some(max_cap) = manifest.max_vectors {
            let current = self.hnsw_count_vectors(orig_table, idx_table);
            if current >= max_cap {
                return Err(InvalidOperationSnafu {
                    op: "hnsw_put",
                    reason: format!(
                        "HNSW index '{}' is at capacity ({current}/{max_cap}): increase \
                         max_vectors or prune old vectors",
                        manifest.index_name
                    ),
                }
                .build()
                .into());
            }
            let warn_threshold = max_cap * 4 / 5; // 80%
            if current >= warn_threshold {
                // CodeQL: cleartext-logging false positive — index_name,
                // current, and max_cap are internal HNSW metadata (a table
                // name and integer counters), not credentials or
                // user-supplied sensitive data.
                warn!(
                    index = %manifest.index_name,
                    current,
                    max_cap,
                    "HNSW index approaching max_vectors capacity"
                );
            }
        }

        let mut extracted = vec![];
        for &field_idx in &manifest.vec_fields {
            let val = tuple.get(field_idx).ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_put",
                    reason: format!(
                        "vector field index {field_idx} out of bounds (tuple len {})",
                        tuple.len()
                    ),
                }
                .build()
            })?;
            match val {
                DataValue::Vec(v) => extracted.push((v, field_idx, -1)),
                DataValue::List(l) => {
                    for (sub, v) in l.iter().enumerate() {
                        if let DataValue::Vec(v) = v {
                            #[expect(
                                clippy::cast_possible_truncation,
                                clippy::cast_possible_wrap,
                                reason = "HNSW layer indices bounded by m_max (< i32::MAX)"
                            )]
                            let sub_i32 = sub as i32;
                            extracted.push((v, field_idx, sub_i32));
                        }
                    }
                }
                _ => {}
            }
        }
        if extracted.is_empty() {
            return Ok(false);
        }

        let mut vec_cache = VectorCache::new(manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);
        for (vec, field_idx, sub) in extracted {
            self.hnsw_put_vector(
                tuple,
                vec,
                field_idx,
                sub,
                manifest,
                orig_table,
                idx_table,
                &mut vec_cache,
            )?;
        }
        Ok(true)
    }

    /// Orphan-consistency check (E24): every level-0 self-entry names a
    /// vector the index believes is live; confirm each still has a
    /// corresponding row in `orig_table`. Returns the orphan count, and
    /// logs each orphan at `warn` — an orphan means an embedding write
    /// failed after (or without) a matching `hnsw_remove`, and the fix is
    /// an index rebuild.
    ///
    /// Ready for a periodic maintenance scheduler to call; wiring that
    /// schedule is outside this module (`runtime/hnsw*` has no scheduler of
    /// its own — the derived code's own `#[expect(dead_code)]` on this
    /// function reflects the same gap, not a design choice this rewrite
    /// changes).
    ///
    /// NOTE: the derived implementation scanned the `level = 1`
    /// entry-point-marker row instead of real per-vector entries — the same
    /// miscount as `hnsw_count_vectors` above — so it always reported
    /// exactly one false orphan (or zero, on an empty index) and never
    /// actually checked a real vector. Fixed here via [`scan_indexed_keys`].
    ///
    /// # Complexity
    ///
    /// `O(n)`: one canary scan plus one base-relation lookup per vector.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "maintenance entry point — exercised directly by tests; \
                                     no in-crate scheduler to call it in production yet"
        )
    )]
    pub(crate) fn hnsw_check_consistency(
        &self,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<usize> {
        let mut orphans = 0usize;
        for key in scan_indexed_keys(self, orig_table, idx_table) {
            match orig_table.get(self, &key.0) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    orphans += 1;
                    warn!(
                        index = %manifest.index_name,
                        base_relation = %manifest.base_relation,
                        orphans,
                        "HNSW index entry has no corresponding fact in base relation \
                         (embedding failure or incomplete write) — run index rebuild to repair"
                    );
                }
                Err(e) => {
                    warn!(
                        index = %manifest.index_name,
                        error = %e,
                        "I/O error scanning base relation during orphan check — skipping entry"
                    );
                }
            }
        }
        Ok(orphans)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use crate::DataValue;
    use crate::DbInstance;

    /// 4-dim F32/L2 HNSW index named `idx` on `vectors { id: Int => vec: <F32; 4> }`.
    fn setup_db() -> DbInstance {
        let db = DbInstance::default();
        db.run_default(":create vectors { id: Int => vec: <F32; 4> }")
            .unwrap();
        db.run_default(
            r"::hnsw create vectors:idx {
                dim: 4, m: 16, dtype: F32, fields: [vec], distance: L2,
                ef_construction: 50, extend_candidates: false, keep_pruned_connections: false,
            }",
        )
        .unwrap();
        db
    }

    fn insert_vectors(db: &DbInstance, n: usize) {
        for i in 0..n {
            #[expect(
                clippy::cast_precision_loss,
                reason = "test fixture with small integers"
            )]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
    }

    #[test]
    fn put_single_vector_is_retrievable() {
        let db = setup_db();
        db.run_default("?[id, vec] <- [[42, vec([1.0, 2.0, 3.0, 4.0])]] :put vectors {}")
            .unwrap();
        let res = db
            .run_default("?[id, vec] := *vectors{id, vec}, id = 42")
            .unwrap();
        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0][0].get_int().unwrap(), 42);
    }

    #[test]
    fn put_multiple_vectors_all_retrievable() {
        let db = setup_db();
        insert_vectors(&db, 10);
        let res = db.run_default("?[id] := *vectors{id}").unwrap();
        assert_eq!(res.rows.len(), 10);
    }

    #[test]
    fn put_duplicate_key_is_idempotent() {
        let db = setup_db();
        for _ in 0..2 {
            db.run_default("?[id, vec] <- [[1, vec([1.0, 1.0, 1.0, 1.0])]] :put vectors {}")
                .unwrap();
        }
        let res = db.run_default("?[id] := *vectors{id}").unwrap();
        assert_eq!(
            res.rows.len(),
            1,
            "duplicate insert must not create extra rows"
        );
    }

    #[test]
    fn put_updated_vector_replaces_old() {
        let db = setup_db();
        db.run_default("?[id, vec] <- [[7, vec([0.0, 0.0, 0.0, 0.0])]] :put vectors {}")
            .unwrap();
        db.run_default("?[id, vec] <- [[7, vec([9.0, 9.0, 9.0, 9.0])]] :put vectors {}")
            .unwrap();
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([9.0, 9.0, 9.0, 9.0]), k: 1, ef: 50, bind_distance: dist}",
            )
            .unwrap();
        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0][0].get_int().unwrap(), 7);
        assert!(res.rows[0][1].get_float().unwrap() < 1e-6);
    }

    #[test]
    fn put_empty_index_search_returns_nothing() {
        let db = setup_db();
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([1.0, 2.0, 3.0, 4.0]), k: 5, ef: 50, bind_distance: dist}",
            )
            .unwrap();
        assert!(res.rows.is_empty());
    }

    #[test]
    fn dense_insert_preserves_connectivity_after_shrink() {
        let db = setup_db();
        insert_vectors(&db, 100);
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([50.0, 50.0, 50.0, 50.0]), k: 5, ef: 50, bind_distance: dist}",
            )
            .unwrap();
        assert!(
            !res.rows.is_empty(),
            "graph must remain searchable after shrink events"
        );
        assert!(res.rows.len() <= 5);
    }

    #[test]
    fn every_inserted_vector_has_a_base_relation_row() {
        let db = setup_db();
        insert_vectors(&db, 15);
        let res = db.run_default("?[id] := *vectors{id}").unwrap();
        assert_eq!(res.rows.len(), 15);
    }

    #[test]
    fn deleted_vector_absent_from_index() {
        let db = setup_db();
        insert_vectors(&db, 10);
        db.run_default("?[id] <- [[5]] :rm vectors {}").unwrap();
        let search = db
            .run_default(
                r"?[id] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 10, ef: 50, bind_distance: _dist}",
            )
            .unwrap();
        let ids: Vec<i64> = search.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(!ids.contains(&5));
    }

    #[test]
    fn rebuilt_index_is_searchable() {
        let db = setup_db();
        insert_vectors(&db, 20);
        db.run_default("::hnsw drop vectors:idx").unwrap();
        db.run_default(
            r"::hnsw create vectors:idx {
                dim: 4, m: 16, dtype: F32, fields: [vec], distance: L2,
                ef_construction: 50, extend_candidates: false, keep_pruned_connections: false,
            }",
        )
        .unwrap();
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([10.0, 10.0, 10.0, 10.0]), k: 3, ef: 50, bind_distance: dist}",
            )
            .unwrap();
        assert!(!res.rows.is_empty());
    }

    /// E04: exactly one entry-point marker row exists, keyed `level = 1`
    /// with every field `Null` on both halves — regardless of how many
    /// vectors have been inserted.
    #[test]
    fn exactly_one_all_null_entry_point_marker() {
        let db = setup_db();
        insert_vectors(&db, 30);

        let tx = db.transact().unwrap();
        let base = tx.get_relation("vectors", false).unwrap();
        let (idx_handle, _manifest) = base.hnsw_indices.get("idx").unwrap().clone();
        let key_len = base.metadata.keys.len();

        let rows: Vec<_> = idx_handle
            .scan_prefix(&tx, &vec![DataValue::from(1_i64)])
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            rows.len(),
            1,
            "exactly one entry-point marker row must exist"
        );
        let row = &rows[0];
        assert_eq!(row[0], DataValue::from(1_i64));
        for field in &row[1..2 * key_len + 5] {
            assert_eq!(
                *field,
                DataValue::Null,
                "every tuple-key field must be Null"
            );
        }
    }

    /// E04, continued: the marker survives across deletes — its identity
    /// (level=1, all-Null) never changes even as the entry point it points
    /// at is replaced.
    #[test]
    fn entry_point_marker_survives_deletes() {
        let db = setup_db();
        insert_vectors(&db, 30);
        db.run_default("?[id] <- [[0],[1],[2],[3],[4]] :rm vectors {}")
            .unwrap();

        let tx = db.transact().unwrap();
        let base = tx.get_relation("vectors", false).unwrap();
        let (idx_handle, _manifest) = base.hnsw_indices.get("idx").unwrap().clone();
        let rows: Vec<_> = idx_handle
            .scan_prefix(&tx, &vec![DataValue::from(1_i64)])
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            rows.len(),
            1,
            "the marker must still be exactly one row after deletes"
        );
    }

    /// E24: `hnsw_check_consistency` must find a manufactured orphan (a
    /// base-relation row removed without going through `hnsw_remove`) and
    /// report zero once the index is rebuilt.
    #[test]
    fn check_consistency_detects_manufactured_orphan() {
        let db = setup_db();
        insert_vectors(&db, 5);

        {
            let mut tx = db.transact_write().unwrap();
            let base = tx.get_relation("vectors", false).unwrap();
            let (idx_handle, manifest) = base.hnsw_indices.get("idx").unwrap().clone();
            // Bypass hnsw_remove entirely: delete the base row directly, as
            // if an embedding write had failed out from under an already-
            // indexed vector.
            let key = vec![DataValue::from(2_i64)];
            let encoded = base
                .encode_key_for_store(&key, crate::SourceSpan::default())
                .unwrap();
            tx.store_tx.del(&encoded).unwrap();
            let orphans = tx
                .hnsw_check_consistency(&manifest, &base, &idx_handle)
                .unwrap();
            assert_eq!(
                orphans, 1,
                "the manually-deleted row must be detected as an orphan"
            );
            tx.commit_tx().unwrap();
        }

        db.run_default("::hnsw drop vectors:idx").unwrap();
        db.run_default(
            r"::hnsw create vectors:idx {
                dim: 4, m: 16, dtype: F32, fields: [vec], distance: L2,
                ef_construction: 50, extend_candidates: false, keep_pruned_connections: false,
            }",
        )
        .unwrap();
        let tx = db.transact().unwrap();
        let base = tx.get_relation("vectors", false).unwrap();
        let (idx_handle, manifest) = base.hnsw_indices.get("idx").unwrap().clone();
        let orphans = tx
            .hnsw_check_consistency(&manifest, &base, &idx_handle)
            .unwrap();
        assert_eq!(orphans, 0, "a fresh rebuild must carry no orphans");
    }

    /// The fixed `max_vectors` basis: count must track the real number of
    /// indexed vectors. (The derived implementation's counter scanned the
    /// entry-point marker's reserved level, so it always read 0 or 1
    /// regardless of how many vectors were actually indexed — the same
    /// miscount `check_consistency_detects_manufactured_orphan` exercises
    /// on the sibling function.)
    #[test]
    fn count_vectors_reflects_real_index_size() {
        let db = setup_db();
        insert_vectors(&db, 7);
        let tx = db.transact().unwrap();
        let base = tx.get_relation("vectors", false).unwrap();
        let (idx_handle, _manifest) = base.hnsw_indices.get("idx").unwrap().clone();
        assert_eq!(tx.hnsw_count_vectors(&base, &idx_handle), 7);
    }
}
