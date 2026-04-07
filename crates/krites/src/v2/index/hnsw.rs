//! HNSW (Hierarchical Navigable Small World) vector index.
//!
//! Approximate nearest neighbor search for embedding vectors. Used by
//! the `~embeddings:semantic_idx{...}` query pattern in episteme's
//! recall pipeline.
//!
//! Implementation: brute-force exact search for correctness. Will be
//! replaced with a proper HNSW graph when the index grows beyond ~10K
//! vectors. The Index trait boundary insulates callers from this.
//!
//! WHY brute-force first: HNSW is complex (~800 LOC for a proper
//! implementation with layered graph, ef_construction, neighbor selection).
//! Starting with exact search lets us validate the query integration
//! end-to-end before optimizing the index structure.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::v2::error::{self, Result};
use crate::v2::value::{Value, VectorValue};

use super::Index;

// ---------------------------------------------------------------------------
// HNSW configuration
// ---------------------------------------------------------------------------

/// Distance metric for vector comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DistanceMetric {
    /// Cosine similarity (1 - cos_sim as distance).
    Cosine,
    /// Euclidean (L2) distance.
    Euclidean,
}

/// Configuration for the HNSW index.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HnswConfig {
    /// Vector dimensionality.
    pub dim: usize,
    /// Distance metric.
    pub metric: DistanceMetric,
    /// ef parameter for search (higher = more accurate, slower).
    pub ef_search: usize,
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            dim: 384, // WHY: common embedding dimension (bge-small, all-MiniLM)
            metric: DistanceMetric::Cosine,
            ef_search: 200,
        }
    }
}

// ---------------------------------------------------------------------------
// HNSW index
// ---------------------------------------------------------------------------

/// Vector index using exact brute-force search.
///
/// Stores vectors in memory, computes distances on every search.
/// Suitable for <10K vectors. For larger indexes, replace internals
/// with a proper HNSW graph while keeping the same Index trait.
pub struct HnswIndex {
    name: String,
    config: HnswConfig,
    vectors: Arc<RwLock<Vec<(Vec<u8>, Vec<f32>)>>>,
}

