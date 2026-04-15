//! HNSW graph traversal: level search, neighbor selection, and connection pruning.
//!
//! This module implements the core graph operations of the HNSW algorithm:
//!
//! - **Level search** (`hnsw_search_level`, `hnsw_search_level_pooled`): beam search
//!   at a single layer of the hierarchical graph, expanding a priority queue of
//!   nearest-neighbor candidates.
//!
//! - **Neighbor selection** (`hnsw_select_neighbours_heuristic`): Algorithm 4 from
//!   the HNSW paper -- selects up to `m_max` neighbors using a diversity heuristic
//!   that prefers candidates closer to the query than to any already-selected neighbor.
//!
//! - **Connection pruning** (`hnsw_shrink_neighbour`): when a node exceeds `m_max`
//!   connections after insertion, this re-selects neighbors using the heuristic and
//!   updates bidirectional edges in the index.
//!
//! - **Neighbor retrieval** (`hnsw_get_neighbours`): scans the index for all edges
//!   from a given node at a given level, optionally filtering soft-deleted edges.

use std::cmp::Reverse;

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rustc_hash::FxHashSet;

use super::idx_to_i64;
use super::types::{CompoundKey, HnswIndexManifest, VectorCache};
use super::visited_pool::VisitedPool;
use crate::DataValue;
use crate::data::tuple::ENCODED_KEY_MIN_LEN;
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl SessionTx<'_> {
    /// Shrink neighbor connections when exceeding `m_max`.
    ///
    /// Re-selects neighbors using the HNSW heuristic and updates bidirectional
    /// edges in the index. Returns the new degree after pruning.
    ///
    /// # Complexity
    ///
    /// O(m^2) where m is the maximum connections. Evaluates all pairwise distances
    /// between neighbors for the heuristic selection.
    pub(crate) fn hnsw_shrink_neighbour(
        &mut self,
        target_key: &CompoundKey,
        m: usize,
        level: i64,
        manifest: &HnswIndexManifest,
        idx_table: &RelationHandle,
        orig_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<usize> {
        vec_cache.ensure_key(target_key, orig_table, self)?;
        let vec = vec_cache.get_key(target_key).clone();
        let mut candidates = PriorityQueue::new();
        for (neighbour_key, neighbour_dist) in
            self.hnsw_get_neighbours(target_key, level, idx_table, false)?
        {
            candidates.push(neighbour_key, OrderedFloat(neighbour_dist));
        }
        let new_candidates = self.hnsw_select_neighbours_heuristic(
            &vec,
            &candidates,
            m,
            level,
            manifest,
            idx_table,
            orig_table,
            vec_cache,
        )?;
        let mut old_candidate_set = FxHashSet::default();
        for (old, _) in &candidates {
            old_candidate_set.insert(old.clone());
        }
        let mut new_candidate_set = FxHashSet::default();
        for (new, _) in &new_candidates {
            new_candidate_set.insert(new.clone());
        }
        let new_degree = new_candidates.len();
        for (new, Reverse(OrderedFloat(new_dist))) in new_candidates {
            if !old_candidate_set.contains(&new) {
                let mut new_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                let new_val = vec![
                    DataValue::from(new_dist),
                    DataValue::Null,
                    DataValue::from(false),
                ];
                new_key.push(DataValue::from(level));
                new_key.extend_from_slice(&target_key.0);
                new_key.push(DataValue::from(idx_to_i64(target_key.1)));
                new_key.push(DataValue::from(i64::from(target_key.2)));
                new_key.extend_from_slice(&new.0);
                new_key.push(DataValue::from(idx_to_i64(new.1)));
                new_key.push(DataValue::from(i64::from(new.2)));
                let new_key_bytes = idx_table.encode_key_for_store(&new_key, Default::default())?;
                let new_val_bytes =
                    idx_table.encode_val_only_for_store(&new_val, Default::default())?;
                self.store_tx.put(&new_key_bytes, &new_val_bytes)?;
            }
        }
        for (old, OrderedFloat(old_dist)) in candidates {
            if !new_candidate_set.contains(&old) {
                let mut old_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                old_key.push(DataValue::from(level));
                old_key.extend_from_slice(&target_key.0);
                old_key.push(DataValue::from(idx_to_i64(target_key.1)));
                old_key.push(DataValue::from(i64::from(target_key.2)));
                old_key.extend_from_slice(&old.0);
                old_key.push(DataValue::from(idx_to_i64(old.1)));
                old_key.push(DataValue::from(i64::from(old.2)));
                let old_key_bytes = idx_table.encode_key_for_store(&old_key, Default::default())?;
                let old_existing_val = match self.store_tx.get(&old_key_bytes, false)? {
                    Some(bytes) => bytes,
                    None => {
                        return Err(InvalidOperationSnafu {
                            op: "hnsw_index",
                            reason: "Indexed vector not found, this signifies a bug in the index implementation".to_string(),
                        }.build().into())
                    }
                };
                let old_existing_val: Vec<DataValue> = rmp_serde::from_slice(
                    &old_existing_val[ENCODED_KEY_MIN_LEN..],
                )
                .map_err(|e| crate::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: e.to_string(),
                    }
                    .build(),
                })?;
                if old_existing_val[2].get_bool().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: "deleted flag is not a boolean".to_string(),
                    }
                    .build()
                })? {
                    self.store_tx.del(&old_key_bytes)?;
                } else {
                    let old_val = vec![
                        DataValue::from(old_dist),
                        DataValue::Null,
                        DataValue::from(true),
                    ];
                    let old_val_bytes =
                        idx_table.encode_val_only_for_store(&old_val, Default::default())?;
                    self.store_tx.put(&old_key_bytes, &old_val_bytes)?;
                }
            }
        }

        Ok(new_degree)
    }

    /// Select neighbors using the HNSW heuristic (Algorithm 4 from the paper).
    ///
    /// Given a set of candidate neighbors `found`, selects up to `m` neighbors
    /// using a diversity-aware heuristic: a candidate is accepted only if it is
    /// closer to the query `q` than to any already-selected neighbor. This
    /// prevents clustering and improves recall by maintaining diverse connections.
    ///
    /// When `extend_candidates` is enabled, the candidates are augmented with
    /// their own neighbors before selection. When `keep_pruned_connections` is
    /// enabled, pruned candidates are used as fallback to fill remaining slots.
    ///
    /// # Complexity
    ///
    /// O(m^2 * ef) where m is max connections and ef is the beam width. Performs
    /// pairwise distance checks between all candidates in the found set.
    pub(crate) fn hnsw_select_neighbours_heuristic(
        &self,
        q: &Vector,
        found: &PriorityQueue<CompoundKey, OrderedFloat<f64>>,
        m: usize,
        level: i64,
        manifest: &HnswIndexManifest,
        idx_table: &RelationHandle,
        orig_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>>> {
        let mut candidates = PriorityQueue::new();
        let mut ret: PriorityQueue<CompoundKey, Reverse<OrderedFloat<_>>> = PriorityQueue::new();
        let mut discarded: PriorityQueue<_, Reverse<OrderedFloat<_>>> = PriorityQueue::new();
        for (item, dist) in found.iter() {
            candidates.push(item.clone(), Reverse(*dist));
        }
        if manifest.extend_candidates {
            for (item, _) in found.iter() {
                for (neighbour_key, _) in self.hnsw_get_neighbours(item, level, idx_table, false)? {
                    vec_cache.ensure_key(&neighbour_key, orig_table, self)?;
                    let dist = vec_cache.v_dist(q, &neighbour_key)?;
                    candidates.push(
                        (neighbour_key.0, neighbour_key.1, neighbour_key.2),
                        Reverse(OrderedFloat(dist)),
                    );
                }
            }
        }
        while !candidates.is_empty() && ret.len() < m {
            let (cand_key, Reverse(OrderedFloat(cand_dist_to_q))) =
                candidates.pop().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_select_neighbors",
                        reason: "candidate queue unexpectedly empty".to_string(),
                    }
                    .build()
                })?;
            let mut should_add = true;
            for (existing, _) in ret.iter() {
                vec_cache.ensure_key(&cand_key, orig_table, self)?;
                vec_cache.ensure_key(existing, orig_table, self)?;
                let dist_to_existing = vec_cache.k_dist(existing, &cand_key)?;
                if dist_to_existing < cand_dist_to_q {
                    should_add = false;
                    break;
                }
            }
            if should_add {
                ret.push(cand_key, Reverse(OrderedFloat(cand_dist_to_q)));
            } else if manifest.keep_pruned_connections {
                discarded.push(cand_key, Reverse(OrderedFloat(cand_dist_to_q)));
            }
        }
        if manifest.keep_pruned_connections {
            while !discarded.is_empty() && ret.len() < m {
                let (nearest_triple, Reverse(OrderedFloat(nearest_dist))) =
                    discarded.pop().ok_or_else(|| {
                        InvalidOperationSnafu {
                            op: "hnsw_select_neighbors",
                            reason: "discarded queue unexpectedly empty".to_string(),
                        }
                        .build()
                    })?;
                ret.push(nearest_triple, Reverse(OrderedFloat(nearest_dist)));
            }
        }
        Ok(ret)
    }

    /// Search a single HNSW level, expanding the `found_nn` set.
    ///
    /// Delegates to [`hnsw_search_level_pooled`](Self::hnsw_search_level_pooled)
    /// without a visited-list pool (fresh allocation per call).
    ///
    /// # Complexity
    ///
    /// O(ef * m) where ef is the beam width and m is max connections per node.
    /// Visits at most ef nodes, checking m neighbors each.
    pub(crate) fn hnsw_search_level(
        &self,
        q: &Vector,
        ef: usize,
        cur_level: i64,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        found_nn: &mut PriorityQueue<CompoundKey, OrderedFloat<f64>>,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        self.hnsw_search_level_pooled(
            q, ef, cur_level, orig_table, idx_table, found_nn, vec_cache, None,
        )
    }

    /// Search a single HNSW level with optional visited-list pool.
    ///
    /// Implements beam search at a single layer: starting from the current best
    /// candidates in `found_nn`, expands the search frontier by visiting neighbors
    /// of the best unvisited candidate. Stops when the best candidate is farther
    /// than the worst element in `found_nn`.
    ///
    /// When a [`VisitedPool`] is provided, the visited set is acquired from the
    /// pool and returned after use, eliminating per-search allocation.
    ///
    /// # Complexity
    ///
    /// O(ef * m) where ef is the beam width and m is max connections per node.
    /// Space: O(ef) for the candidate priority queue plus O(ef) for visited set.
    pub(crate) fn hnsw_search_level_pooled(
        &self,
        q: &Vector,
        ef: usize,
        cur_level: i64,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        found_nn: &mut PriorityQueue<CompoundKey, OrderedFloat<f64>>,
        vec_cache: &mut VectorCache,
        visited_pool: Option<&VisitedPool>,
    ) -> Result<()> {
        let mut visited = match visited_pool {
            Some(pool) => pool.acquire(),
            None => FxHashSet::default(),
        };
        let mut candidates: PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>> =
            PriorityQueue::new();

        for item in found_nn.iter() {
            visited.insert(item.0.clone());
            candidates.push(item.0.clone(), Reverse(*item.1));
        }

        while let Some((candidate, Reverse(OrderedFloat(candidate_dist)))) = candidates.pop() {
            let (_, OrderedFloat(furtherest_dist)) = found_nn.peek().ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_search_level",
                    reason: "found_nn empty during level search".to_string(),
                }
                .build()
            })?;
            let furtherest_dist = *furtherest_dist;
            if candidate_dist > furtherest_dist {
                break;
            }
            for (neighbour_key, _) in
                self.hnsw_get_neighbours(&candidate, cur_level, idx_table, false)?
            {
                if visited.contains(&neighbour_key) {
                    continue;
                }
                vec_cache.ensure_key(&neighbour_key, orig_table, self)?;
                let neighbour_dist = vec_cache.v_dist(q, &neighbour_key)?;
                let (_, OrderedFloat(cand_furtherest_dist)) = found_nn.peek().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_search_level",
                        reason: "found_nn empty during neighbor evaluation".to_string(),
                    }
                    .build()
                })?;
                if found_nn.len() < ef || neighbour_dist < *cand_furtherest_dist {
                    candidates.push(neighbour_key.clone(), Reverse(OrderedFloat(neighbour_dist)));
                    found_nn.push(neighbour_key.clone(), OrderedFloat(neighbour_dist));
                    if found_nn.len() > ef {
                        found_nn.pop();
                    }
                }
                visited.insert(neighbour_key);
            }
        }

        if let Some(pool) = visited_pool {
            pool.release(visited);
        }

        Ok(())
    }

    /// Get neighbors of a node at a specific level.
    ///
    /// Scans the index for all edges originating from `cand_key` at `level`.
    /// Self-loops (edges where source == destination) are excluded. When
    /// `include_deleted` is false, edges marked with the soft-delete flag are
    /// also excluded.
    ///
    /// # Complexity
    ///
    /// O(m) where m is the number of neighbors (bounded by m_max).
    pub(super) fn hnsw_get_neighbours<'b>(
        &'b self,
        cand_key: &'b CompoundKey,
        level: i64,
        idx_handle: &RelationHandle,
        include_deleted: bool,
    ) -> Result<impl Iterator<Item = (CompoundKey, f64)> + 'b> {
        let mut start_tuple = Vec::with_capacity(cand_key.0.len() + 3);
        start_tuple.push(DataValue::from(level));
        start_tuple.extend_from_slice(&cand_key.0);
        start_tuple.push(DataValue::from(idx_to_i64(cand_key.1)));
        start_tuple.push(DataValue::from(i64::from(cand_key.2)));
        let key_len = cand_key.0.len();
        Ok(idx_handle
            .scan_prefix(self, &start_tuple)
            .filter_map(move |res| {
                let tuple = res.ok()?;

                #[expect(clippy::cast_sign_loss, reason = "HNSW indices are non-negative")]
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "HNSW index fits in usize on all platforms"
                )]
                // INVARIANT: HNSW index tuples store int values at these positions
                let key_idx = tuple[2 * key_len + 3]
                    .get_int()
                    .unwrap_or_else(|| unreachable!("HNSW neighbor index is not an integer"))
                    as usize;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "HNSW subindex bounded by m_max (< i32::MAX)"
                )]
                let key_subidx = tuple[2 * key_len + 4]
                    .get_int()
                    .unwrap_or_else(|| unreachable!("HNSW neighbor subindex is not an integer"))
                    as i32;
                let key_tup = tuple[key_len + 3..2 * key_len + 3].to_vec();
                if key_tup == cand_key.0 {
                    None
                } else {
                    if include_deleted {
                        return Some((
                            (key_tup, key_idx, key_subidx),
                            tuple[2 * key_len + 5].get_float().unwrap_or_else(|| {
                                unreachable!("HNSW neighbor distance is not a float")
                            }),
                        ));
                    }
                    let is_deleted = tuple[2 * key_len + 7]
                        .get_bool()
                        .unwrap_or_else(|| unreachable!("HNSW deleted flag is not a boolean"));
                    if is_deleted {
                        None
                    } else {
                        Some((
                            (key_tup, key_idx, key_subidx),
                            tuple[2 * key_len + 5].get_float().unwrap_or_else(|| {
                                unreachable!("HNSW neighbor distance is not a float")
                            }),
                        ))
                    }
                }
            }))
    }
}
