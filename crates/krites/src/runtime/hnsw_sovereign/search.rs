//! `SessionTx` method for HNSW KNN search: the query-time entry point the
//! `~idx{... | query: ..., k: ..., ef: ...}` form compiles down to.

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;

use super::types::{DEFAULT_VECTOR_CACHE_CAPACITY, VectorCache};
use super::visited_pool::VisitedPool;
use crate::data::expr::{Bytecode, eval_bytecode_pred};
use crate::data::program::HnswSearch;
use crate::data::relation::VecElementType;
use crate::data::tuple::Tuple;
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::transact::SessionTx;
use crate::{DataValue, SourceSpan};

impl SessionTx<'_> {
    /// K-nearest-neighbour search over an HNSW index: converts the query
    /// vector to the index's stored dtype, greedily descends from the
    /// current entry point through the upper (sparser) levels with a
    /// single-candidate beam, then runs the real beam width (`config.ef`)
    /// at level 0, and finally shapes the output tuples per the requested
    /// bindings.
    ///
    /// # Complexity
    ///
    /// `O(log n * ef)`.
    pub(crate) fn hnsw_knn(
        &self,
        q: Vector,
        config: &HnswSearch,
        filter_bytecode: &Option<(Vec<Bytecode>, SourceSpan)>,
        stack: &mut Vec<DataValue>,
    ) -> Result<Vec<Tuple>> {
        if q.len() != config.manifest.vec_dim {
            return Err(InvalidOperationSnafu {
                op: "hnsw_query",
                reason: "query vector dimension mismatch".to_string(),
            }
            .build()
            .into());
        }
        let q = convert_query_dtype(q, config.manifest.dtype);

        let mut vec_cache = VectorCache::new(config.manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);
        let key_len = config.base_handle.metadata.keys.len();

        let Some(ep) = config
            .idx_handle
            .scan_bounded_prefix(
                self,
                &[],
                &[DataValue::from(i64::MIN)],
                &[DataValue::from(1)],
            )
            .next()
            .transpose()?
        else {
            return Ok(vec![]);
        };

        let bottom_level = ep[0].get_int().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_search",
                reason: "entry point level is not an integer".to_string(),
            }
            .build()
        })?;
        let Some(ep_field_raw) = ep[key_len + 1].get_int() else {
            return Ok(vec![]);
        };
        let ep_field = usize::try_from(ep_field_raw).map_err(|_e| {
            InvalidOperationSnafu {
                op: "hnsw_read",
                reason: "entry point field out of range",
            }
            .build()
        })?;
        let ep_subidx_raw = ep[key_len + 2].get_int().ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_search",
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
        let ep_key = (ep[1..key_len + 1].to_vec(), ep_field, ep_subidx);

        vec_cache.ensure_key(&ep_key, &config.base_handle, self)?;
        let mut found_nn = PriorityQueue::new();
        found_nn.push(ep_key.clone(), OrderedFloat(vec_cache.v_dist(&q, &ep_key)?));

        let pool = VisitedPool::with_defaults();
        for level in bottom_level..0 {
            self.hnsw_search_level_pooled(
                &q,
                1,
                level,
                &config.base_handle,
                &config.idx_handle,
                &mut found_nn,
                &mut vec_cache,
                Some(&pool),
            )?;
        }
        self.hnsw_search_level_pooled(
            &q,
            config.ef,
            0,
            &config.base_handle,
            &config.idx_handle,
            &mut found_nn,
            &mut vec_cache,
            Some(&pool),
        )?;
        if found_nn.is_empty() {
            return Ok(vec![]);
        }

        // No query-time filter: the graph search already produced exactly
        // the candidate set we want, so trim to k up front (discards the
        // worst first — this is a max-heap on distance). With a filter,
        // some candidates will be excluded below, so keep every candidate
        // the beam found and truncate only after filtering.
        if config.filter.is_none() {
            while found_nn.len() > config.k {
                found_nn.pop();
            }
        }

        let mut ret = Vec::with_capacity(found_nn.len());
        while let Some((cand_key, OrderedFloat(distance))) = found_nn.pop() {
            if let Some(radius) = config.radius
                && distance > radius
            {
                continue;
            }

            let Some(mut cand_tuple) = config.base_handle.get(self, &cand_key.0)? else {
                return Err(crate::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "hnsw_query",
                        reason: "corrupted index",
                    }
                    .build(),
                });
            };

            if config.bind_field.is_some() {
                let field_name = if cand_key.1 < key_len {
                    config.base_handle.metadata.keys[cand_key.1].name.clone()
                } else {
                    config.base_handle.metadata.non_keys[cand_key.1 - key_len].name.clone()
                };
                cand_tuple.push(DataValue::Str(field_name));
            }
            if config.bind_field_idx.is_some() {
                cand_tuple.push(if cand_key.2 < 0 {
                    DataValue::Null
                } else {
                    DataValue::from(i64::from(cand_key.2))
                });
            }
            if config.bind_distance.is_some() {
                cand_tuple.push(DataValue::from(distance));
            }
            if config.bind_vector.is_some() {
                let vec_val = if cand_key.2 < 0 {
                    cand_tuple[cand_key.1].clone()
                } else {
                    let DataValue::List(l) = &cand_tuple[cand_key.1] else {
                        return Err(InvalidOperationSnafu {
                            op: "hnsw_index",
                            reason: format!("corrupted index value {:?}", cand_tuple[cand_key.1]),
                        }
                        .build()
                        .into());
                    };
                    #[expect(clippy::cast_sign_loss, reason = "guarded by the < 0 check above")]
                    let sub = cand_key.2 as usize;
                    l[sub].clone()
                };
                cand_tuple.push(vec_val);
            }

            if let Some((code, span)) = filter_bytecode
                && !eval_bytecode_pred(code, &cand_tuple, stack, *span)?
            {
                continue;
            }

            ret.push(cand_tuple);
        }
        ret.reverse();
        ret.truncate(config.k);
        Ok(ret)
    }
}

