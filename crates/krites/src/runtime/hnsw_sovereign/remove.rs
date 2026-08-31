//! `SessionTx` methods for HNSW vector removal.

use std::cmp::Reverse;

use itertools::Itertools;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rustc_hash::FxHashSet;

use super::types::{
    CompoundKey, DEFAULT_VECTOR_CACHE_CAPACITY, HnswIndexManifest, VectorCache, decode_edge_value,
    edge_key, entry_point_key, self_entry_key,
};
use crate::DataValue;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl SessionTx<'_> {
    /// Remove every indexed vector belonging to `tuple` (every
    /// `(vec_field, sub_index)` combination it carries) from the index.
    ///
    /// Finds candidates by scanning level-0 rows with `fr_key = tuple`'s
    /// key: this returns the self-entry plus every outbound edge for every
    /// `(field, sub_index)` this tuple indexes, and deduplicating on
    /// `(field, sub_index)` yields the distinct vector instances to remove.
    ///
    /// # Complexity
    ///
    /// `O(v * L * m)`, `v` = vectors in the tuple, `L` = levels, `m` = max
    /// connections.
    pub(crate) fn hnsw_remove(
        &mut self,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        tuple: &[DataValue],
    ) -> Result<()> {
        let key_len = orig_table.metadata.keys.len();
        let mut prefix = vec![DataValue::from(0)];
        prefix.extend_from_slice(&tuple[..key_len]);

        let candidates: FxHashSet<CompoundKey> = idx_table
            .scan_prefix(self, &prefix)
            .filter_map(|res| {
                let row = res.ok()?;
                #[expect(
                    clippy::cast_sign_loss,
                    clippy::cast_possible_truncation,
                    reason = "HNSW field/sub-index values are non-negative and bounded by m_max"
                )]
                let field = row[key_len + 1]
                    .get_int()
                    .unwrap_or_else(|| unreachable!("HNSW field is not an integer"))
                    as usize;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "HNSW sub-index bounded by m_max (< i32::MAX)"
                )]
                let subidx = row[key_len + 2]
                    .get_int()
                    .unwrap_or_else(|| unreachable!("HNSW sub-index is not an integer"))
                    as i32;
                Some((row[1..key_len + 1].to_vec(), field, subidx))
            })
            .collect();

        for (tuple_key, field, subidx) in candidates {
            self.hnsw_remove_vec(&tuple_key, field, subidx, manifest, orig_table, idx_table)?;
        }
        Ok(())
    }

    /// Remove one vector instance (identified by compound key) from the
    /// index: at every level it participates in (from 0 upward through its
    /// own top level), delete its self-entry and every edge touching it —
    /// both directions, hard delete (a genuine removal never needs the
    /// soft-delete tombstone `hnsw_shrink_neighbour` uses) — decrementing
    /// each surviving neighbour's degree, then reconnect the surviving
    /// neighbourhood through the selection heuristic (see
    /// [`Self::hnsw_repair_severed_neighbourhood`]). If any level left this
    /// vector with zero neighbours, it may have been the entry point —
    /// rebuild or clear the entry-point marker afterward.
    ///
    /// # Complexity
    ///
    /// `O(L * m^2)`: `O(m)` edge cuts plus the repair pass's
    /// heuristic-bounded reconnection per level.
    pub(super) fn hnsw_remove_vec(
        &mut self,
        tuple_key: &[DataValue],
        field: usize,
        subidx: i32,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<()> {
        let target: CompoundKey = (tuple_key.to_vec(), field, subidx);
        let mut encountered_singleton = false;
        let mut vec_cache = VectorCache::new(manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);

        for depth in 0i64.. {
            let level = -depth;
            let self_key = self_entry_key(level, tuple_key, field, subidx);
            let self_key_bytes = idx_table.encode_key_for_store(&self_key, Default::default())?;
            if !self.store_tx.exists(&self_key_bytes, false)? {
                break;
            }
            self.store_tx.del(&self_key_bytes)?;

            // WHY two collections (#6952): the cut below must erase every
            // edge row touching the deleted node, soft-deleted ones
            // included — but only the *live* former neighbours lost real
            // connectivity, so they alone form the repair set. Both are
            // snapshotted before any edge is cut.
            let live_neighbours = self
                .hnsw_get_neighbours(&target, level, idx_table, false)?
                .map(|(neighbour, _dist)| neighbour)
                .collect_vec();
            let neighbours = self
                .hnsw_get_neighbours(&target, level, idx_table, true)?
                .collect_vec();
            encountered_singleton |= neighbours.is_empty();

            for (neighbour, _dist) in neighbours {
                let out_bytes = idx_table.encode_key_for_store(
                    &edge_key(level, &target, &neighbour),
                    Default::default(),
                )?;
                self.store_tx.del(&out_bytes)?;
                let in_bytes = idx_table.encode_key_for_store(
                    &edge_key(level, &neighbour, &target),
                    Default::default(),
                )?;
                self.store_tx.del(&in_bytes)?;

                let neighbour_self = self_entry_key(level, &neighbour.0, neighbour.1, neighbour.2);
                let neighbour_self_bytes =
                    idx_table.encode_key_for_store(&neighbour_self, Default::default())?;
                let existing =
                    self.store_tx
                        .get(&neighbour_self_bytes, false)?
                        .ok_or_else(|| {
                            InvalidOperationSnafu {
                        op: "hnsw_remove",
                        reason: "neighbour self-entry missing during removal — index is corrupted"
                            .to_string(),
                    }
                    .build()
                        })?;
                let mut neighbour_val = decode_edge_value(&existing)?;
                let degree = neighbour_val[0].get_float().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_remove",
                        reason: "neighbour degree is not a float".to_string(),
                    }
                    .build()
                })?;
                neighbour_val[0] = DataValue::from(degree - 1.0);
                let neighbour_val_bytes =
                    idx_table.encode_val_only_for_store(&neighbour_val, Default::default())?;
                self.store_tx
                    .put(&neighbour_self_bytes, &neighbour_val_bytes)?;
            }

            if live_neighbours.len() > 1 {
                self.hnsw_repair_severed_neighbourhood(
                    &live_neighbours,
                    level,
                    manifest,
                    orig_table,
                    idx_table,
                    &mut vec_cache,
                )?;
            }
        }

        if encountered_singleton {
            self.hnsw_rebuild_entry_point(orig_table, idx_table)?;
        }
        Ok(())
    }

    /// Repair pass after a deletion (#6952): the cut above removed a node
    /// that may have been the only path between its neighbours, so offer
    /// each surviving former neighbour the others as connection candidates
    /// and let [`Self::hnsw_select_neighbours_heuristic`] decide which
    /// bridges to keep. Without this, deleting a hub's neighbourhood can
    /// sever the level-0 graph outright — the entry-point re-derivation then
    /// lands inside a severed island and every query confined there returns
    /// zero relevant results.
    ///
    /// Purely additive: existing edges are never dropped here (cutting is
    /// what created the hole). A survivor pushed past `m_max` by a new
    /// bridge is rebalanced through the same
    /// [`Self::hnsw_shrink_neighbour`] path an insert-time overflow takes.
    /// Distances among survivors never involve the deleted vector, so the
    /// pass needs no access to the removed tuple's row.
    ///
    /// # Complexity
    ///
    /// `O(m^2)` candidate distances per survivor, `m` survivors — bounded
    /// by `m_max` (`m_max0` at level 0) either way, with typically only a
    /// handful of accepted bridges.
    fn hnsw_repair_severed_neighbourhood(
        &mut self,
        survivors: &[CompoundKey],
        level: i64,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        let m_max = if level == 0 {
            manifest.m_max0
        } else {
            manifest.m_max
        };

        for u in survivors {
            vec_cache.ensure_key(u, orig_table, self)?;
            let u_vec = vec_cache.get_key(u).clone();

            let mut candidates: PriorityQueue<CompoundKey, OrderedFloat<f64>> =
                PriorityQueue::new();
            for (neighbour, dist) in self.hnsw_get_neighbours(u, level, idx_table, false)? {
                candidates.push(neighbour, OrderedFloat(dist));
            }
            let existing: FxHashSet<CompoundKey> =
                candidates.iter().map(|(k, _)| k.clone()).collect();

            let mut extended = false;
            for w in survivors {
                if w == u || existing.contains(w) {
                    continue;
                }
                vec_cache.ensure_key(w, orig_table, self)?;
                let dist = vec_cache.k_dist(u, w)?;
                candidates.push(w.clone(), OrderedFloat(dist));
                extended = true;
            }
            if !extended {
                continue;
            }

            let reselected = self.hnsw_select_neighbours_heuristic(
                &u_vec,
                &candidates,
                m_max,
                level,
                manifest,
                idx_table,
                orig_table,
                vec_cache,
            )?;

            let mut added = 0usize;
            for (w, Reverse(OrderedFloat(dist))) in reselected {
                if existing.contains(&w) {
                    continue;
                }
                self.hnsw_connect_at_level(
                    level, u, &w, dist, m_max, manifest, idx_table, orig_table, vec_cache,
                )?;
                added += 1;
            }
            if added == 0 {
                continue;
            }

            // `hnsw_connect_at_level` maintains the far endpoint's degree;
            // this side mirrors it for `u` — bump by the accepted bridges
            // and rebalance through the standard shrink path if that
            // crossed `m_max`.
            let u_self = self_entry_key(level, &u.0, u.1, u.2);
            let u_self_bytes = idx_table.encode_key_for_store(&u_self, Default::default())?;
            let existing_val = self.store_tx.get(&u_self_bytes, false)?.ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_remove",
                    reason: "survivor self-entry missing during repair — index is corrupted"
                        .to_string(),
                }
                .build()
            })?;
            let mut u_val = decode_edge_value(&existing_val)?;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "degree is a small non-negative integer stored as f64"
            )]
            let mut degree = u_val[0].get_float().ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_remove",
                    reason: "survivor degree is not a float".to_string(),
                }
                .build()
            })? as usize
                + added;
            if degree > m_max {
                degree = self.hnsw_shrink_neighbour(
                    u, m_max, level, manifest, idx_table, orig_table, vec_cache,
                )?;
            }
            #[expect(
                clippy::cast_precision_loss,
                reason = "HNSW degree bounded by m_max (< 2^53)"
            )]
            {
                u_val[0] = DataValue::from(degree as f64);
            }
            let u_val_bytes = idx_table.encode_val_only_for_store(&u_val, Default::default())?;
            self.store_tx.put(&u_self_bytes, &u_val_bytes)?;
        }
        Ok(())
    }

    /// Re-derive the entry-point marker from whatever real row currently
    /// sorts first among levels `<= 1` (see [`super::put`]'s
    /// `hnsw_put_vector` for why that scan reliably lands on a real node
    /// rather than the marker itself), or clear the marker entirely if the
    /// index is now empty.
    ///
    /// WARNING: the marker's opaque back-reference (value slot 1) encodes
    /// the *entire* row the scan returned — key columns and value columns
    /// both — passed through the key encoder as-is. That is not a valid key
    /// for this relation, but it is never decoded back into a lookup
    /// (write-only bookkeeping, same as the fresh-insert path's own
    /// opaque reference — see `put.rs`), so it is harmless; reproduced
    /// exactly for byte-compat rather than "corrected" to a canonical key.
    fn hnsw_rebuild_entry_point(
        &mut self,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<()> {
        let candidate = idx_table
            .scan_bounded_prefix(
                self,
                &[],
                &[DataValue::from(i64::MIN)],
                &[DataValue::from(1)],
            )
            .next()
            .transpose()?;

        let marker_key_bytes = idx_table.encode_key_for_store(
            &entry_point_key(orig_table.metadata.keys.len()),
            Default::default(),
        )?;

        let Some(row) = candidate else {
            self.store_tx.del(&marker_key_bytes)?;
            return Ok(());
        };

        let top_level = row[0].get_int().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_remove",
                reason: "candidate entry point level is not an integer".to_string(),
            }
            .build()
        })?;
        let opaque_ref = idx_table.encode_key_for_store(&row, Default::default())?;
        let marker_val = [
            DataValue::from(top_level),
            DataValue::Bytes(opaque_ref),
            DataValue::from(false),
        ];
        let marker_val_bytes =
            idx_table.encode_val_only_for_store(&marker_val, Default::default())?;
        self.store_tx.put(&marker_key_bytes, &marker_val_bytes)?;
        Ok(())
    }
}
