//! HNSW index types: manifest, vector cache, distance, and the shared
//! index-relation key-tuple layout.
//!
//! ## On-disk key layout (byte-compatible with `super::hnsw`)
//!
//! The index relation's key columns are, for a base relation with `K` key
//! columns: `[layer, fr_key(K), fr__field, fr__sub_idx, to_key(K),
//! to__field, to__sub_idx]` (length `2*K + 5`). Value columns are `[dist,
//! hash, ignore_link]`.
//!
//! Three row shapes share that one layout:
//!
//! - **Edge row** — `fr` and `to` halves name different vectors: a directed
//!   graph connection. Value = `[distance, Null, deleted_flag]`.
//! - **Self-entry row** — `fr` and `to` halves are identical: per-vector,
//!   per-level bookkeeping (degree in value slot 0, content hash in value
//!   slot 1 for dedup, deletion flag in slot 2). Written at every level a
//!   vector participates in, always including level 0 — so a level-0 scan
//!   filtered to `fr == to` enumerates every indexed vector exactly once.
//! - **Entry-point row (the canary node)** — `layer = 1` (a level no real
//!   vector ever occupies: [`HnswIndexManifest::get_random_level`] only
//!   produces levels `<= 0`), both halves entirely `Null`. Exactly one such
//!   row exists per index; it is overwritten in place whenever the current
//!   entry point changes. Value slot 0 repurposes the "dist" column to hold
//!   the graph's current top level (an `Int`, not a `Float`); slot 1 holds
//!   an opaque reference to the entry-point vector's own self-entry key
//!   bytes (write-only bookkeeping — never decoded back into a lookup).

use std::num::NonZeroUsize;

use compact_str::CompactString;
use lru::LruCache;
use rand::RngExt;

use crate::DataValue;
use crate::data::relation::VecElementType;
use crate::data::tuple::{ENCODED_KEY_MIN_LEN, Tuple};
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::parse::sys::HnswDistance;
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

use super::idx_to_i64;

pub(crate) const DEFAULT_VECTOR_CACHE_CAPACITY: usize = 10_000;

/// Configuration and tuning parameters for one HNSW index.
///
/// Field order is part of the on-disk contract: this struct is
/// `rmp_serde`-serialized (as a positional array, not a map) inside the
/// owning relation's system metadata, so reordering, adding, or removing a
/// field changes the bytes a fresh `Db::open_fjall` reads back. Keep this
/// layout identical to `super::hnsw::HnswIndexManifest`.
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
    /// Maximum number of vectors allowed in this index. `Some(n)` rejects
    /// insertions once `n` is reached and logs a warning at 80% utilisation;
    /// `None` means unbounded (#1722).
    #[serde(default)]
    pub(crate) max_vectors: Option<usize>,
}

impl HnswIndexManifest {
    /// Draw a random insertion level from HNSW's geometric-decay
    /// distribution: `level = -floor(-ln(U) * level_multiplier)`, `U ~
    /// Uniform(0, 1)`. Always non-positive; level 0 is by far the most
    /// common draw, with exponentially decreasing probability mass at each
    /// level further from zero.
    pub(crate) fn get_random_level(&self) -> i64 {
        let uniform: f64 = rand::rng().random_range(0.0..1.0);
        let unbounded = -uniform.ln() * self.level_multiplier;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "floor of a bounded non-negative float fits in i64"
        )]
        {
            -(unbounded.floor() as i64)
        }
    }
}

/// `(base-relation key tuple, vector-field index, sub-index)`. `sub_idx` is
/// `-1` when the field itself is a vector, or the position within a `List`
/// of vectors otherwise.
pub(crate) type CompoundKey = (Tuple, usize, i32);

/// Build a self-entry (degree/existence) key: `[level, key.., idx, subidx,
/// key.., idx, subidx]` — the `fr == to` shape described on the module.
pub(super) fn self_entry_key(
    level: i64,
    key: &[DataValue],
    idx: usize,
    subidx: i32,
) -> Vec<DataValue> {
    let mut out = Vec::with_capacity(key.len() * 2 + 5);
    out.push(DataValue::from(level));
    for _ in 0..2 {
        out.extend_from_slice(key);
        out.push(DataValue::from(idx_to_i64(idx)));
        out.push(DataValue::from(i64::from(subidx)));
    }
    out
}