/// Convert the query vector to the index's stored element type, if they
/// differ. `F64 -> F32` intentionally loses precision (the index stores
/// `F32`, so the query must meet it there); `F32 -> F64` widens exactly.
fn convert_query_dtype(q: Vector, dtype: VecElementType) -> Vector {
    match (q, dtype) {
        (v @ Vector::F32(_), VecElementType::F32) | (v @ Vector::F64(_), VecElementType::F64) => v,
        (Vector::F32(v), VecElementType::F64) => Vector::F64(v.mapv(f64::from)),
        (Vector::F64(v), VecElementType::F32) => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "f64 to f32: intentional precision reduction to match the index dtype"
            )]
            let narrowed = v.mapv(|x| x as f32);
            Vector::F32(narrowed)
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use crate::DbInstance;

    fn setup_db() -> DbInstance {
        let db = DbInstance::default();
        db.run_default(":create vectors { id: Int => vec: <F32; 4> }").unwrap();
        db.run_default(
            r"::hnsw create vectors:idx {
                dim: 4, m: 16, dtype: F32, fields: [vec], distance: L2,
                ef_construction: 50, extend_candidates: false, keep_pruned_connections: false,
            }",
        )
        .unwrap();
        db
    }

    #[test]
    fn empty_index_returns_no_results() {
        let db = setup_db();
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([1.0, 2.0, 3.0, 4.0]), k: 5, ef: 50, bind_distance: dist}",
            )
            .unwrap();
        assert!(res.rows.is_empty());
    }

    #[test]
    fn exact_match_is_top_result() {
        let db = setup_db();
        for i in 0..20 {
            #[expect(clippy::cast_precision_loss, reason = "test fixture")]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 3, ef: 50, bind_distance: dist}",
            )
            .unwrap();
        assert!(!res.rows.is_empty());
        assert!(res.rows.len() <= 3);
        let ids: Vec<i64> = res.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(ids.contains(&5));
    }

    #[test]
    fn results_are_ordered_by_ascending_distance() {
        let db = setup_db();
        for i in 0..50 {
            #[expect(clippy::cast_precision_loss, reason = "test fixture")]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, 0.0, 0.0, 0.0])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([25.0, 0.0, 0.0, 0.0]), k: 10, ef: 50, bind_distance: dist} :order dist",
            )
            .unwrap();
        let distances: Vec<f64> = res.rows.iter().filter_map(|r| r[1].get_float()).collect();
        assert!(!distances.is_empty());
        for window in distances.windows(2) {
            assert!(window[0] <= window[1]);
        }
    }

    #[test]
    fn deleted_vector_excluded_from_results() {
        let db = setup_db();
        for i in 0..10 {
            #[expect(clippy::cast_precision_loss, reason = "test fixture")]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        db.run_default("?[id] <- [[5]] :rm vectors {}").unwrap();
        let res = db
            .run_default(
                r"?[id, dist] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 3, ef: 50, bind_distance: dist}",
            )
            .unwrap();
        let ids: Vec<i64> = res.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(!ids.contains(&5));
    }

    /// SC-1: a zero-magnitude vector under cosine distance must never
    /// poison a query with NaN/Inf — it must be treated as maximally
    /// distant, not crash or silently misorder every other result.
    #[test]
    fn cosine_zero_vector_does_not_produce_nan() {
        let db = DbInstance::default();
        db.run_default(":create c { id: Int => vec: <F32; 3> }").unwrap();
        db.run_default(
            r"::hnsw create c:idx {
                dim: 3, m: 8, dtype: F32, fields: [vec], distance: Cosine, ef_construction: 20,
                extend_candidates: false, keep_pruned_connections: false,
            }",
        )
        .unwrap();
        db.run_default("?[id, vec] <- [[0, vec([0.0, 0.0, 0.0])]] :put c {}").unwrap();
        db.run_default("?[id, vec] <- [[1, vec([1.0, 0.0, 0.0])]] :put c {}").unwrap();
        let res = db
            .run_default(
                r"?[id, dist] := ~c:idx{id | query: vec([1.0, 0.0, 0.0]), k: 2, ef: 20, bind_distance: dist} :order dist",
            )
            .unwrap();
        assert_eq!(res.rows.len(), 2);
        for row in &res.rows {
            let dist = row[1].get_float().unwrap();
            assert!(dist.is_finite(), "distance must never be NaN/Inf, got {dist}");
            assert!((0.0..=2.0).contains(&dist), "cosine distance must land in [0,2], got {dist}");
        }
        // The exact-parallel vector must sort ahead of the zero vector.
        assert_eq!(res.rows[0][0].get_int().unwrap(), 1);
    }
}
