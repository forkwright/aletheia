// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

//! SessionTx methods for HNSW vector removal.

use std::cmp::Reverse;

use itertools::Itertools;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rustc_hash::FxHashSet;

use super::idx_to_i64;
use super::types::{CompoundKey, DEFAULT_VECTOR_CACHE_CAPACITY, HnswIndexManifest, VectorCache};
use crate::DataValue;
use crate::data::tuple::ENCODED_KEY_MIN_LEN;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl SessionTx<'_> {
    /// Remove all vectors associated with a tuple from the HNSW index.
    ///
    /// # Complexity
    ///
    /// O(v * L * m) where v is vectors in tuple, L is number of levels, and m is
    /// max connections. Must update neighbor connections at each level.
    pub(crate) fn hnsw_remove(
        &mut self,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        tuple: &[DataValue],
    ) -> Result<()> {
        let mut prefix = vec![DataValue::from(0)];
        prefix.extend_from_slice(&tuple[0..orig_table.metadata.keys.len()]);
        let candidates: FxHashSet<_> = idx_table
            .scan_prefix(self, &prefix)
            .filter_map(|t| match t {
                Ok(t) => {
                    #[expect(clippy::cast_sign_loss, reason = "HNSW indices are non-negative")]
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "HNSW index fits in usize on all platforms"
                    )]
                    // INVARIANT: HNSW index tuples store int values at index positions
                    let idx = t[orig_table.metadata.keys.len() + 1]
                        .get_int()
                        .unwrap_or_else(|| unreachable!("HNSW index value is not an integer"))
                        as usize;
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "HNSW subindex bounded by m_max (< i32::MAX)"
                    )]
                    let subidx = t[orig_table.metadata.keys.len() + 2]
                        .get_int()
                        .unwrap_or_else(|| unreachable!("HNSW subindex value is not an integer"))
                        as i32;
                    Some((
                        t[1..orig_table.metadata.keys.len() + 1].to_vec(),
                        idx,
                        subidx,
                    ))
                }
                Err(_) => None,
            })
            .collect();
        for (tuple_key, idx, subidx) in candidates {
            self.hnsw_remove_vec(&tuple_key, idx, subidx, manifest, orig_table, idx_table)?;
        }
        Ok(())
    }
    /// Remove a specific vector (identified by compound key) from the index.
    ///
    /// After cutting the removed node's edges at each layer, the surviving
    /// former neighbours are offered to each other as connection candidates
    /// through the selection heuristic (#6952) — without that repair pass an
    /// unlucky hub deletion severs the layer-0 graph and traps searches in
    /// the resulting island.
    ///
    /// # Complexity
    ///
    /// O(L * m^2) where L is number of levels and m is max connections per
    /// node: O(m) bidirectional edge updates plus the heuristic-bounded
    /// repair pass at each level containing the vector.
    pub(super) fn hnsw_remove_vec(
        &mut self,
        tuple_key: &[DataValue],
        idx: usize,
        subidx: i32,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<()> {
        let compound_key = (tuple_key.to_vec(), idx, subidx);
        let mut encountered_singletons = false;
        let mut vec_cache = VectorCache::new(manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);
        for neg_layer in 0i64.. {
            let layer = -neg_layer;
            let mut self_key = vec![DataValue::from(layer)];
            for _ in 0..2 {
                self_key.extend_from_slice(tuple_key);
                self_key.push(DataValue::from(idx_to_i64(idx)));
                self_key.push(DataValue::from(i64::from(subidx)));
            }
            let self_key_bytes = idx_table.encode_key_for_store(&self_key, Default::default())?;
            if self.store_tx.exists(&self_key_bytes, false)? {
                self.store_tx.del(&self_key_bytes)?;
            } else {
                break;
            }

            // WHY two collections (#6952): the cut below erases every edge
            // row touching the removed node, soft-deleted ones included —
            // but only the *live* former neighbours lost real connectivity,
            // so they alone form the repair set. Both are snapshotted before
            // any edge is cut.
            let live_neighbours = self
                .hnsw_get_neighbours(&compound_key, layer, idx_table, false)?
                .map(|(neighbour_key, _)| neighbour_key)
                .collect_vec();
            let neigbours = self
                .hnsw_get_neighbours(&compound_key, layer, idx_table, true)?
                .collect_vec();
            encountered_singletons |= neigbours.is_empty();
            for (neighbour_key, _) in neigbours {
                let mut out_key = vec![DataValue::from(layer)];
                out_key.extend_from_slice(tuple_key);
                out_key.push(DataValue::from(idx_to_i64(idx)));
                out_key.push(DataValue::from(i64::from(subidx)));
                out_key.extend_from_slice(&neighbour_key.0);
                out_key.push(DataValue::from(idx_to_i64(neighbour_key.1)));
                out_key.push(DataValue::from(i64::from(neighbour_key.2)));
                let out_key_bytes = idx_table.encode_key_for_store(&out_key, Default::default())?;
                self.store_tx.del(&out_key_bytes)?;
                let mut in_key = vec![DataValue::from(layer)];
                in_key.extend_from_slice(&neighbour_key.0);
                in_key.push(DataValue::from(idx_to_i64(neighbour_key.1)));
                in_key.push(DataValue::from(i64::from(neighbour_key.2)));
                in_key.extend_from_slice(tuple_key);
                in_key.push(DataValue::from(idx_to_i64(idx)));
                in_key.push(DataValue::from(i64::from(subidx)));
                let in_key_bytes = idx_table.encode_key_for_store(&in_key, Default::default())?;
                self.store_tx.del(&in_key_bytes)?;
                let mut neighbour_self_key = vec![DataValue::from(layer)];
                for _ in 0..2 {
                    neighbour_self_key.extend_from_slice(&neighbour_key.0);
                    neighbour_self_key.push(DataValue::from(idx_to_i64(neighbour_key.1)));
                    neighbour_self_key.push(DataValue::from(i64::from(neighbour_key.2)));
                }
                let neighbour_val_bytes = match self
                    .store_tx
                    .get(
                        &idx_table.encode_key_for_store(&neighbour_self_key, Default::default())?,
                        false,
                    )? {
                    Some(bytes) => bytes,
                    None => return Err(InvalidOperationSnafu {
                        op: "hnsw_remove",
                        reason: "HNSW neighbour self-key not found during removal, index may be corrupted".to_string(),
                    }.build().into()),
                };
                let mut neighbour_val: Vec<DataValue> = rmp_serde::from_slice(
                    &neighbour_val_bytes[ENCODED_KEY_MIN_LEN..],
                )
                .map_err(|e| crate::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: e.to_string(),
                    }
                    .build(),
                })?;
                neighbour_val[0] = DataValue::from(
                    neighbour_val[0].get_float().ok_or_else(|| {
                        InvalidOperationSnafu {
                            op: "hnsw_remove",
                            reason: "neighbor degree is not a float".to_string(),
                        }
                        .build()
                    })? - 1.,
                );
                self.store_tx.put(
                    &idx_table.encode_key_for_store(&neighbour_self_key, Default::default())?,
                    &idx_table.encode_val_only_for_store(&neighbour_val, Default::default())?,
                )?;
            }

            if live_neighbours.len() > 1 {
                self.hnsw_repair_severed_neighbourhood(
                    &live_neighbours,
                    layer,
                    manifest,
                    orig_table,
                    idx_table,
                    &mut vec_cache,
                )?;
            }
        }

        if encountered_singletons {
            let ep_res = idx_table
                .scan_bounded_prefix(
                    self,
                    &[],
                    &[DataValue::from(i64::MIN)],
                    &[DataValue::from(1)],
                )
                .next();
            let mut canary_key = vec![DataValue::from(1)];
            for _ in 0..2 {
                for _ in 0..orig_table.metadata.keys.len() {
                    canary_key.push(DataValue::Null);
                }
                canary_key.push(DataValue::Null);
                canary_key.push(DataValue::Null);
            }
            let canary_key_bytes =
                idx_table.encode_key_for_store(&canary_key, Default::default())?;
            if let Some(ep) = ep_res {
                let ep = ep?;
                let target_key_bytes = idx_table.encode_key_for_store(&ep, Default::default())?;
                // SAFETY: `ep` comes from HNSW index scan which yields tuples with at least 1 element.
                let bottom_level = ep[0].get_int().ok_or_else(|| {
                    InvalidOperationSnafu {
                        op: "hnsw_remove",
                        reason: "entry point bottom_level is not an integer".to_string(),
                    }
                    .build()
                })?;
                // WHY: canary value is for conflict detection: prevent the scenario of disconnected graphs at all levels
                let canary_value = [
                    DataValue::from(bottom_level),
                    DataValue::Bytes(target_key_bytes),
                    DataValue::from(false),
                ];
                let canary_value_bytes =
                    idx_table.encode_val_only_for_store(&canary_value, Default::default())?;
                self.store_tx.put(&canary_key_bytes, &canary_value_bytes)?;
            } else {
                self.store_tx.del(&canary_key_bytes)?;
            }
        }

        Ok(())
    }

    /// Repair pass after a deletion (#6952): the removed node may have been
    /// the only path between its neighbours, so offer each surviving former
    /// neighbour the others as connection candidates and let
    /// [`Self::hnsw_select_neighbours_heuristic`] decide which bridges to
    /// keep. Without this, deleting a hub's neighbourhood can sever the
    /// layer-0 graph outright — the entry-point re-derivation then lands
    /// inside a severed island and every query confined there returns zero
    /// relevant results.
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
    /// O(m^2) candidate distances per survivor, m survivors — bounded by
    /// `m_max` (`m_max0` at layer 0) either way, with typically only a
    /// handful of accepted bridges.
    fn hnsw_repair_severed_neighbourhood(
        &mut self,
        survivors: &[CompoundKey],
        layer: i64,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        let m_max = if layer == 0 {
            manifest.m_max0
        } else {
            manifest.m_max
        };

        for u in survivors {
            vec_cache.ensure_key(u, orig_table, self)?;
            let u_vec = vec_cache.get_key(u).clone();

            let mut candidates: PriorityQueue<CompoundKey, OrderedFloat<f64>> =
                PriorityQueue::new();
            for (neighbour_key, neighbour_dist) in
                self.hnsw_get_neighbours(u, layer, idx_table, false)?
            {
                candidates.push(neighbour_key, OrderedFloat(neighbour_dist));
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
                layer,
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
                self.hnsw_repair_wire_bridge(
                    u, &w, dist, layer, m_max, manifest, idx_table, orig_table, vec_cache,
                )?;
                added += 1;
            }
            if added == 0 {
                continue;
            }

            // The bridge writer above maintains the far endpoint's degree;
            // this mirrors it for `u` — bump by the accepted bridges and
            // rebalance through the standard shrink path if that crossed
            // `m_max`.
            let mut degree = self.hnsw_repair_read_degree(u, layer, idx_table)? + added;
            if degree > m_max {
                degree = self.hnsw_shrink_neighbour(
                    u, m_max, layer, manifest, idx_table, orig_table, vec_cache,
                )?;
            }
            self.hnsw_repair_write_degree(u, layer, degree, idx_table)?;
        }
        Ok(())
    }

    /// Write the live bridge edges `u -> w` and `w -> u` at `layer`, then
    /// bump `w`'s self-entry degree — shrinking `w`'s adjacency through the
    /// heuristic if that pushes it past `m_max` (the same maintenance the
    /// insert path performs after wiring a new connection).
    fn hnsw_repair_wire_bridge(
        &mut self,
        u: &CompoundKey,
        w: &CompoundKey,
        dist: f64,
        layer: i64,
        m_max: usize,
        manifest: &HnswIndexManifest,
        idx_table: &RelationHandle,
        orig_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        let edge_val = vec![
            DataValue::from(dist),
            DataValue::Null,
            DataValue::from(false),
        ];
        let edge_val_bytes = idx_table.encode_val_only_for_store(&edge_val, Default::default())?;
        for (from, to) in [(u, w), (w, u)] {
            let mut edge_key = Vec::with_capacity(from.0.len() * 2 + 5);
            edge_key.push(DataValue::from(layer));
            edge_key.extend_from_slice(&from.0);
            edge_key.push(DataValue::from(idx_to_i64(from.1)));
            edge_key.push(DataValue::from(i64::from(from.2)));
            edge_key.extend_from_slice(&to.0);
            edge_key.push(DataValue::from(idx_to_i64(to.1)));
            edge_key.push(DataValue::from(i64::from(to.2)));
            let edge_key_bytes = idx_table.encode_key_for_store(&edge_key, Default::default())?;
            self.store_tx.put(&edge_key_bytes, &edge_val_bytes)?;
        }

        let mut degree = self.hnsw_repair_read_degree(w, layer, idx_table)? + 1;
        if degree > m_max {
            degree = self.hnsw_shrink_neighbour(
                w, m_max, layer, manifest, idx_table, orig_table, vec_cache,
            )?;
        }
        self.hnsw_repair_write_degree(w, layer, degree, idx_table)?;
        Ok(())
    }

    /// Read a node's self-entry degree at `layer` (value slot 0).
    fn hnsw_repair_read_degree(
        &self,
        node: &CompoundKey,
        layer: i64,
        idx_table: &RelationHandle,
    ) -> Result<usize> {
        let self_key_bytes = Self::hnsw_repair_self_key_bytes(node, layer, idx_table)?;
        let existing = self.store_tx.get(&self_key_bytes, false)?.ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_remove",
                reason: "survivor self-entry missing during repair — index is corrupted"
                    .to_string(),
            }
            .build()
        })?;
        let val: Vec<DataValue> =
            rmp_serde::from_slice(&existing[ENCODED_KEY_MIN_LEN..]).map_err(|e| {
                crate::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: e.to_string(),
                    }
                    .build(),
                }
            })?;
        let degree = val[0].get_float().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_remove",
                reason: "survivor degree is not a float".to_string(),
            }
            .build()
        })?;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "degree is a small non-negative integer stored as f64"
        )]
        Ok(degree as usize)
    }

    /// Overwrite a node's self-entry degree at `layer` (value slot 0),
    /// preserving the stored hash and deletion flag.
    fn hnsw_repair_write_degree(
        &mut self,
        node: &CompoundKey,
        layer: i64,
        degree: usize,
        idx_table: &RelationHandle,
    ) -> Result<()> {
        let self_key_bytes = Self::hnsw_repair_self_key_bytes(node, layer, idx_table)?;
        let existing = self.store_tx.get(&self_key_bytes, false)?.ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_remove",
                reason: "survivor self-entry missing during repair — index is corrupted"
                    .to_string(),
            }
            .build()
        })?;
        let mut val: Vec<DataValue> = rmp_serde::from_slice(&existing[ENCODED_KEY_MIN_LEN..])
            .map_err(|e| crate::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "hnsw_index",
                    reason: e.to_string(),
                }
                .build(),
            })?;
        #[expect(
            clippy::cast_precision_loss,
            reason = "HNSW degree bounded by m_max (< 2^53)"
        )]
        {
            val[0] = DataValue::from(degree as f64);
        }
        let val_bytes = idx_table.encode_val_only_for_store(&val, Default::default())?;
        self.store_tx.put(&self_key_bytes, &val_bytes)?;
        Ok(())
    }

    /// Encode a node's self-entry key at `layer` (the `fr == to` row shape).
    fn hnsw_repair_self_key_bytes(
        node: &CompoundKey,
        layer: i64,
        idx_table: &RelationHandle,
    ) -> Result<Vec<u8>> {
        let mut self_key = vec![DataValue::from(layer)];
        for _ in 0..2 {
            self_key.extend_from_slice(&node.0);
            self_key.push(DataValue::from(idx_to_i64(node.1)));
            self_key.push(DataValue::from(i64::from(node.2)));
        }
        idx_table.encode_key_for_store(&self_key, Default::default())
    }
}