/// Build a directed edge key: `[level, from.., to..]`.
pub(super) fn edge_key(level: i64, from: &CompoundKey, to: &CompoundKey) -> Vec<DataValue> {
    let mut out = Vec::with_capacity(from.0.len() * 2 + 5);
    out.push(DataValue::from(level));
    out.extend_from_slice(&from.0);
    out.push(DataValue::from(idx_to_i64(from.1)));
    out.push(DataValue::from(i64::from(from.2)));
    out.extend_from_slice(&to.0);
    out.push(DataValue::from(idx_to_i64(to.1)));
    out.push(DataValue::from(i64::from(to.2)));
    out
}

/// The reserved level for the single entry-point marker row. No real vector
/// ever occupies this level ([`HnswIndexManifest::get_random_level`] only
/// draws levels `<= 0`), so it can never collide with a self-entry or edge.
pub(super) const ENTRY_POINT_LEVEL: i64 = 1;

/// Build the entry-point marker's key: `[1, Null * (2*(key_len+2))]` — the
/// canary node. `key_len` is the base relation's key-column count.
pub(super) fn entry_point_key(key_len: usize) -> Vec<DataValue> {
    let mut out = Vec::with_capacity(2 * (key_len + 2) + 1);
    out.push(DataValue::from(ENTRY_POINT_LEVEL));
    for _ in 0..2 * (key_len + 2) {
        out.push(DataValue::Null);
    }
    out
}

/// Enumerate every vector currently in the index: one [`CompoundKey`] per
/// level-0 self-entry (`fr == to`), which every indexed vector carries
/// exactly one of — [`super::put`]'s insertion path always reaches level 0,
/// whether via the fresh-node path or the main per-level connection loop.
///
/// This is the correct basis for "every indexed vector" (E24's orphan
/// check, `max_vectors` accounting, and the brute-force flat-search oracle
/// all need it): the derived implementation instead scanned the
/// entry-point marker's reserved level (1), which holds exactly one row
/// total regardless of index size and is never a real vector — see
/// `hnsw_count_vectors`'s doc comment in `put.rs` for the fix this
/// replaces.
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
            if tuple[1..key_len + 3] != tuple[key_len + 3..2 * key_len + 5] {
                return None;
            }
            #[expect(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "HNSW field/sub-index values are non-negative and bounded by m_max"
            )]
            let field = tuple[key_len + 1]
                .get_int()
                .unwrap_or_else(|| unreachable!("HNSW self-entry field is not an integer"))
                as usize;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "HNSW sub-index bounded by m_max (< i32::MAX)"
            )]
            let subidx = tuple[key_len + 2]
                .get_int()
                .unwrap_or_else(|| unreachable!("HNSW self-entry sub-index is not an integer"))
                as i32;
            let key = tuple[1..key_len + 1].to_vec();
            Some((key, field, subidx))
        })
}

/// Deserialize a raw index-relation value blob (as returned by a direct
/// `store_tx.get`, key prefix included) back into its `[dist,
/// hash_or_null, deleted]` value tuple.
pub(super) fn decode_edge_value(bytes: &[u8]) -> Result<Vec<DataValue>> {
    rmp_serde::from_slice(&bytes[ENCODED_KEY_MIN_LEN..]).map_err(|e| {
        crate::error::InternalError::Runtime {
            source: InvalidOperationSnafu {
                op: "hnsw_index",
                reason: e.to_string(),
            }
            .build(),
        }
    })
}

/// Cosine distance from raw squared-norms and a dot product, defined for
/// every real input (SC-1). Two guards the naive `1.0 - dot /
/// sqrt(a_norm_sq * b_norm_sq)` formula lacks:
///
/// - a zero-magnitude operand carries no direction to compare against, so
///   it reports maximum distance (`2.0`) instead of dividing by zero into
///   NaN — an embedding provider occasionally emits an all-zero vector
///   (e.g. a filtered-out or unembeddable chunk), and a NaN distance would
///   corrupt every priority-queue comparison it touches;
/// - the cosine similarity is clamped to `[-1.0, 1.0]` before conversion,
///   so floating-point rounding on near-parallel vectors can never push the
///   result a hair below `0.0`.
///
/// Shared by every distance call site in this module, including the
/// brute-force flat search in [`super::adaptive`] — the graph search and
/// its exact-kNN oracle must agree on distance or a recall comparison
/// between them is meaningless.
pub(super) fn cosine_distance(a_norm_sq: f64, b_norm_sq: f64, dot: f64) -> f64 {
    let denom = (a_norm_sq * b_norm_sq).sqrt();
    if denom <= 0.0 {
        return 2.0;
    }
    let cos_sim = (dot / denom).clamp(-1.0, 1.0);
    1.0 - cos_sim
}

