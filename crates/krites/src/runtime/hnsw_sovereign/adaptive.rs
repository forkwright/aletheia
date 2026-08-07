//! Adaptive search strategy (exact vs. approximate) and the brute-force
//! exact-kNN flat search that serves as this module's own conformance
//! oracle (E25): for a dataset small enough that a linear scan beats graph
//! traversal on constants and cache locality, exact search is both faster
//! and perfectly recall-1.0 by construction.
//!
//! Not yet wired into [`super::search::SessionTx::hnsw_knn`]'s query path —
//! same as the derived module this replaces, this is infrastructure for a
//! future search-path integration, kept because latent capability is kept,
//! not dropped for being uncalled.
//!
//! `not(test)`-scoped: the tests below exercise everything in this module
//! directly (that is what makes them a meaningful regression check against
//! the derived oracle's enumeration bug — see `hnsw_exact_knn`'s doc
//! comment), so `dead_code` genuinely does not fire under `cfg(test)`; only
//! the production (non-test) build has zero callers.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "infrastructure for future HNSW search-path integration")
)]

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;

use super::types::{CompoundKey, VectorCache, scan_indexed_keys};
use crate::DataValue;
use crate::SourceSpan;
use crate::data::expr::{Bytecode, eval_bytecode_pred};
use crate::data::tuple::Tuple;
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

/// Below this many indexed vectors, exact (brute-force) search is
/// typically faster than HNSW graph traversal, thanks to lower constant
/// factors and perfect cache locality.
pub(crate) const DEFAULT_EXACT_THRESHOLD: usize = 256;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AdaptiveSearchConfig {
    /// Datasets at or below this size use exact search; larger ones use HNSW.
    pub(crate) exact_threshold: usize,
}

impl Default for AdaptiveSearchConfig {
    fn default() -> Self {
        Self {
            exact_threshold: DEFAULT_EXACT_THRESHOLD,
        }
    }
}

impl AdaptiveSearchConfig {
    pub(crate) fn with_threshold(exact_threshold: usize) -> Self {
        Self { exact_threshold }
    }

    #[expect(clippy::trivially_copy_pass_by_ref, reason = "&self is idiomatic")]
    pub(crate) fn should_use_exact(&self, dataset_size: usize) -> bool {
        dataset_size <= self.exact_threshold
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchStrategy {
    Exact,
    Approximate,
}

impl SessionTx<'_> {
    /// Brute-force k-nearest-neighbour search: computes the distance from
    /// `q` to every vector currently indexed and returns the closest `k`,
    /// ascending. Guarantees perfect recall; `O(n)` in index size.
    ///
    /// NOTE: the derived implementation enumerated candidates by scanning
    /// the index's `level = 1` prefix — but that level holds only the
    /// single entry-point marker row (see
    /// [`super::types::ENTRY_POINT_LEVEL`]), never a per-vector row. That
    /// scan therefore matched, at most, the one marker row, which then
    /// always failed the vector-cache lookup and was skipped — so the
    /// derived oracle always returned zero rows, for any query, on any
    /// non-empty index. Fixed here via [`scan_indexed_keys`], which
    /// enumerates real per-vector self-entries — the same fix
    /// `super::put::SessionTx::hnsw_count_vectors` and
    /// `super::put::SessionTx::hnsw_check_consistency` needed for the
    /// identical reason. Uses the same [`super::types::cosine_distance`]
    /// clamp (SC-1) as the graph search, via `VectorCache::v_dist` — an
    /// exact-kNN oracle that disagrees with the approximate search on
    /// distance would make any recall comparison between them meaningless.
    ///
    /// # Complexity
    ///
    /// `O(n)`.
    pub(crate) fn hnsw_exact_knn(
        &self,
        q: &Vector,
        k: usize,
        base_handle: &RelationHandle,
        idx_handle: &RelationHandle,
        vec_cache: &mut VectorCache,
        filter_bytecode: &Option<(Vec<Bytecode>, SourceSpan)>,
        stack: &mut Vec<DataValue>,
        bind_distance: bool,
    ) -> Result<Vec<Tuple>> {
        let mut top_k: PriorityQueue<CompoundKey, OrderedFloat<f64>> = PriorityQueue::new();

        for key in scan_indexed_keys(self, base_handle, idx_handle) {
            if vec_cache.ensure_key(&key, base_handle, self).is_err() {
                // Base row is gone without a matching hnsw_remove (the same
                // condition hnsw_check_consistency reports) — skip rather
                // than fail the whole search.
                continue;
            }
            let Ok(dist) = vec_cache.v_dist(q, &key) else {
                continue;
            };
            top_k.push(key, OrderedFloat(dist));
            if top_k.len() > k {
                top_k.pop();
            }
        }

        let mut ascending = Vec::with_capacity(top_k.len());
        while let Some((key, OrderedFloat(dist))) = top_k.pop() {
            ascending.push((key, dist));
        }
        ascending.reverse();

        let mut ret = Vec::with_capacity(ascending.len());
        for (key, dist) in ascending {
            let Some(mut cand_tuple) = base_handle.get(self, &key.0)? else {
                continue;
            };
            if bind_distance {
                cand_tuple.push(DataValue::from(dist));
            }
            if let Some((code, span)) = filter_bytecode
                && !eval_bytecode_pred(code, &cand_tuple, stack, *span)?
            {
                continue;
            }
            ret.push(cand_tuple);
        }
        Ok(ret)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::cast_precision_loss, reason = "test assertions and test-only numeric casts")]
mod tests {
    use super::*;
    use crate::DbInstance;

