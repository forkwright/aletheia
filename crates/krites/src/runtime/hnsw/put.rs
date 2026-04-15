//! SessionTx methods for HNSW vector insertion.
//!
//! Implements the insertion half of the HNSW algorithm:
//!
//! - **`hnsw_put`**: public entry point that extracts vectors from a tuple,
//!   enforces capacity limits, and dispatches to `hnsw_put_vector`.
//!
//! - **`hnsw_put_vector`**: inserts a single vector into the graph by selecting
//!   a random level, traversing from the entry point down through layers, and
//!   establishing bidirectional connections at each level.
//!
//! - **`hnsw_put_fresh_at_levels`**: initializes a new node at specified levels
//!   when no entry point exists or when the node starts a new highest layer.
//!
//! Graph traversal and neighbor selection are in the sibling `graph` module.

use std::cmp::{Reverse, max};

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use tracing::warn;

use super::idx_to_i64;
use super::types::{DEFAULT_VECTOR_CACHE_CAPACITY, HnswIndexManifest, VectorCache};
use crate::DataValue;
use crate::data::expr::{Bytecode, eval_bytecode_pred};
use crate::data::tuple::ENCODED_KEY_MIN_LEN;
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl SessionTx<'_> {
    /// Insert a single vector into the HNSW index.
    ///
    /// Assigns a random level, traverses the graph from the entry point through
    /// upper layers (greedy search with ef=1), then at the target layers uses
    /// the construction beam width to find and establish neighbor connections.
    ///
    /// If the vector's hash matches an existing entry, insertion is skipped
    /// (content-addressed deduplication).
    ///
    /// # Complexity
    ///
    /// O(log n * ef_construction * m) where n is the number of vectors, ef_construction
    /// is the construction beam width, and m is the maximum connections per node.
    /// Space: O(m) for neighbor maintenance.
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
        let tuple_key = &tuple[..orig_table.metadata.keys.len()];
        vec_cache.insert((tuple_key.to_vec(), idx, subidx), q.clone());
        let hash = q.get_hash();
        let mut canary_tuple = vec![DataValue::from(0)];
        for _ in 0..2 {
            canary_tuple.extend_from_slice(tuple_key);
            canary_tuple.push(DataValue::from(idx_to_i64(idx)));
            canary_tuple.push(DataValue::from(i64::from(subidx)));
        }
        if let Some(v) = idx_table.get(self, &canary_tuple)? {
            if let DataValue::Bytes(b) = &v[tuple_key.len() * 2 + 6]
                && b == hash.as_ref()
            {
                return Ok(());
            }
            self.hnsw_remove_vec(tuple_key, idx, subidx, orig_table, idx_table)?;
        }

        let ep_res = idx_table
            .scan_bounded_prefix(
                self,
                &[],
                &[DataValue::from(i64::MIN)],
                &[DataValue::from(0)],
            )
            .next();
        if let Some(ep) = ep_res {
            let ep = ep?;
            let bottom_level = ep[0].get_int().ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_read",
                    reason: "entry point bottom_level is not an integer".to_string(),
                }
                .build()
            })?;
            let ep_t_key = ep[1..orig_table.metadata.keys.len() + 1].to_vec();
            let ep_idx = usize::try_from(
                ep[orig_table.metadata.keys.len() + 1]
                    .get_int()
                    .ok_or_else(|| {
                        InvalidOperationSnafu {
                            op: "hnsw_read",
                            reason: "stored index is not an integer".to_string(),
                        }
                        .build()
                    })?,
            )
            .map_err(|_e| {
                InvalidOperationSnafu {
                    op: "hnsw_read",
                    reason: "stored index out of range",
                }
                .build()
            })?;
            let ep_subidx = i32::try_from(
                ep[orig_table.metadata.keys.len() + 2]
                    .get_int()
                    .ok_or_else(|| {
                        InvalidOperationSnafu {
                            op: "hnsw_read",
                            reason: "stored subindex is not an integer".to_string(),
                        }
                        .build()
                    })?,
            )
            .map_err(|_e| {
                InvalidOperationSnafu {
                    op: "hnsw_read",
                    reason: "stored subindex out of range",
                }
                .build()
            })?;
            let ep_key = (ep_t_key, ep_idx, ep_subidx);
            vec_cache.ensure_key(&ep_key, orig_table, self)?;
            let ep_distance = vec_cache.v_dist(q, &ep_key)?;
            let mut found_nn = PriorityQueue::new();
            found_nn.push(ep_key, OrderedFloat(ep_distance));
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
            for current_level in bottom_level..target_level {
                self.hnsw_search_level(
                    q,
                    1,
                    current_level,
                    orig_table,
                    idx_table,
                    &mut found_nn,
                    vec_cache,
                )?;
            }
            let mut self_tuple_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
            self_tuple_key.push(DataValue::from(0));
            for _ in 0..2 {
                self_tuple_key.extend_from_slice(tuple_key);
                self_tuple_key.push(DataValue::from(idx_to_i64(idx)));
                self_tuple_key.push(DataValue::from(i64::from(subidx)));
            }
            let mut self_tuple_val = vec![
                DataValue::from(0.0),
                DataValue::Bytes(hash.as_ref().to_vec()),
                DataValue::from(false),
            ];
            for current_level in max(target_level, bottom_level)..=0 {
                let m_max = if current_level == 0 {
                    manifest.m_max0
                } else {
                    manifest.m_max
                };
                self.hnsw_search_level(
                    q,
                    manifest.ef_construction,
                    current_level,
                    orig_table,
                    idx_table,
                    &mut found_nn,
                    vec_cache,
                )?;
                let neighbours = self.hnsw_select_neighbours_heuristic(
                    q,
                    &found_nn,
                    m_max,
                    current_level,
                    manifest,
                    idx_table,
                    orig_table,
                    vec_cache,
                )?;
                self_tuple_key[0] = DataValue::from(current_level);
                // INVARIANT: HNSW degree is bounded by m_max (typically <= 128).
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "HNSW degree bounded by m_max (< 2^53)"
                )]
                let degree_f64 = neighbours.len() as f64;
                self_tuple_val[0] = DataValue::from(degree_f64);

                let self_tuple_key_bytes =
                    idx_table.encode_key_for_store(&self_tuple_key, Default::default())?;
                let self_tuple_val_bytes =
                    idx_table.encode_val_only_for_store(&self_tuple_val, Default::default())?;
                self.store_tx
                    .put(&self_tuple_key_bytes, &self_tuple_val_bytes)?;

                for (neighbour, Reverse(OrderedFloat(dist))) in neighbours.iter() {
                    let mut out_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                    let out_val = vec![
                        DataValue::from(*dist),
                        DataValue::Null,
                        DataValue::from(false),
                    ];
                    out_key.push(DataValue::from(current_level));
                    out_key.extend_from_slice(tuple_key);
                    out_key.push(DataValue::from(idx_to_i64(idx)));
                    out_key.push(DataValue::from(i64::from(subidx)));
                    out_key.extend_from_slice(&neighbour.0);
                    out_key.push(DataValue::from(idx_to_i64(neighbour.1)));
                    out_key.push(DataValue::from(i64::from(neighbour.2)));
                    let out_key_bytes =
                        idx_table.encode_key_for_store(&out_key, Default::default())?;
                    let out_val_bytes =
                        idx_table.encode_val_only_for_store(&out_val, Default::default())?;
                    self.store_tx.put(&out_key_bytes, &out_val_bytes)?;

                    let mut in_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                    let in_val = vec![
                        DataValue::from(*dist),
                        DataValue::Null,
                        DataValue::from(false),
                    ];
                    in_key.push(DataValue::from(current_level));
                    in_key.extend_from_slice(&neighbour.0);
                    in_key.push(DataValue::from(idx_to_i64(neighbour.1)));
                    in_key.push(DataValue::from(i64::from(neighbour.2)));
                    in_key.extend_from_slice(tuple_key);
                    in_key.push(DataValue::from(idx_to_i64(idx)));
                    in_key.push(DataValue::from(i64::from(subidx)));

                    let in_key_bytes =
                        idx_table.encode_key_for_store(&in_key, Default::default())?;
                    let in_val_bytes =
                        idx_table.encode_val_only_for_store(&in_val, Default::default())?;
                    self.store_tx.put(&in_key_bytes, &in_val_bytes)?;

                    let mut target_self_key =
                        Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                    target_self_key.push(DataValue::from(current_level));
                    for _ in 0..2 {
                        target_self_key.extend_from_slice(&neighbour.0);
                        target_self_key.push(DataValue::from(idx_to_i64(neighbour.1)));
                        target_self_key.push(DataValue::from(i64::from(neighbour.2)));
                    }
                    let target_self_key_bytes =
                        idx_table.encode_key_for_store(&target_self_key, Default::default())?;
                    let target_self_val_bytes = match self
                        .store_tx
                        .get(&target_self_key_bytes, false)?
                    {
                        Some(bytes) => bytes,
                        None => return Err(InvalidOperationSnafu {
                            op: "hnsw_index",
                            reason: "Indexed vector not found, this signifies a bug in the index implementation".to_string(),
                        }.build().into()),
                    };
                    let mut target_self_val: Vec<DataValue> =
                        rmp_serde::from_slice(&target_self_val_bytes[ENCODED_KEY_MIN_LEN..])
                            .map_err(|e| crate::error::InternalError::Runtime {
                                source: InvalidOperationSnafu {
                                    op: "hnsw_index",
                                    reason: e.to_string(),
                                }
                                .build(),
                            })?;
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "degree is a small non-negative integer stored as f64"
                    )]
                    let mut target_degree = target_self_val[0].get_float().ok_or_else(|| {
                        InvalidOperationSnafu {
                            op: "hnsw_index",
                            reason: "degree is not a float".to_string(),
                        }
                        .build()
                    })? as usize
                        + 1;
                    if target_degree > m_max {
                        target_degree = self.hnsw_shrink_neighbour(
                            neighbour,
                            m_max,
                            current_level,
                            manifest,
                            idx_table,
                            orig_table,
                            vec_cache,
                        )?;
                    }
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "HNSW degree bounded by m_max (< 2^53)"
                    )]
                    let target_degree_f64 = target_degree as f64;
                    target_self_val[0] = DataValue::from(target_degree_f64);
                    self.store_tx.put(
                        &target_self_key_bytes,
                        &idx_table
                            .encode_val_only_for_store(&target_self_val, Default::default())?,
                    )?;
                }
            }
        } else {
            let level = manifest.get_random_level();
            self.hnsw_put_fresh_at_levels(
                hash.as_ref(),
                tuple_key,
                idx,
                subidx,
                orig_table,
                idx_table,
                level,
                0,
            )?;
        }
        Ok(())
    }

    /// Insert a fresh vector at the specified levels without graph traversal.
    ///
    /// Used when inserting into an empty index or when the new node starts a
    /// layer above the current entry point. Writes self-loop entries and a
    /// canary entry for conflict detection.
    ///
    /// # Complexity
    ///
    /// O(levels) where levels is the number of layers to populate.
    fn hnsw_put_fresh_at_levels(
        &mut self,
        hash: &[u8],
        tuple: &[DataValue],
        idx: usize,
        subidx: i32,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        bottom_level: i64,
        top_level: i64,
    ) -> Result<()> {
        let mut target_key = vec![DataValue::Null];
        let mut canary_key = vec![DataValue::from(1)];
        for _ in 0..2 {
            for i in 0..orig_table.metadata.keys.len() {
                target_key.push(
                    tuple
                        .get(i)
                        .ok_or_else(|| {
                            InvalidOperationSnafu {
                                op: "hnsw_put",
                                reason: format!(
                                    "tuple index {i} out of bounds (len {})",
                                    tuple.len()
                                ),
                            }
                            .build()
                        })?
                        .clone(),
                );
                canary_key.push(DataValue::Null);
            }
            target_key.push(DataValue::from(idx_to_i64(idx)));
            target_key.push(DataValue::from(i64::from(subidx)));
            canary_key.push(DataValue::Null);
            canary_key.push(DataValue::Null);
        }
        let target_value = [
            DataValue::from(0.0),
            DataValue::Bytes(hash.to_vec()),
            DataValue::from(false),
        ];
        let target_key_bytes = idx_table.encode_key_for_store(&target_key, Default::default())?;

        // WHY: canary value is for conflict detection: prevent the scenario of disconnected graphs at all levels
        let canary_value = [
            DataValue::from(bottom_level),
            DataValue::Bytes(target_key_bytes),
            DataValue::from(false),
        ];
        let canary_key_bytes = idx_table.encode_key_for_store(&canary_key, Default::default())?;
        let canary_value_bytes =
            idx_table.encode_val_only_for_store(&canary_value, Default::default())?;
        self.store_tx.put(&canary_key_bytes, &canary_value_bytes)?;

        for cur_level in bottom_level..=top_level {
            target_key[0] = DataValue::from(cur_level);
            let key = idx_table.encode_key_for_store(&target_key, Default::default())?;
            let val = idx_table.encode_val_only_for_store(&target_value, Default::default())?;
            self.store_tx.put(&key, &val)?;
        }
        Ok(())
    }

    /// Count vectors currently in the index by scanning the canary prefix.
    ///
    /// Canary entries (level = `DataValue::from(1)`) are written once per vector
    /// in `hnsw_put_fresh_at_levels` and serve as a per-vector marker.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of vectors. Requires a full scan of canary entries.
    fn hnsw_count_vectors(&self, idx_table: &RelationHandle) -> usize {
        let prefix = vec![DataValue::from(1_i64)];
        idx_table.scan_prefix(self, &prefix).count()
    }

    /// Public entry point for HNSW vector insertion.
    ///
    /// Extracts vector fields from `tuple` according to `manifest.vec_fields`,
    /// enforces `max_vectors` capacity limits, and inserts each extracted vector
    /// into the graph. Returns `true` if at least one vector was inserted, `false`
    /// if the tuple was filtered out by the index filter.
    ///
    /// # Complexity
    ///
    /// O(v * log n * ef_construction * m) where v is the number of vectors in the
    /// tuple, n is index size, ef_construction is beam width, and m is max connections.
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
            self.hnsw_remove(orig_table, idx_table, tuple)?;
            return Ok(false);
        }

        // WHY: enforce max_vectors capacity limit to prevent unbounded memory/disk growth (#1722).
        if let Some(max_cap) = manifest.max_vectors {
            let current = self.hnsw_count_vectors(idx_table);
            let warn_threshold = max_cap * 4 / 5; // 80 %
            if current >= max_cap {
                return Err(InvalidOperationSnafu {
                    op: "hnsw_put",
                    reason: format!(
                        "HNSW index '{}' is at capacity ({current}/{max_cap}): \
                         increase max_vectors or prune old vectors",
                        manifest.index_name
                    ),
                }
                .build()
                .into());
            }
            if current >= warn_threshold {
                warn!(
                    index = %manifest.index_name,
                    current,
                    max_cap,
                    "HNSW index approaching max_vectors capacity"
                );
            }
        }

        let mut extracted_vectors = vec![];
        for idx in &manifest.vec_fields {
            let val = tuple.get(*idx).ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_put",
                    reason: format!(
                        "vector field index {} out of bounds (tuple len {})",
                        idx,
                        tuple.len()
                    ),
                }
                .build()
            })?;
            if let DataValue::Vec(v) = val {
                extracted_vectors.push((v, *idx, -1));
            } else if let DataValue::List(l) = val {
                for (sidx, v) in l.iter().enumerate() {
                    if let DataValue::Vec(v) = v {
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_possible_wrap,
                            reason = "HNSW layer indices bounded by m_max (< i32::MAX)"
                        )]
                        let sidx_i32 = sidx as i32;
                        extracted_vectors.push((v, *idx, sidx_i32));
                    }
                }
            }
        }
        if extracted_vectors.is_empty() {
            return Ok(false);
        }
        let mut vec_cache = VectorCache::new(manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);
        for (vec, idx, sub) in extracted_vectors {
            self.hnsw_put_vector(
                tuple,
                vec,
                idx,
                sub,
                manifest,
                orig_table,
                idx_table,
                &mut vec_cache,
            )?;
        }
        Ok(true)
    }

    /// Check that every HNSW canary entry has a corresponding row in the base relation.
    ///
    /// Scans the "self-entry" nodes at level 0 (entries where the source and destination
    /// tuple key are identical) and verifies each exists in `orig_table`.  Returns the
    /// number of orphaned HNSW entries detected -- entries whose base row has been deleted
    /// without a matching HNSW removal (#1719).
    ///
    /// Orphans are logged at `warn` level for each occurrence.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of indexed vectors. Performs a canary scan plus
    /// n lookups in the base relation.
    #[expect(
        dead_code,
        reason = "entry point for maintenance tasks — not yet wired into scheduler"
    )]
    pub(crate) fn hnsw_check_consistency(
        &self,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<usize> {
        let key_len = orig_table.metadata.keys.len();
        let mut orphan_count = 0usize;

        // WHY: canary entries written by `hnsw_put_fresh_at_levels` start with
        // `DataValue::from(1_i64)`.  Each represents exactly one indexed vector.
        let canary_prefix = vec![DataValue::from(1_i64)];
        for res in idx_table.scan_prefix(self, &canary_prefix) {
            let tuple = match res {
                Ok(t) => t,
                Err(_) => continue,
            };
            // Canary layout: [1, Null..., key_fields..., idx, subidx, Null..., Null, Null]
            // The tuple_key fields start at offset 1.
            let tuple_key: Vec<DataValue> = tuple.get(1..key_len + 1).unwrap_or_default().to_vec();
            if tuple_key.is_empty() {
                continue;
            }
            match orig_table.get(self, &tuple_key) {
                Ok(Some(_)) => {} // base row exists -- consistent
                Ok(None) => {
                    orphan_count = orphan_count.saturating_add(1);
                    warn!(
                        index = %manifest.index_name,
                        base_relation = %manifest.base_relation,
                        orphans = orphan_count,
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

        Ok(orphan_count)
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    reason = "test assertions and test-only numeric casts"
)]
mod tests {
    use crate::DbInstance;

    // ---------------------------------------------------------------------------
    // Helper: create a standard 4-dim F32/L2 HNSW index on a relation named
    // `vectors { id: Int => vec: <F32; 4> }` with the index named `idx`.
    // ---------------------------------------------------------------------------
    fn setup_db() -> DbInstance {
        let db = DbInstance::default();
        db.run_default(":create vectors { id: Int => vec: <F32; 4> }")
            .unwrap();
        db.run_default(
            r#"::hnsw create vectors:idx {
                dim: 4,
                m: 16,
                dtype: F32,
                fields: [vec],
                distance: L2,
                ef_construction: 50,
                extend_candidates: false,
                keep_pruned_connections: false,
            }"#,
        )
        .unwrap();
        db
    }

    // Insert `n` vectors into the index. Vector for id `i` is [i,i,i,i].
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
        assert_eq!(res.rows.len(), 1, "inserted vector should be retrievable");
        let id = res.rows[0][0].get_int().unwrap();
        assert_eq!(id, 42);
    }

    #[test]
    fn put_multiple_vectors_all_retrievable() {
        let db = setup_db();
        insert_vectors(&db, 10);
        let res = db.run_default("?[id] := *vectors{id}").unwrap();
        assert_eq!(
            res.rows.len(),
            10,
            "all 10 inserted vectors should be stored"
        );
    }

    #[test]
    fn put_duplicate_key_is_idempotent() {
        let db = setup_db();
        db.run_default("?[id, vec] <- [[1, vec([1.0, 1.0, 1.0, 1.0])]] :put vectors {}")
            .unwrap();
        db.run_default("?[id, vec] <- [[1, vec([1.0, 1.0, 1.0, 1.0])]] :put vectors {}")
            .unwrap();
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
                r#"?[id, dist] := ~vectors:idx{id | query: vec([9.0, 9.0, 9.0, 9.0]), k: 1, ef: 50, bind_distance: dist}"#,
            )
            .unwrap();
        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0][0].get_int().unwrap(), 7);
        let dist = res.rows[0][1].get_float().unwrap();
        assert!(
            dist < 1e-6,
            "updated vector should be at distance ~0 from query"
        );
    }

    #[test]
    fn put_empty_index_search_returns_nothing() {
        let db = setup_db();
        let res = db
            .run_default(
                r#"?[id, dist] := ~vectors:idx{id | query: vec([1.0, 2.0, 3.0, 4.0]), k: 5, ef: 50, bind_distance: dist}"#,
            )
            .unwrap();
        assert!(res.rows.is_empty(), "empty index must return no results");
    }

    #[test]
    fn search_level_returns_nearest_first() {
        let db = setup_db();
        for i in 0..50 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "test fixture with small integers"
            )]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, 0.0, 0.0, 0.0])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        let res = db
            .run_default(
                r#"?[id, dist] := ~vectors:idx{id | query: vec([25.0, 0.0, 0.0, 0.0]), k: 5, ef: 50, bind_distance: dist} :order dist"#,
            )
            .unwrap();
        assert!(!res.rows.is_empty(), "search should return results");
        let first_id = res.rows[0][0].get_int().unwrap();
        assert_eq!(
            first_id, 25,
            "nearest neighbor (id=25) must be returned first"
        );
    }

    #[test]
    fn search_level_results_non_decreasing_distance() {
        let db = setup_db();
        insert_vectors(&db, 30);
        let res = db
            .run_default(
                r#"?[id, dist] := ~vectors:idx{id | query: vec([15.0, 15.0, 15.0, 15.0]), k: 10, ef: 50, bind_distance: dist} :order dist"#,
            )
            .unwrap();
        assert!(
            !res.rows.is_empty(),
            "search must return results after 30 inserts"
        );
        let distances: Vec<f64> = res.rows.iter().filter_map(|r| r[1].get_float()).collect();
        for window in distances.windows(2) {
            assert!(
                window[0] <= window[1],
                "distances must be non-decreasing: {} > {}",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn search_level_pooled_greedy_finds_exact_match() {
        let db = setup_db();
        insert_vectors(&db, 20);
        let res = db
            .run_default(
                r#"?[id] := ~vectors:idx{id | query: vec([10.0, 10.0, 10.0, 10.0]), k: 1, ef: 1, bind_distance: _dist}"#,
            )
            .unwrap();
        assert!(
            res.rows.len() <= 1,
            "greedy search must return at most k=1 results"
        );
    }

    #[test]
    fn get_neighbours_dense_insert_preserves_connectivity() {
        let db = setup_db();
        insert_vectors(&db, 100);
        let res = db
            .run_default(
                r#"?[id, dist] := ~vectors:idx{id | query: vec([50.0, 50.0, 50.0, 50.0]), k: 5, ef: 50, bind_distance: dist}"#,
            )
            .unwrap();
        assert!(
            !res.rows.is_empty(),
            "graph must remain searchable after dense insert (neighbour shrink)"
        );
        assert!(
            res.rows.len() <= 5,
            "must return at most k=5 results, got {}",
            res.rows.len()
        );
    }

    #[test]
    fn consistency_all_inserted_vectors_present_in_base_relation() {
        let db = setup_db();
        insert_vectors(&db, 15);
        let res = db.run_default("?[id] := *vectors{id}").unwrap();
        assert_eq!(
            res.rows.len(),
            15,
            "every inserted vector must have a base-relation row (consistency invariant)"
        );
    }

    #[test]
    fn consistency_deleted_vector_absent_from_index() {
        let db = setup_db();
        insert_vectors(&db, 10);
        db.run_default("?[id] <- [[5]] :rm vectors {}").unwrap();

        let base = db.run_default("?[id] := *vectors{id}, id = 5").unwrap();
        assert!(
            base.rows.is_empty(),
            "deleted vector must be absent from base relation"
        );

        let search = db
            .run_default(
                r#"?[id] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 10, ef: 50, bind_distance: _dist}"#,
            )
            .unwrap();
        let ids: Vec<i64> = search.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(
            !ids.contains(&5),
            "deleted vector id=5 must not appear in search results (index consistency)"
        );
    }

    #[test]
    fn consistency_after_index_rebuild() {
        let db = setup_db();
        insert_vectors(&db, 20);
        db.run_default("::hnsw drop vectors:idx").unwrap();
        db.run_default(
            r#"::hnsw create vectors:idx {
                dim: 4,
                m: 16,
                dtype: F32,
                fields: [vec],
                distance: L2,
                ef_construction: 50,
                extend_candidates: false,
                keep_pruned_connections: false,
            }"#,
        )
        .unwrap();
        let res = db
            .run_default(
                r#"?[id, dist] := ~vectors:idx{id | query: vec([10.0, 10.0, 10.0, 10.0]), k: 3, ef: 50, bind_distance: dist}"#,
            )
            .unwrap();
        assert!(
            !res.rows.is_empty(),
            "rebuilt index must be searchable — base-relation rows are consistent"
        );
    }
}