pub(crate) struct VectorCache {
    pub(super) cache: LruCache<CompoundKey, Vector>,
    distance: HnswDistance,
}

impl VectorCache {
    pub(crate) fn new(distance: HnswDistance, capacity: usize) -> Self {
        Self {
            // INVARIANT: capacity is validated positive by every call site
            // (a fixed default or a config value checked at index-create
            // time), so NonZeroUsize::new never actually fails.
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
                _ => Err(InvalidOperationSnafu {
                    op: "hnsw_l2",
                    reason: format!("cannot compute L2 distance between {:?} and {:?}", v1, v2),
                }
                .build()
                .into()),
            },
            HnswDistance::Cosine => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => {
                    let (a_norm, b_norm, dot) = Zip::from(a)
                        .and(b)
                        .fold((0.0f32, 0.0f32, 0.0f32), |(an, bn, d), &x, &y| {
                            (an + x * x, bn + y * y, d + x * y)
                        });
                    Ok(cosine_distance(
                        f64::from(a_norm),
                        f64::from(b_norm),
                        f64::from(dot),
                    ))
                }
                (Vector::F64(a), Vector::F64(b)) => {
                    let (a_norm, b_norm, dot) = Zip::from(a)
                        .and(b)
                        .fold((0.0f64, 0.0f64, 0.0f64), |(an, bn, d), &x, &y| {
                            (an + x * x, bn + y * y, d + x * y)
                        });
                    Ok(cosine_distance(a_norm, b_norm, dot))
                }
                _ => Err(InvalidOperationSnafu {
                    op: "hnsw_cosine",
                    reason: format!(
                        "cannot compute cosine distance between {:?} and {:?}",
                        v1, v2
                    ),
                }
                .build()
                .into()),
            },
            HnswDistance::InnerProduct => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => Ok(1. - f64::from(a.dot(b))),
                (Vector::F64(a), Vector::F64(b)) => Ok(1. - a.dot(b)),
                _ => Err(InvalidOperationSnafu {
                    op: "hnsw_ip",
                    reason: format!("cannot compute inner product between {:?} and {:?}", v1, v2),
                }
                .build()
                .into()),
            },
        }
    }

    // INVARIANT: callers must call ensure_key() before v_dist/k_dist/get_key.
    // The cache holds the key after ensure_key succeeds, modulo LRU
    // eviction if capacity is very small and many other keys were ensured
    // in between — callers needing several keys at once should ensure them
    // close together.
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
        // WHY: clone to avoid overlapping immutable borrows of the same cache.
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
        // INVARIANT: callers must call ensure_key() before get_key.
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
        if self.cache.contains(key) {
            return Ok(());
        }
        let Some(tuple) = handle.get(tx, &key.0)? else {
            return Err(InvalidOperationSnafu {
                op: "hnsw_index",
                reason: format!("cannot find compound key for HNSW: {:?}", key),
            }
            .build()
            .into());
        };
        let mut field = &tuple[key.1];
        if key.2 >= 0 {
            let DataValue::List(l) = field else {
                return Err(InvalidOperationSnafu {
                    op: "hnsw_index",
                    reason: format!("cannot interpret {} as list", field),
                }
                .build()
                .into());
            };
            #[expect(clippy::cast_sign_loss, reason = "guarded by the >= 0 check above")]
            let sub = key.2 as usize;
            field = &l[sub];
        }
        let DataValue::Vec(v) = field else {
            return Err(InvalidOperationSnafu {
                op: "hnsw_index",
                reason: format!("cannot interpret {} as vector", field),
            }
            .build()
            .into());
        };
        self.cache.put(key.clone(), v.clone());
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.cache.len()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn dummy_key(i: i64) -> CompoundKey {
        (vec![DataValue::from(i)], 0, -1)
    }

    #[test]
    fn cache_is_bounded_at_capacity() {
        let mut cache = VectorCache::new(HnswDistance::L2, 10);
        for i in 0..20 {
            cache.insert(dummy_key(i), Vector::F64(ndarray::Array1::zeros(4)));
        }
        assert_eq!(
            cache.len(),
            10,
            "cache must stay bounded at its configured capacity"
        );
    }

    #[test]
    fn cache_evicts_least_recently_used() {
        let mut cache = VectorCache::new(HnswDistance::L2, 5);
        for i in 0..10 {
            cache.insert(dummy_key(i), Vector::F64(ndarray::Array1::zeros(4)));
        }
        for i in 5..10 {
            assert!(
                cache.cache.contains(&dummy_key(i)),
                "recently-inserted key {i} should remain"
            );
        }
        for i in 0..5 {
            assert!(
                !cache.cache.contains(&dummy_key(i)),
                "oldest key {i} should have been evicted"
            );
        }
    }

    #[test]
    fn mismatched_vector_types_are_a_distance_error() {
        let cache = VectorCache::new(HnswDistance::L2, 10);
        let v1 = Vector::F32(ndarray::Array1::from_vec(vec![1.0f32, 2.0]));
        let v2 = Vector::F64(ndarray::Array1::from_vec(vec![1.0f64, 2.0]));
        assert!(cache.dist(&v1, &v2).is_err());
    }

    #[test]
    fn l2_distance_is_squared_euclidean() {
        let cache = VectorCache::new(HnswDistance::L2, 10);
        let a = Vector::F64(ndarray::Array1::from_vec(vec![0.0, 0.0]));
        let b = Vector::F64(ndarray::Array1::from_vec(vec![3.0, 4.0]));
        let d = cache.dist(&a, &b).unwrap();
        assert!(
            (d - 25.0).abs() < 1e-10,
            "3-4-5 triangle: squared distance should be 25.0, got {d}"
        );
    }

    #[test]
    fn cosine_distance_of_identical_vectors_is_zero() {
        let cache = VectorCache::new(HnswDistance::Cosine, 10);
        let v = Vector::F64(ndarray::Array1::from_vec(vec![1.0, 0.0, 0.0]));
        let d = cache.dist(&v, &v).unwrap();
        assert!(
            d.abs() < 1e-10,
            "identical vectors must have ~0 cosine distance, got {d}"
        );
    }

    /// SC-1: a zero-magnitude vector has no direction to compare — clamp to
    /// maximum distance instead of dividing by zero into NaN.
    #[test]
    fn cosine_distance_zero_vector_is_clamped_not_nan() {
        let cache = VectorCache::new(HnswDistance::Cosine, 10);
        let zero = Vector::F64(ndarray::Array1::zeros(3));
        let other = Vector::F64(ndarray::Array1::from_vec(vec![1.0, 2.0, 3.0]));
        let d = cache.dist(&zero, &other).unwrap();
        assert!(
            d.is_finite(),
            "zero-vector cosine distance must be finite, got {d}"
        );
        assert!((0.0..=2.0).contains(&d));
    }

    /// SC-1: floating-point rounding on near-parallel vectors must never
    /// push the clamped result below zero.
    #[test]
    fn cosine_distance_never_goes_negative() {
        let cache = VectorCache::new(HnswDistance::Cosine, 10);
        let a = Vector::F32(ndarray::Array1::from_vec(vec![1.0f32, 1.0, 1.0]));
        let d = cache.dist(&a, &a).unwrap();
        assert!(
            d >= 0.0,
            "cosine distance of a vector with itself must never be negative, got {d}"
        );
    }

    #[test]
    fn inner_product_distance_is_one_minus_dot() {
        let cache = VectorCache::new(HnswDistance::InnerProduct, 10);
        let a = Vector::F64(ndarray::Array1::from_vec(vec![1.0, 0.0]));
        let b = Vector::F64(ndarray::Array1::from_vec(vec![0.5, 0.5]));
        let d = cache.dist(&a, &b).unwrap();
        assert!(
            (d - 0.5).abs() < 1e-10,
            "1 - dot([1,0],[0.5,0.5]) = 0.5, got {d}"
        );
    }
}