    #[test]
    fn adaptive_config_default_threshold() {
        let config = AdaptiveSearchConfig::default();
        assert_eq!(config.exact_threshold, DEFAULT_EXACT_THRESHOLD);
        assert!(config.should_use_exact(100));
        assert!(config.should_use_exact(DEFAULT_EXACT_THRESHOLD));
        assert!(!config.should_use_exact(DEFAULT_EXACT_THRESHOLD + 1));
    }

    #[test]
    fn adaptive_config_custom_threshold() {
        let config = AdaptiveSearchConfig::with_threshold(50);
        assert!(config.should_use_exact(50));
        assert!(!config.should_use_exact(51));
    }

    #[test]
    fn search_strategy_at_boundary() {
        let config = AdaptiveSearchConfig::with_threshold(10);
        let below = if config.should_use_exact(10) { SearchStrategy::Exact } else { SearchStrategy::Approximate };
        assert_eq!(below, SearchStrategy::Exact);
        let above = if config.should_use_exact(11) { SearchStrategy::Exact } else { SearchStrategy::Approximate };
        assert_eq!(above, SearchStrategy::Approximate);
    }

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

    /// The fixed oracle finds the exact match — the derived version, per
    /// the note on `hnsw_exact_knn` above, returned zero rows for every
    /// non-empty index.
    #[test]
    fn exact_knn_finds_the_true_nearest_neighbour() {
        let db = setup_db();
        for i in 0..10 {
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        let tx = db.transact().unwrap();
        let base = tx.get_relation("vectors", false).unwrap();
        let (idx_handle, manifest) = base.hnsw_indices.get("idx").unwrap().clone();
        let mut vec_cache = super::VectorCache::new(manifest.distance, 100);
        let q = Vector::F32(ndarray::Array1::from_vec(vec![5.0, 5.0, 5.0, 5.0]));
        let mut stack = vec![];
        let rows = tx
            .hnsw_exact_knn(&q, 3, &base, &idx_handle, &mut vec_cache, &None, &mut stack, true)
            .unwrap();
        assert_eq!(rows.len(), 3, "exact search must return k=3 rows on a 10-vector index");
        let ids: Vec<i64> = rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(ids.contains(&5), "id=5 (the exact match) must be among the top 3, got {ids:?}");
    }

    #[test]
    fn exact_knn_on_empty_index_returns_nothing() {
        let db = setup_db();
        let tx = db.transact().unwrap();
        let base = tx.get_relation("vectors", false).unwrap();
        let (idx_handle, manifest) = base.hnsw_indices.get("idx").unwrap().clone();
        let mut vec_cache = super::VectorCache::new(manifest.distance, 100);
        let q = Vector::F32(ndarray::Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0]));
        let mut stack = vec![];
        let rows = tx
            .hnsw_exact_knn(&q, 5, &base, &idx_handle, &mut vec_cache, &None, &mut stack, false)
            .unwrap();
        assert!(rows.is_empty());
    }
}
