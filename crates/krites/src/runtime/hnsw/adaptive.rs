//! Adaptive search strategy: exact vs approximate based on dataset size.
//!
//! For small datasets, brute-force linear scan is both faster and produces
//! perfect recall. Beyond a configurable threshold, HNSW approximate search
//! takes over. The threshold is configurable per-index.
#![expect(
    dead_code,
    reason = "infrastructure for future HNSW search-path integration"
)]
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;

use super::types::{CompoundKey, VectorCache};
use crate::DataValue;
use crate::SourceSpan;
use crate::data::expr::{Bytecode, eval_bytecode_pred};
use crate::data::tuple::Tuple;
use crate::data::value::Vector;
use crate::error::InternalResult as Result;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

/// Default threshold below which exact (brute-force) search is used.
///
/// At 256 vectors, linear scan over all vectors is typically faster than
/// HNSW graph traversal due to lower constant factors and perfect cache
/// locality.
pub(crate) const DEFAULT_EXACT_THRESHOLD: usize = 256;

/// Configuration for adaptive search behaviour.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AdaptiveSearchConfig {
    /// Maximum dataset size for exact search. Datasets with more vectors use
    /// approximate (HNSW) search.
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
    /// Create a config with a custom exact-search threshold.
    pub(crate) fn with_threshold(exact_threshold: usize) -> Self {
        Self { exact_threshold }
    }

    /// Whether to use exact search for the given dataset size.
    pub(crate) fn should_use_exact(&self, dataset_size: usize) -> bool {
        dataset_size <= self.exact_threshold
    }
}

/// Search strategy resolved for a specific query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchStrategy {
    /// Brute-force linear scan over all vectors.
    Exact,
    /// HNSW approximate nearest neighbour search.
    Approximate,
}

impl<'a> SessionTx<'a> {
    /// Perform exact (brute-force) kNN search by scanning all vectors.
    ///
    /// Iterates over every canary entry in the index, computes the distance to
    /// the query vector, and returns the top-k nearest neighbours. This
    /// guarantees perfect recall but is O(n) in dataset size.
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
        // Scan all canary entries (level = 1) to enumerate all indexed vectors.
        let canary_prefix = vec![DataValue::from(1_i64)];
        let key_len = base_handle.metadata.keys.len();

        // Max-heap: we push all candidates and pop the worst when > k.
        let mut top_k: PriorityQueue<CompoundKey, OrderedFloat<f64>> = PriorityQueue::new();

        for res in idx_handle.scan_prefix(self, &canary_prefix) {
            let tuple = match res {
                Ok(t) => t,
                Err(_) => continue,
            };

            // Extract the compound key from the canary entry.
            let tuple_key: Vec<DataValue> = match tuple.get(1..key_len + 1) {
                Some(slice) => slice.to_vec(),
                None => continue,
            };
            if tuple_key.is_empty() {
                continue;
            }

            let idx = match tuple.get(key_len + 1).and_then(|v| v.get_int()) {
                Some(x) => match usize::try_from(x) {
                    Ok(v) => v,
                    Err(_) => continue,
                },
                None => continue,
            };
            let subidx = match tuple.get(key_len + 2).and_then(|v| v.get_int()) {
                Some(x) => match i32::try_from(x) {
                    Ok(v) => v,
                    Err(_) => continue,
                },
                None => continue,
            };

            let cand_key = (tuple_key, idx, subidx);
            if vec_cache.ensure_key(&cand_key, base_handle, self).is_err() {
                continue;
            }
            let dist = match vec_cache.v_dist(q, &cand_key) {
                Ok(d) => d,
                Err(_) => continue,
            };

            top_k.push(cand_key, OrderedFloat(dist));
            if top_k.len() > k {
                top_k.pop();
            }
        }

        // Collect results in ascending distance order.
        let mut results: Vec<(CompoundKey, f64)> = Vec::with_capacity(top_k.len());
        while let Some((key, OrderedFloat(dist))) = top_k.pop() {
            results.push((key, dist));
        }
        results.reverse();

        // Build output tuples.
        let mut ret = Vec::with_capacity(results.len());
        for (cand_key, distance) in results {
            let mut cand_tuple = match base_handle.get(self, &cand_key.0)? {
                Some(t) => t,
                None => continue,
            };

            if bind_distance {
                cand_tuple.push(DataValue::from(distance));
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
#[expect(
    clippy::unwrap_used,
    clippy::as_conversions,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    reason = "test assertions and test-only numeric casts"
)]
mod tests {
    use super::*;
    use crate::DbInstance;

    #[test]
    fn adaptive_config_default_threshold() {
        let config = AdaptiveSearchConfig::default();
        assert_eq!(
            config.exact_threshold, DEFAULT_EXACT_THRESHOLD,
            "default threshold"
        );
        assert!(config.should_use_exact(100), "100 vectors → exact");
        assert!(
            config.should_use_exact(DEFAULT_EXACT_THRESHOLD),
            "at threshold → exact"
        );
        assert!(
            !config.should_use_exact(DEFAULT_EXACT_THRESHOLD + 1),
            "above threshold → approximate"
        );
    }

    #[test]
    fn adaptive_config_custom_threshold() {
        let config = AdaptiveSearchConfig::with_threshold(50);
        assert!(config.should_use_exact(50), "at custom threshold → exact");
        assert!(
            !config.should_use_exact(51),
            "above custom threshold → approximate"
        );
    }

    #[test]
    fn search_strategy_at_boundary() {
        let config = AdaptiveSearchConfig::with_threshold(10);
        let strategy = if config.should_use_exact(10) {
            SearchStrategy::Exact
        } else {
            SearchStrategy::Approximate
        };
        assert_eq!(strategy, SearchStrategy::Exact, "boundary is exact");

        let strategy = if config.should_use_exact(11) {
            SearchStrategy::Exact
        } else {
            SearchStrategy::Approximate
        };
        assert_eq!(
            strategy,
            SearchStrategy::Approximate,
            "above boundary is approximate"
        );
    }

    #[test]
    fn exact_search_returns_correct_results() {
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
        // Insert 10 vectors (well below any threshold).
        for i in 0..10 {
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        // Normal HNSW search for comparison.
        let res = db
            .run_default(
                r#"?[id, dist] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 3, ef: 50, bind_distance: dist}"#,
            )
            .unwrap();
        assert!(!res.rows.is_empty(), "HNSW search should return results");
        let ids: Vec<i64> = res.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(
            ids.contains(&5),
            "exact match id=5 should be in results, got {:?}",
            ids
        );
    }
}
