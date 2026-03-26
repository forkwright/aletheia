//! HNSW index types: HnswIndexManifest and VectorCache.

use std::num::NonZeroUsize;

use compact_str::CompactString;
use lru::LruCache;
use rand::Rng;

use crate::DataValue;
use crate::data::relation::VecElementType;
use crate::data::tuple::Tuple;
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::parse::sys::HnswDistance;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

pub(crate) const DEFAULT_VECTOR_CACHE_CAPACITY: usize = 10_000;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HnswIndexManifest {
    pub(crate) base_relation: CompactString,
    pub(crate) index_name: CompactString,
    pub(crate) vec_dim: usize,
    pub(crate) dtype: VecElementType,
    pub(crate) vec_fields: Vec<usize>,
    pub(crate) distance: HnswDistance,
    pub(crate) ef_construction: usize,
    pub(crate) m_neighbours: usize,
    pub(crate) m_max: usize,
    pub(crate) m_max0: usize,
    pub(crate) level_multiplier: f64,
    pub(crate) index_filter: Option<String>,
    pub(crate) extend_candidates: bool,
    pub(crate) keep_pruned_connections: bool,
    /// Maximum number of vectors allowed in this index.
    ///
    /// When `Some(n)`, insertions that would exceed `n` are rejected and a
    /// warning is logged at 80 % utilisation. `None` means no limit (#1722).
    #[serde(default)]
    pub(crate) max_vectors: Option<usize>,
}

impl HnswIndexManifest {
    pub(crate) fn get_random_level(&self) -> i64 {
        let mut rng = rand::rng();
        let uniform_num: f64 = rng.random_range(0.0..1.0);
        let r = -uniform_num.ln() * self.level_multiplier;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "floor of bounded float fits in i64"
        )]
        {
            -(r.floor() as i64)
        }
    }
}

pub(crate) type CompoundKey = (Tuple, usize, i32);

pub(crate) struct VectorCache {
    pub(super) cache: LruCache<CompoundKey, Vector>,
    distance: HnswDistance,
}

impl VectorCache {
    pub(crate) fn new(distance: HnswDistance, capacity: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap_or_else(|| unreachable!())),
            distance,
        }
    }
    pub(crate) fn insert(&mut self, k: CompoundKey, v: Vector) {
        self.cache.put(k, v);
    }
    pub(super) fn dist(&self, v1: &Vector, v2: &Vector) -> Result<f64> {
        use ndarray::Zip;
        match self.distance {
            HnswDistance::L2 => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => Ok(f64::from(Zip::from(a).and(b).fold(
                    0.0f32,
                    |acc, &x, &y| {
                        let d = x - y;
                        acc + d * d
                    },
                ))),
                (Vector::F64(a), Vector::F64(b)) => {
                    Ok(Zip::from(a).and(b).fold(0.0f64, |acc, &x, &y| {
                        let d = x - y;
                        acc + d * d
                    }))
                }
                _ => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_l2",
                        reason: format!("Cannot compute L2 distance between {:?} and {:?}", v1, v2),
                    }
                    .build()
                    .into());
                }
            },
            HnswDistance::Cosine => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => {
                    let (a_norm, b_norm, dot) = Zip::from(a)
                        .and(b)
                        .fold((0.0f32, 0.0f32, 0.0f32), |(an, bn, d), &x, &y| {
                            (an + x * x, bn + y * y, d + x * y)
                        });
                    Ok(1.0 - f64::from(dot) / (f64::from(a_norm) * f64::from(b_norm)).sqrt())
                }
                (Vector::F64(a), Vector::F64(b)) => {
                    let (a_norm, b_norm, dot) = Zip::from(a)
                        .and(b)
                        .fold((0.0f64, 0.0f64, 0.0f64), |(an, bn, d), &x, &y| {
                            (an + x * x, bn + y * y, d + x * y)
                        });
                    Ok(1.0 - dot / (a_norm * b_norm).sqrt())
                }
                _ => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_cosine",
                        reason: format!(
                            "Cannot compute cosine distance between {:?} and {:?}",
                            v1, v2
                        ),
                    }
                    .build()
                    .into());
                }
            },
            HnswDistance::InnerProduct => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => {
                    let dot = a.dot(b);
                    Ok(1. - f64::from(dot))
                }
                (Vector::F64(a), Vector::F64(b)) => {
                    let dot = a.dot(b);
                    Ok(1. - dot)
                }
                _ => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_ip",
                        reason: format!(
                            "Cannot compute inner product between {:?} and {:?}",
                            v1, v2
                        ),
                    }
                    .build()
                    .into());
                }
            },
        }
    }
    // INVARIANT: callers must call ensure_key() before v_dist/k_dist/get_key.
    // The cache is guaranteed to contain the key after ensure_key succeeds
    // (though LRU eviction may have removed it if capacity is very small and
    // many keys were ensured between the ensure and the access: callers that
    // need multiple keys should ensure them close to their use site).
    pub(crate) fn v_dist(&mut self, v: &Vector, key: &CompoundKey) -> Result<f64> {
        let v2 = self.cache.peek(key).unwrap_or_else(|| unreachable!());
        self.dist(v, v2)
    }
    pub(crate) fn k_dist(&mut self, k1: &CompoundKey, k2: &CompoundKey) -> Result<f64> {
        // WHY: Clone to avoid overlapping borrows on the cache.
        let v1 = self
            .cache
            .peek(k1)
            .unwrap_or_else(|| unreachable!())
            .clone();
        let v2 = self.cache.peek(k2).unwrap_or_else(|| unreachable!());
        self.dist(&v1, v2)
    }
    pub(crate) fn get_key(&mut self, key: &CompoundKey) -> &Vector {
        self.cache.peek(key).unwrap_or_else(|| unreachable!())
    }
    pub(crate) fn ensure_key(
        &mut self,
        key: &CompoundKey,
        handle: &RelationHandle,
        tx: &SessionTx<'_>,
    ) -> Result<()> {
        if !self.cache.contains(key) {
            match handle.get(tx, &key.0)? {
                Some(tuple) => {
                    let mut field = &tuple[key.1];
                    if key.2 >= 0 {
                        match field {
                            DataValue::List(l) => {
                                #[expect(clippy::cast_sign_loss, reason = "guarded by >= 0 check")]
                                let sub = key.2 as usize;
                                field = &l[sub];
                            }
                            _ => {
                                return Err(InvalidOperationSnafu {
                                    op: "hnsw_index",
                                    reason: format!("Cannot interpret {} as list", field),
                                }
                                .build()
                                .into());
                            }
                        }
                    }
                    match field {
                        DataValue::Vec(v) => {
                            self.cache.put(key.clone(), v.clone());
                        }
                        _ => {
                            return Err(InvalidOperationSnafu {
                                op: "hnsw_index",
                                reason: format!("Cannot interpret {} as vector", field),
                            }
                            .build()
                            .into());
                        }
                    }
                }
                None => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: format!("Cannot find compound key for HNSW: {:?}", key),
                    }
                    .build()
                    .into());
                }
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.cache.len()
    }
}
