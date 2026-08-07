//! HNSW graph traversal: level search, neighbour selection, and connection
//! pruning.
//!
//! - **Level search** ([`SessionTx::hnsw_search_level`],
//!   [`SessionTx::hnsw_search_level_pooled`]): beam search at a single
//!   layer, expanding a priority queue of nearest-neighbour candidates.
//! - **Neighbour selection** ([`SessionTx::hnsw_select_neighbours_heuristic`]):
//!   Algorithm 4 from the HNSW paper — picks up to `m` neighbours with a
//!   diversity heuristic that prefers a candidate closer to the query than
//!   to any neighbour already selected, so connections don't clump.
//! - **Connection pruning** ([`SessionTx::hnsw_shrink_neighbour`]): once a
//!   node exceeds `m_max` connections, re-runs the heuristic over its
//!   current neighbours and rewrites the bidirectional edges that changed.
//! - **Neighbour retrieval** ([`SessionTx::hnsw_get_neighbours`]): scans the
//!   index for edges from one node at one level, excluding self-loops and
//!   (by default) soft-deleted edges.

use std::cmp::Reverse;

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rustc_hash::FxHashSet;

use super::types::{CompoundKey, HnswIndexManifest, VectorCache, decode_edge_value, edge_key};
use super::visited_pool::VisitedPool;
use crate::DataValue;
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl SessionTx<'_> {
    /// Re-select and rewrite `target_key`'s own **outbound** adjacency list
    /// at `level` after it exceeds `m` connections. Only edges directed
    /// `target_key -> *` are touched — this rebalances the one node's own
    /// list, not the reverse direction. (Each node's outbound list is
    /// rebalanced independently, by its own call to this function when its
    /// own degree crosses `m_max`; a neighbour's reverse edge back to
    /// `target_key` is that neighbour's concern, fixed the same way when
    /// its own degree next crosses the bound.) Returns the resulting
    /// degree.
    ///
    /// # Complexity
    ///
    /// `O(m^2)`: the heuristic evaluates every pairwise distance among the
    /// candidate neighbours.
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

        let mut current = PriorityQueue::new();
        for (neighbour, dist) in self.hnsw_get_neighbours(target_key, level, idx_table, false)? {
            current.push(neighbour, OrderedFloat(dist));
        }

        let reselected = self.hnsw_select_neighbours_heuristic(
            &vec, &current, m, level, manifest, idx_table, orig_table, vec_cache,
        )?;

        let kept: FxHashSet<_> = reselected.iter().map(|(k, _)| k.clone()).collect();
        let previously: FxHashSet<_> = current.iter().map(|(k, _)| k.clone()).collect();
        let new_degree = reselected.len();

        for (neighbour, Reverse(OrderedFloat(dist))) in reselected {
            if previously.contains(&neighbour) {
                continue;
            }
            let val = [DataValue::from(dist), DataValue::Null, DataValue::from(false)];
            let key_bytes = idx_table
                .encode_key_for_store(&edge_key(level, target_key, &neighbour), Default::default())?;
            let val_bytes = idx_table.encode_val_only_for_store(&val, Default::default())?;
            self.store_tx.put(&key_bytes, &val_bytes)?;
        }

        for (old, OrderedFloat(dist)) in current {
            if kept.contains(&old) {
                continue;
            }
            let key_bytes = idx_table
                .encode_key_for_store(&edge_key(level, target_key, &old), Default::default())?;
            let existing = self.store_tx.get(&key_bytes, false)?.ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_index",
                    reason: "indexed vector not found, this signifies a bug in the index \
                             implementation"
                        .to_string(),
                }
                .build()
            })?;
            let already_deleted = decode_edge_value(&existing)?[2].get_bool().ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_index",
                    reason: "deleted flag is not a boolean".to_string(),
                }
                .build()
            })?;
            if already_deleted {
                self.store_tx.del(&key_bytes)?;
            } else {
                let val = [DataValue::from(dist), DataValue::Null, DataValue::from(true)];
                let val_bytes = idx_table.encode_val_only_for_store(&val, Default::default())?;
                self.store_tx.put(&key_bytes, &val_bytes)?;
            }
        }

        Ok(new_degree)
    }

    /// Select up to `m` neighbours from `found` using the HNSW diversity
    /// heuristic (Algorithm 4 from the paper): a candidate is accepted only
    /// if it is closer to the query `q` than to every neighbour already
    /// accepted. With `extend_candidates`, the candidate set is first
    /// widened with each candidate's own neighbours; with
    /// `keep_pruned_connections`, rejected candidates fill any remaining
    /// slots once the diverse set is exhausted.
    ///
    /// # Complexity
    ///
    /// `O(m^2 * ef)`: pairwise distance checks between all candidates.
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
        let mut candidates: PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>> =
            PriorityQueue::new();
        for (item, dist) in found.iter() {
            candidates.push(item.clone(), Reverse(*dist));
        }
        if manifest.extend_candidates {
            for (item, _) in found.iter() {
                for (neighbour, _) in self.hnsw_get_neighbours(item, level, idx_table, false)? {
                    vec_cache.ensure_key(&neighbour, orig_table, self)?;
                    let dist = vec_cache.v_dist(q, &neighbour)?;
                    candidates.push(neighbour, Reverse(OrderedFloat(dist)));
                }
            }
        }

        let mut accepted: PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>> =
            PriorityQueue::new();
        let mut rejected: PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>> =
            PriorityQueue::new();

        while !candidates.is_empty() && accepted.len() < m {
            let (candidate, Reverse(OrderedFloat(dist_to_q))) =
                candidates.pop().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_select_neighbors",
                        reason: "candidate queue unexpectedly empty".to_string(),
                    }
                    .build()
                })?;
            let mut diverse = true;
            for (already, _) in accepted.iter() {
                vec_cache.ensure_key(&candidate, orig_table, self)?;
                vec_cache.ensure_key(already, orig_table, self)?;
                if vec_cache.k_dist(already, &candidate)? < dist_to_q {
                    diverse = false;
                    break;
                }
            }
            if diverse {
                accepted.push(candidate, Reverse(OrderedFloat(dist_to_q)));
            } else if manifest.keep_pruned_connections {
                rejected.push(candidate, Reverse(OrderedFloat(dist_to_q)));
            }
        }

        if manifest.keep_pruned_connections {
            while !rejected.is_empty() && accepted.len() < m {
                let (candidate, priority) = rejected.pop().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_select_neighbors",
                        reason: "rejected queue unexpectedly empty".to_string(),
                    }
                    .build()
                })?;
                accepted.push(candidate, priority);
            }
        }

        Ok(accepted)
    }

    /// Search a single HNSW level, expanding `found_nn`. Delegates to
    /// [`Self::hnsw_search_level_pooled`] with a fresh (unpooled)
    /// visited-set.
    ///
    /// # Complexity
    ///
    /// `O(ef * m)`.
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

    /// Beam search at a single layer: starting from the current best
    /// candidates in `found_nn`, repeatedly expands the neighbours of the
    /// closest unvisited candidate, stopping once the closest remaining
    /// candidate is farther than the worst element currently kept.
    ///
    /// A [`VisitedPool`], when given, supplies the visited set instead of a
    /// fresh allocation.
    ///
    /// # Complexity
    ///
    /// `O(ef * m)` time; `O(ef)` space for the candidate queue plus `O(ef)`
    /// for the visited set.
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
        let mut frontier: PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>> =
            PriorityQueue::new();
        for (item, dist) in found_nn.iter() {
            visited.insert(item.clone());
            frontier.push(item.clone(), Reverse(*dist));
        }

        while let Some((candidate, Reverse(OrderedFloat(candidate_dist)))) = frontier.pop() {
            let (_, OrderedFloat(worst_kept)) = found_nn.peek().ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_search_level",
                    reason: "found_nn empty during level search".to_string(),
                }
                .build()
            })?;
            if candidate_dist > *worst_kept {
                break;
            }
            for (neighbour, _) in self.hnsw_get_neighbours(&candidate, cur_level, idx_table, false)? {
                if visited.contains(&neighbour) {
                    continue;
                }
                vec_cache.ensure_key(&neighbour, orig_table, self)?;
                let dist = vec_cache.v_dist(q, &neighbour)?;
                let (_, OrderedFloat(worst_kept)) = found_nn.peek().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_search_level",
                        reason: "found_nn empty during neighbor evaluation".to_string(),
                    }
                    .build()
                })?;
                if found_nn.len() < ef || dist < *worst_kept {
                    frontier.push(neighbour.clone(), Reverse(OrderedFloat(dist)));
                    found_nn.push(neighbour.clone(), OrderedFloat(dist));
                    if found_nn.len() > ef {
                        found_nn.pop();
                    }
                }
                visited.insert(neighbour);
            }
        }

        if let Some(pool) = visited_pool {
            pool.release(visited);
        }
        Ok(())
    }

    /// Neighbours of `cand_key` at `level`: every edge row keyed with `fr =
    /// cand_key`, excluding the self-loop and — unless `include_deleted` —
    /// edges carrying the soft-delete flag.
    ///
    /// # Complexity
    ///
    /// `O(m)`, `m` bounded by `m_max`.
    pub(super) fn hnsw_get_neighbours<'b>(
        &'b self,
        cand_key: &'b CompoundKey,
        level: i64,
        idx_handle: &RelationHandle,
        include_deleted: bool,
    ) -> Result<impl Iterator<Item = (CompoundKey, f64)> + 'b> {
        let key_len = cand_key.0.len();
        let mut prefix = Vec::with_capacity(key_len + 3);
        prefix.push(DataValue::from(level));
        prefix.extend_from_slice(&cand_key.0);
        prefix.push(DataValue::from(super::idx_to_i64(cand_key.1)));
        prefix.push(DataValue::from(i64::from(cand_key.2)));

        Ok(idx_handle
            .scan_prefix(self, &prefix)
            .filter_map(move |res| {
                let tuple = res.ok()?;
                // INVARIANT: row layout is [level, fr_key(K), fr_field,
                // fr_subidx, to_key(K), to_field, to_subidx, dist, hash, deleted].
                #[expect(
                    clippy::cast_sign_loss,
                    clippy::cast_possible_truncation,
                    reason = "HNSW field/sub-index values are non-negative and bounded by m_max"
                )]
                let to_field = tuple[2 * key_len + 3]
                    .get_int()
                    .unwrap_or_else(|| unreachable!("HNSW neighbour field is not an integer"))
                    as usize;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "HNSW sub-index bounded by m_max (< i32::MAX)"
                )]
                let to_subidx = tuple[2 * key_len + 4]
                    .get_int()
                    .unwrap_or_else(|| unreachable!("HNSW neighbour sub-index is not an integer"))
                    as i32;
                let to_key = tuple[key_len + 3..2 * key_len + 3].to_vec();
                if to_key == cand_key.0 {
                    return None;
                }
                if !include_deleted {
                    let deleted = tuple[2 * key_len + 7]
                        .get_bool()
                        .unwrap_or_else(|| unreachable!("HNSW deleted flag is not a boolean"));
                    if deleted {
                        return None;
                    }
                }
                let dist = tuple[2 * key_len + 5]
                    .get_float()
                    .unwrap_or_else(|| unreachable!("HNSW neighbour distance is not a float"));
                Some(((to_key, to_field, to_subidx), dist))
            }))
    }
}