impl HnswIndex {
    /// Create a new HNSW index.
    #[must_use]
    pub fn new(name: impl Into<String>, config: HnswConfig) -> Self {
        Self {
            name: name.into(),
            config,
            vectors: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Index for HnswIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn upsert(&self, id: &[u8], value: &Value) -> Result<()> {
        let vec = extract_f32_vector(value, self.config.dim)?;
        let mut vectors = self.vectors.write().map_err(|_| {
            error::IndexSnafu {
                index_name: self.name.clone(),
                message: "lock poisoned",
            }
            .build()
        })?;

        // WHY: upsert semantics — replace if ID exists, else append.
        if let Some(entry) = vectors.iter_mut().find(|(eid, _)| eid == id) {
            entry.1 = vec;
        } else {
            vectors.push((id.to_vec(), vec));
        }
        Ok(())
    }

    fn remove(&self, id: &[u8]) -> Result<()> {
        let mut vectors = self.vectors.write().map_err(|_| {
            error::IndexSnafu {
                index_name: self.name.clone(),
                message: "lock poisoned",
            }
            .build()
        })?;
        vectors.retain(|(eid, _)| eid != id);
        Ok(())
    }

    fn search(
        &self,
        query: &Value,
        k: usize,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Vec<(Vec<u8>, f64)>> {
        let query_vec = extract_f32_vector(query, self.config.dim)?;
        let vectors = self.vectors.read().map_err(|_| {
            error::IndexSnafu {
                index_name: self.name.clone(),
                message: "lock poisoned",
            }
            .build()
        })?;

        let mut scored: Vec<(Vec<u8>, f64)> = vectors
            .iter()
            .map(|(id, vec)| {
                let dist = match self.config.metric {
                    DistanceMetric::Cosine => cosine_distance(&query_vec, vec),
                    DistanceMetric::Euclidean => euclidean_distance(&query_vec, vec),
                };
                (id.clone(), dist)
            })
            .collect();

        // WHY: sort by distance ascending (lower = more similar).
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        Ok(scored)
    }

    fn len(&self) -> usize {
        self.vectors
            .read()
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Distance functions
// ---------------------------------------------------------------------------

fn cosine_distance(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let x = f64::from(*x);
        let y = f64::from(*y);
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        1.0 // WHY: zero vectors are maximally distant.
    } else {
        1.0 - (dot / denom) // Cosine distance = 1 - cosine_similarity
    }
}

fn euclidean_distance(a: &[f32], b: &[f32]) -> f64 {
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = f64::from(*x) - f64::from(*y);
            d * d
        })
        .sum();
    sum.sqrt()
}

/// Extract f32 vector from a Value, validating dimensionality.
fn extract_f32_vector(value: &Value, expected_dim: usize) -> Result<Vec<f32>> {
    match value {
        Value::Vector(VectorValue::F32(data)) => {
            if expected_dim > 0 && data.len() != expected_dim {
                return Err(error::IndexSnafu {
                    index_name: "hnsw",
                    message: format!(
                        "dimension mismatch: expected {expected_dim}, got {}",
                        data.len()
                    ),
                }
                .build());
            }
            Ok(data.to_vec())
        }
        Value::Vector(VectorValue::F64(data)) => {
            if expected_dim > 0 && data.len() != expected_dim {
                return Err(error::IndexSnafu {
                    index_name: "hnsw",
                    message: format!(
                        "dimension mismatch: expected {expected_dim}, got {}",
                        data.len()
                    ),
                }
                .build());
            }
            Ok(data.iter().map(|&x| f32::from(x)).collect())
        }
        _ => Err(error::IndexSnafu {
            index_name: "hnsw",
            message: format!("expected vector, got {}", value.type_name()),
        }
        .build()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_vec(values: &[f32]) -> Value {
        Value::Vector(VectorValue::F32(Arc::from(values)))
    }

    #[test]
    fn insert_and_search() {
        let idx = HnswIndex::new("test", HnswConfig { dim: 3, ..HnswConfig::default() });

        idx.upsert(b"v1", &make_vec(&[1.0, 0.0, 0.0])).unwrap();
        idx.upsert(b"v2", &make_vec(&[0.0, 1.0, 0.0])).unwrap();
        idx.upsert(b"v3", &make_vec(&[0.9, 0.1, 0.0])).unwrap();

        let results = idx.search(&make_vec(&[1.0, 0.0, 0.0]), 2, &BTreeMap::new()).unwrap();
        assert_eq!(results.len(), 2);
        // v1 should be closest (exact match), v3 second.
        assert_eq!(results[0].0, b"v1");
        assert_eq!(results[1].0, b"v3");
    }

    #[test]
    fn upsert_replaces() {
        let idx = HnswIndex::new("test", HnswConfig { dim: 2, ..HnswConfig::default() });

        idx.upsert(b"v1", &make_vec(&[1.0, 0.0])).unwrap();
        idx.upsert(b"v1", &make_vec(&[0.0, 1.0])).unwrap();

        assert_eq!(idx.len(), 1);
        let results = idx.search(&make_vec(&[0.0, 1.0]), 1, &BTreeMap::new()).unwrap();
        assert_eq!(results[0].0, b"v1");
        assert!(results[0].1 < 0.01); // nearly zero distance after update
    }

    #[test]
    fn remove() {
        let idx = HnswIndex::new("test", HnswConfig { dim: 2, ..HnswConfig::default() });

        idx.upsert(b"v1", &make_vec(&[1.0, 0.0])).unwrap();
        idx.upsert(b"v2", &make_vec(&[0.0, 1.0])).unwrap();
        assert_eq!(idx.len(), 2);

        idx.remove(b"v1").unwrap();
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn dimension_mismatch() {
        let idx = HnswIndex::new("test", HnswConfig { dim: 3, ..HnswConfig::default() });
        let result = idx.upsert(b"v1", &make_vec(&[1.0, 0.0]));
        assert!(result.is_err());
    }

    #[test]
    fn cosine_distance_identical() {
        let dist = cosine_distance(&[1.0, 0.0], &[1.0, 0.0]);
        assert!(dist.abs() < 1e-10);
    }

    #[test]
    fn cosine_distance_orthogonal() {
        let dist = cosine_distance(&[1.0, 0.0], &[0.0, 1.0]);
        assert!((dist - 1.0).abs() < 1e-10);
    }

    #[test]
    fn euclidean_distance_works() {
        let dist = euclidean_distance(&[0.0, 0.0], &[3.0, 4.0]);
        assert!((dist - 5.0).abs() < 1e-10);
    }
}
