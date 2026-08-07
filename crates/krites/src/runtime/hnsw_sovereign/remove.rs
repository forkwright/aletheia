//! `SessionTx` methods for HNSW vector removal.

use itertools::Itertools;
use rustc_hash::FxHashSet;

use super::types::{CompoundKey, decode_edge_value, edge_key, entry_point_key, self_entry_key};
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
            self.hnsw_remove_vec(&tuple_key, field, subidx, orig_table, idx_table)?;
        }
        Ok(())
    }

    /// Remove one vector instance (identified by compound key) from the
    /// index: at every level it participates in (from 0 upward through its
    /// own top level), delete its self-entry and every edge touching it —
    /// both directions, hard delete (a genuine removal never needs the
    /// soft-delete tombstone `hnsw_shrink_neighbour` uses) — decrementing
    /// each surviving neighbour's degree. If any level left this vector
    /// with zero neighbours, it may have been the entry point — rebuild or
    /// clear the entry-point marker afterward.
    ///
    /// # Complexity
    ///
    /// `O(L * m)`.
    pub(super) fn hnsw_remove_vec(
        &mut self,
        tuple_key: &[DataValue],
        field: usize,
        subidx: i32,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<()> {
        let target: CompoundKey = (tuple_key.to_vec(), field, subidx);
        let mut encountered_singleton = false;

        for depth in 0i64.. {
            let level = -depth;
            let self_key = self_entry_key(level, tuple_key, field, subidx);
            let self_key_bytes = idx_table.encode_key_for_store(&self_key, Default::default())?;
            if !self.store_tx.exists(&self_key_bytes, false)? {
                break;
            }
            self.store_tx.del(&self_key_bytes)?;

            let neighbours = self
                .hnsw_get_neighbours(&target, level, idx_table, true)?
                .collect_vec();
            encountered_singleton |= neighbours.is_empty();

            for (neighbour, _dist) in neighbours {
                let out_bytes = idx_table
                    .encode_key_for_store(&edge_key(level, &target, &neighbour), Default::default())?;
                self.store_tx.del(&out_bytes)?;
                let in_bytes = idx_table
                    .encode_key_for_store(&edge_key(level, &neighbour, &target), Default::default())?;
                self.store_tx.del(&in_bytes)?;

                let neighbour_self = self_entry_key(level, &neighbour.0, neighbour.1, neighbour.2);
                let neighbour_self_bytes =
                    idx_table.encode_key_for_store(&neighbour_self, Default::default())?;
                let existing = self.store_tx.get(&neighbour_self_bytes, false)?.ok_or_else(|| {
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
                self.store_tx.put(&neighbour_self_bytes, &neighbour_val_bytes)?;
            }
        }

        if encountered_singleton {
            self.hnsw_rebuild_entry_point(orig_table, idx_table)?;
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

        let marker_key_bytes = idx_table
            .encode_key_for_store(&entry_point_key(orig_table.metadata.keys.len()), Default::default())?;

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
        let marker_val_bytes = idx_table.encode_val_only_for_store(&marker_val, Default::default())?;
        self.store_tx.put(&marker_key_bytes, &marker_val_bytes)?;
        Ok(())
    }
}
