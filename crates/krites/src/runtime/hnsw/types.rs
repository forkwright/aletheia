// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

//! HNSW index types: HnswIndexManifest and VectorCache.

use std::num::NonZeroUsize;

use compact_str::CompactString;
use lru::LruCache;
use rand::RngExt;

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
            // INVARIANT: capacity is validated as positive at config time
            cache: LruCache::new(
                NonZeroUsize::new(capacity)
                    .unwrap_or_else(|| unreachable!("vector cache capacity must be non-zero")),
            ),
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
                    #[expect(
                        clippy::needless_return,
                        reason = "explicit return for early exit in match arm"
                    )]
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
                    #[expect(
                        clippy::needless_return,
                        reason = "explicit return for early exit in match arm"
                    )]
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
                    #[expect(
                        clippy::needless_return,
                        reason = "explicit return for early exit in match arm"
                    )]
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
    pub(crate) fn v_dist(&self, v: &Vector, key: &CompoundKey) -> Result<f64> {
        let v2 = self.cache.peek(key).ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_cache",
                reason: "vector not found in cache after ensure_key".to_string(),
            }
            .build()
        })?;
        self.dist(v, v2)
    }
    pub(crate) fn k_dist(&self, k1: &CompoundKey, k2: &CompoundKey) -> Result<f64> {
        // WHY: Clone to avoid overlapping borrows on the cache.
        let v1 = self
            .cache
            .peek(k1)
            .ok_or_else(|| {
                InvalidOperationSnafu {
                    op: "hnsw_cache",
                    reason: "vector k1 not found in cache after ensure_key".to_string(),
                }
                .build()
            })?
            .clone();
        let v2 = self.cache.peek(k2).ok_or_else(|| {
            InvalidOperationSnafu {
                op: "hnsw_cache",
                reason: "vector k2 not found in cache after ensure_key".to_string(),
            }
            .build()
        })?;
        self.dist(&v1, v2)
    }
    pub(crate) fn get_key(&self, key: &CompoundKey) -> &Vector {
        // INVARIANT: callers must call ensure_key() before get_key
        self.cache
            .peek(key)
            .unwrap_or_else(|| unreachable!("vector not found in cache; ensure_key was not called"))
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

/// Enumerate every vector currently in the index: one [`CompoundKey`] per
/// level-0 self-entry (`fr == to`), which every indexed vector carries
/// exactly one of. Insertion always reaches level 0, whether through the
/// fresh-node path (`hnsw_put_fresh_at_levels`) or the main per-level
/// connection loop (`hnsw_put_vector`).
///
/// NOTE: `hnsw_count_vectors`, `hnsw_check_consistency`, and
/// `hnsw_exact_knn` previously scanned the `level = 1` prefix instead —
/// that level holds only the single per-index entry-point marker row,
/// never a real vector, so the scan returned at most one row regardless
/// of index size (#6642).
pub(super) fn scan_indexed_keys<'a>(
    tx: &'a SessionTx<'_>,
    orig_table: &RelationHandle,
    idx_table: &RelationHandle,
) -> impl Iterator<Item = CompoundKey> + 'a {
    let key_len = orig_table.metadata.keys.len();
    idx_table
        .scan_prefix(tx, &vec![DataValue::from(0_i64)])
        .filter_map(move |res| {
            let tuple = res.ok()?;
            let fr = tuple.get(1..key_len + 3)?;
            let to = tuple.get(key_len + 3..2 * key_len + 5)?;
            if fr != to {
                return None;
            }
            let tuple_key: Tuple = tuple.get(1..key_len + 1)?.to_vec();
            let idx = usize::try_from(tuple.get(key_len + 1)?.get_int()?).ok()?;
            let subidx = i32::try_from(tuple.get(key_len + 2)?.get_int()?).ok()?;
            Some((tuple_key, idx, subidx))
        })
}
