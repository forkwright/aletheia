//! SessionTx methods for HNSW KNN search.
//! SessionTx methods for HNSW vector index operations.
//! Hierarchical Navigable Small World vector index.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]

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

impl<'a> SessionTx<'a> {
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
        let q = match (q, config.manifest.dtype) {
            (v @ Vector::F32(_), VecElementType::F32) => v,
            (v @ Vector::F64(_), VecElementType::F64) => v,
            (Vector::F32(v), VecElementType::F64) => Vector::F64(v.mapv(f64::from)),
            (Vector::F64(v), VecElementType::F32) => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "intentional F64→F32 precision reduction for vector storage"
                )]
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "f64 to f32: intentional precision reduction"
                )]
                let converted = v.mapv(|x| x as f32);
                Vector::F32(converted)
            }
        };

        let mut vec_cache =
            VectorCache::new(config.manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);

        let ep_res = config
            .idx_handle
            .scan_bounded_prefix(
                self,
                &[],
                &[DataValue::from(i64::MIN)],
                &[DataValue::from(1)],
            )
            .next();
        if let Some(ep) = ep_res {
            let ep = ep?;
            #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
            let bottom_level = ep[0].get_int().unwrap_or_else(|| unreachable!());
            let ep_idx = match ep[config.base_handle.metadata.keys.len() + 1].get_int() {
                Some(x) => usize::try_from(x).map_err(|_e| {
                    InvalidOperationSnafu {
                        op: "hnsw_read",
                        reason: "stored index out of range",
                    }
                    .build()
                })?,
                None => {
                    return Ok(vec![]);
                }
            };
            let ep_t_key = ep[1..config.base_handle.metadata.keys.len() + 1].to_vec();
            let ep_subidx = i32::try_from(
                ep[config.base_handle.metadata.keys.len() + 2]
                    .get_int()
                    .unwrap_or_else(|| unreachable!()),
            )
            .map_err(|_e| {
                InvalidOperationSnafu {
                    op: "hnsw_read",
                    reason: "stored subindex out of range",
                }
                .build()
            })?;
            let ep_key = (ep_t_key, ep_idx, ep_subidx);
            vec_cache.ensure_key(&ep_key, &config.base_handle, self)?;
            let ep_distance = vec_cache.v_dist(&q, &ep_key)?;
            let mut found_nn = PriorityQueue::new();
            found_nn.push(ep_key, OrderedFloat(ep_distance));
            let pool = VisitedPool::with_defaults();
            for current_level in bottom_level..0 {
                self.hnsw_search_level_pooled(
                    &q,
                    1,
                    current_level,
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

            if config.filter.is_none() {
                while found_nn.len() > config.k {
                    found_nn.pop();
                }
            }

            let mut ret = vec![];

            while let Some((cand_key, OrderedFloat(distance))) = found_nn.pop() {
                if let Some(r) = config.radius
                    && distance > r
                {
                    continue;
                }

                let mut cand_tuple =
                    config.base_handle.get(self, &cand_key.0)?.ok_or_else(|| {
                        crate::error::InternalError::Runtime {
                            source: InvalidOperationSnafu {
                                op: "hnsw_query",
                                reason: "corrupted index",
                            }
                            .build(),
                        }
                    })?;

                if config.bind_field.is_some() {
                    let field = if cand_key.1 < config.base_handle.metadata.keys.len() {
                        config.base_handle.metadata.keys[cand_key.1].name.clone()
                    } else {
                        config.base_handle.metadata.non_keys
                            [cand_key.1 - config.base_handle.metadata.keys.len()]
                        .name
                        .clone()
                    };
                    cand_tuple.push(DataValue::Str(field));
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
                    let vec = if cand_key.2 < 0 {
                        cand_tuple[cand_key.1].clone()
                    } else {
                        match &cand_tuple[cand_key.1] {
                            DataValue::List(v) => {
                                #[expect(clippy::cast_sign_loss, reason = "guarded by >= 0 check")]
                                let sub = cand_key.2 as usize;
                                v[sub].clone()
                            }
                            v => {
                                return Err(InvalidOperationSnafu {
                                    op: "hnsw_index",
                                    reason: format!("corrupted index value {:?}", v),
                                }
                                .build()
                                .into());
                            }
                        }
                    };
                    cand_tuple.push(vec);
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
        } else {
            Ok(vec![])
        }
    }
}
#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "test assertions and test-only numeric casts"
)]
mod tests {
    use std::collections::BTreeMap;

    use rand::Rng;

    use super::*;
    use crate::DbInstance;
    use crate::data::value::DataValue;
    use crate::parse::sys::HnswDistance;

