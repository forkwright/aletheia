//! SessionTx methods for HNSW vector removal.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]

use itertools::Itertools;
use rustc_hash::FxHashSet;

use crate::DataValue;
use crate::data::tuple::ENCODED_KEY_MIN_LEN;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl<'a> SessionTx<'a> {
    pub(crate) fn hnsw_remove(
        &mut self,
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
                    let idx = t[orig_table.metadata.keys.len() + 1]
                        .get_int()
                        .unwrap_or_else(|| unreachable!()) as usize;
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "HNSW subindex bounded by m_max (< i32::MAX)"
                    )]
                    let subidx = t[orig_table.metadata.keys.len() + 2]
                        .get_int()
                        .unwrap_or_else(|| unreachable!()) as i32;
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
            self.hnsw_remove_vec(&tuple_key, idx, subidx, orig_table, idx_table)?;
        }
        Ok(())
    }
    pub(super) fn hnsw_remove_vec(
        &mut self,
        tuple_key: &[DataValue],
        idx: usize,
        subidx: i32,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<()> {
        let compound_key = (tuple_key.to_vec(), idx, subidx);
        let mut encountered_singletons = false;
        for neg_layer in 0i64.. {
            let layer = -neg_layer;
            let mut self_key = vec![DataValue::from(layer)];
            for _ in 0..2 {
                self_key.extend_from_slice(tuple_key);
                self_key.push(DataValue::from(idx as i64));
                self_key.push(DataValue::from(i64::from(subidx)));
            }
            let self_key_bytes = idx_table.encode_key_for_store(&self_key, Default::default())?;
            if self.store_tx.exists(&self_key_bytes, false)? {
                self.store_tx.del(&self_key_bytes)?;
            } else {
                break;
            }

            let neigbours = self
                .hnsw_get_neighbours(&compound_key, layer, idx_table, true)?
                .collect_vec();
            encountered_singletons |= neigbours.is_empty();
            for (neighbour_key, _) in neigbours {
                let mut out_key = vec![DataValue::from(layer)];
                out_key.extend_from_slice(tuple_key);
                out_key.push(DataValue::from(idx as i64));
                out_key.push(DataValue::from(i64::from(subidx)));
                out_key.extend_from_slice(&neighbour_key.0);
                out_key.push(DataValue::from(neighbour_key.1 as i64));
                out_key.push(DataValue::from(i64::from(neighbour_key.2)));
                let out_key_bytes = idx_table.encode_key_for_store(&out_key, Default::default())?;
                self.store_tx.del(&out_key_bytes)?;
                let mut in_key = vec![DataValue::from(layer)];
                in_key.extend_from_slice(&neighbour_key.0);
                in_key.push(DataValue::from(neighbour_key.1 as i64));
                in_key.push(DataValue::from(i64::from(neighbour_key.2)));
                in_key.extend_from_slice(tuple_key);
                in_key.push(DataValue::from(idx as i64));
                in_key.push(DataValue::from(i64::from(subidx)));
                let in_key_bytes = idx_table.encode_key_for_store(&in_key, Default::default())?;
                self.store_tx.del(&in_key_bytes)?;
                let mut neighbour_self_key = vec![DataValue::from(layer)];
                for _ in 0..2 {
                    neighbour_self_key.extend_from_slice(&neighbour_key.0);
                    neighbour_self_key.push(DataValue::from(neighbour_key.1 as i64));
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
                    neighbour_val[0]
                        .get_float()
                        .unwrap_or_else(|| unreachable!())
                        - 1.,
                );
                self.store_tx.put(
                    &idx_table.encode_key_for_store(&neighbour_self_key, Default::default())?,
                    &idx_table.encode_val_only_for_store(&neighbour_val, Default::default())?,
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
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let bottom_level = ep[0].get_int().unwrap_or_else(|| unreachable!());
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
}