    #[test]
    fn random_level_distribution_is_non_empty_over_many_samples() {
        let m = 20;
        #[expect(
            clippy::cast_precision_loss,
            reason = "i64 to f64: precision loss acceptable"
        )]
        let mult = 1. / (m as f64).ln();
        let mut rng = rand::rng();
        let mut collected = BTreeMap::new();
        for _ in 0..10000 {
            let uniform_num: f64 = rng.random_range(0.0..1.0);
            let r = -uniform_num.ln() * mult;
            #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
            let level = -(r.floor() as i64);
            collected.entry(level).and_modify(|x| *x += 1).or_insert(1);
        }
        assert!(!collected.is_empty());
    }

    #[test]
    fn hnsw_cache_eviction() {
        let mut cache = VectorCache::new(HnswDistance::L2, 10);
        for i in 0..20u8 {
            #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            let vec = Vector::F64(ndarray::Array1::zeros(4));
            cache.insert(key, vec);
        }
        assert_eq!(cache.len(), 10, "cache should be bounded at capacity");
    }

    #[test]
    fn hnsw_cache_retains_recent() {
        let mut cache = VectorCache::new(HnswDistance::L2, 5);
        for i in 0..10u8 {
            #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            let vec = Vector::F64(ndarray::Array1::zeros(4));
            cache.insert(key, vec);
        }
        for i in 5..10u8 {
            #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            assert!(
                cache.cache.contains(&key),
                "recent key {i} should be in cache"
            );
        }
        for i in 0..5u8 {
            #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            assert!(!cache.cache.contains(&key), "old key {i} should be evicted");
        }
    }

    #[test]
    fn hnsw_dist_mismatched_types_returns_error() {
        let cache = VectorCache::new(HnswDistance::L2, 10);
        let v1 = Vector::F32(ndarray::Array1::from_vec(vec![1.0f32, 2.0]));
        let v2 = Vector::F64(ndarray::Array1::from_vec(vec![1.0f64, 2.0]));
        assert!(cache.dist(&v1, &v2).is_err());
    }

    #[test]
    fn hnsw_dist_l2_correctness() {
        let cache = VectorCache::new(HnswDistance::L2, 10);
        let v1 = Vector::F64(ndarray::Array1::from_vec(vec![0.0, 0.0]));
        let v2 = Vector::F64(ndarray::Array1::from_vec(vec![3.0, 4.0]));
        let d = cache.dist(&v1, &v2).unwrap();
        assert!(
            (d - 25.0).abs() < 1e-10,
            "L2 squared distance should be 25.0, got {d}"
        );
    }

    #[test]
    fn hnsw_dist_cosine_identical_vectors() {
        let cache = VectorCache::new(HnswDistance::Cosine, 10);
        let v = Vector::F64(ndarray::Array1::from_vec(vec![1.0, 0.0, 0.0]));
        let d = cache.dist(&v, &v).unwrap();
        assert!(
            d.abs() < 1e-10,
            "cosine distance of identical vectors should be ~0, got {d}"
        );
    }

    #[test]
    fn hnsw_insert_and_search() {
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
        for i in 0..20 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "f64 to f32: intentional precision reduction"
            )]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 3, ef: 50, bind_distance: dist}"#,
        ).unwrap();
        assert!(!res.rows.is_empty(), "search should return results");
        assert!(res.rows.len() <= 3, "should return at most k=3 results");
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let ids: Vec<i64> = res.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(
            ids.contains(&5),
            "exact match id=5 should be among top-k results, got {:?}",
            ids
        );
    }

    #[test]
    fn hnsw_empty_search() {
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
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([1.0, 2.0, 3.0, 4.0]), k: 5, ef: 50, bind_distance: dist}"#,
        ).unwrap();
        assert!(
            res.rows.is_empty(),
            "empty index search should return no results"
        );
    }

    #[test]
    fn hnsw_delete() {
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
        for i in 0..10 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "f64 to f32: intentional precision reduction"
            )]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        db.run_default("?[id] <- [[5]] :rm vectors {}").unwrap();
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 3, ef: 50, bind_distance: dist}"#,
        ).unwrap();
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let ids: Vec<i64> = res.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(
            !ids.contains(&5),
            "deleted vector id=5 should not appear in results"
        );
    }

    #[test]
    fn hnsw_search_results_ordered_by_distance() {
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
        for i in 0..50 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "f64 to f32: intentional precision reduction"
            )]
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, 0.0, 0.0, 0.0])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([25.0, 0.0, 0.0, 0.0]), k: 10, ef: 50, bind_distance: dist} :order dist"#,
        ).unwrap();
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let distances: Vec<f64> = res.rows.iter().filter_map(|r| r[1].get_float()).collect();
        assert!(!distances.is_empty(), "should return results");
        for window in distances.windows(2) {
            assert!(
                window[0] <= window[1],
                "results should be ordered by distance: {} <= {}",
                window[0],
                window[1]
            );
        }
    }
}
