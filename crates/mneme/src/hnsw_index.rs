//! In-memory HNSW vector index backed by `hnsw_rs`.
//!
//! At our scale (<100K vectors, 384 dims, ~160MB), the entire index fits in RAM.
//! Persistence is handled via `hnsw_rs` built-in `file_dump()` / `HnswIo::load_hnsw()`.
//!
//! This module is feature-gated behind `hnsw_rs`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use hnsw_rs::anndists::dist::distances::DistCosine;
use hnsw_rs::hnsw::{Hnsw, Neighbour};
use hnsw_rs::hnswio::HnswIo;
use snafu::OptionExt;
use tracing::instrument;

use crate::error::HnswIndexSnafu;

/// Result of a nearest-neighbour search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// External data ID assigned at insertion time.
    pub data_id: usize,
    /// Distance to the query vector (cosine distance: lower = more similar).
    pub distance: f32,
}

/// Configuration for the HNSW index.
#[derive(Debug, Clone)]
pub struct HnswConfig {
    /// Embedding dimension (e.g., 384 for `MiniLM`).
    pub dim: usize,
    /// Max number of neighbours per node per layer.
    pub max_nb_connection: usize,
    /// Construction-time search width (higher = slower build, better recall).
    pub ef_construction: usize,
    /// Maximum number of layers.
    pub max_layer: usize,
    /// Expected number of elements (hint for pre-allocation).
    pub max_elements: usize,
    /// Directory for dump/load persistence.
    pub persist_dir: Option<PathBuf>,
    /// Base filename for dump files (without extension).
    pub persist_basename: String,
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            dim: 384,
            max_nb_connection: 16,
            ef_construction: 200,
            max_layer: 16,
            max_elements: 100_000,
            persist_dir: None,
            persist_basename: "mneme_hnsw".to_owned(),
        }
    }
}

/// Thread-safe wrapper around `hnsw_rs::Hnsw<f32, DistCosine>`.
///
/// Provides:
/// - In-memory insert and kNN search (sub-millisecond queries)
/// - Persistence via dump/load to disk
/// - Arc-wrapped for shared concurrent access
///
/// # Persistence and the `'static` transmute
///
/// `hnsw_rs::HnswIo::load_hnsw` returns a `Hnsw<'_, …>` that borrows the
/// loader. We extend that lifetime to `'static` via `mem::transmute` so the
/// index can be stored in a `'static`-bounded field.
///
/// **Safety invariant:** This is sound because we do **not** use mmap. When
/// mmap is disabled (our default), `load_hnsw` copies all point data into
/// heap-owned `Vec`s inside the `Hnsw` struct. The loader is not accessed
/// after `load_hnsw` returns; it is retained in `_loader` only to satisfy
/// the type system's lifetime requirement at construction time.
///
/// Drop order (fields drop in declaration order): `_loader` drops before
/// `inner`. Because the `Hnsw` owns its data and never dereferences the
/// loader at runtime, this ordering is safe regardless of which field drops
/// first.
///
/// `#[repr(C)]` pins the field layout so static offset assertions below
/// remain accurate if the struct is ever modified.
#[repr(C)]
pub struct HnswIndex {
    /// Retained to satisfy the type system; not accessed after construction.
    _loader: RwLock<Option<Box<HnswIo>>>,
    inner: RwLock<Option<Hnsw<'static, f32, DistCosine>>>,
    config: HnswConfig,
}

// Compile-time assertion: `_loader` must precede `inner` in the struct layout.
// If the field order is changed the safety argument above must be re-evaluated.
const _: () = assert!(
    std::mem::offset_of!(HnswIndex, _loader) < std::mem::offset_of!(HnswIndex, inner),
    "_loader must be declared before inner; see safety comment on HnswIndex"
);

impl HnswIndex {
    /// Create a new empty HNSW index.
    #[instrument(skip_all, fields(dim = config.dim, max_conn = config.max_nb_connection))]
    pub fn new(config: HnswConfig) -> Arc<Self> {
        let hnsw = Hnsw::<f32, DistCosine>::new(
            config.max_nb_connection,
            config.max_elements,
            config.max_layer,
            config.ef_construction,
            DistCosine,
        );
        Arc::new(Self {
            _loader: RwLock::new(None),
            inner: RwLock::new(Some(hnsw)),
            config,
        })
    }

    /// Try to load an existing index from disk, or create a new one.
    #[instrument(skip_all, fields(dir = ?config.persist_dir))]
    pub fn open_or_create(config: HnswConfig) -> crate::error::Result<Arc<Self>> {
        if let Some(ref dir) = config.persist_dir {
            let graph_path = dir.join(format!("{}.hnsw.graph", &config.persist_basename));
            let data_path = dir.join(format!("{}.hnsw.data", &config.persist_basename));

            if graph_path.exists() && data_path.exists() {
                tracing::info!("loading existing HNSW index from {:?}", dir);
                let mut loader = Box::new(HnswIo::new(dir, &config.persist_basename));
                let hnsw: Hnsw<'_, f32, DistCosine> =
                    loader.load_hnsw::<f32, DistCosine>().map_err(|e| {
                        HnswIndexSnafu {
                            message: format!("failed to load HNSW index: {e}"),
                        }
                        .build()
                    })?;
                // SAFETY: Extending the `Hnsw` lifetime to `'static` is sound
                // because mmap is not used. `load_hnsw` copies all point data into
                // heap-owned allocations inside `Hnsw`; the loader is not accessed
                // after this call returns. The `_loader` Box is stored in the struct
                // to satisfy the type system: see the safety invariant on `HnswIndex`.
                #[expect(
                    unsafe_code,
                    reason = "lifetime extension: Hnsw owns its data after load (no mmap)"
                )]
                let hnsw: Hnsw<'static, f32, DistCosine> = unsafe { std::mem::transmute(hnsw) };
                return Ok(Arc::new(Self {
                    _loader: RwLock::new(Some(loader)),
                    inner: RwLock::new(Some(hnsw)),
                    config,
                }));
            }
        }

        Ok(Self::new(config))
    }

    /// Insert a vector with an external ID.
    ///
    /// The `data_id` is caller-assigned and returned in search results.
    #[instrument(skip(self, vector), fields(data_id))]
    pub fn insert(&self, vector: &[f32], data_id: usize) {
        let guard = self.inner.read().expect("hnsw lock poisoned");
        if let Some(ref hnsw) = *guard {
            hnsw.insert((vector, data_id));
        }
    }

    /// Insert multiple vectors in parallel.
    #[instrument(skip(self, vectors))]
    pub fn insert_batch(&self, vectors: &[(&Vec<f32>, usize)]) {
        let guard = self.inner.read().expect("hnsw lock poisoned");
        if let Some(ref hnsw) = *guard {
            for (vec, id) in vectors {
                hnsw.insert((vec.as_slice(), *id));
            }
        }
    }

    /// Search for the `k` nearest neighbours to the query vector.
    ///
    /// `ef` controls search width (must be >= `k`). Higher = better recall, slower.
    #[instrument(skip(self, query), fields(k, ef))]
    pub fn search(&self, query: &[f32], k: usize, ef: usize) -> Vec<SearchResult> {
        let guard = self.inner.read().expect("hnsw lock poisoned");
        match *guard {
            Some(ref hnsw) => {
                let neighbours: Vec<Neighbour> = hnsw.search(query, k, ef);
                neighbours
                    .into_iter()
                    .map(|n| SearchResult {
                        data_id: n.d_id,
                        distance: n.distance,
                    })
                    .collect()
            }
            None => Vec::new(),
        }
    }

    /// Dump the index to disk for persistence.
    ///
    /// Writes two files: `{basename}.hnsw.graph` and `{basename}.hnsw.data`.
    #[instrument(skip(self))]
    pub fn dump(&self) -> crate::error::Result<()> {
        let dir = self.config.persist_dir.as_ref().context(HnswIndexSnafu {
            message: "no persist_dir configured for HNSW dump",
        })?;

        std::fs::create_dir_all(dir).map_err(|e| {
            HnswIndexSnafu {
                message: format!("failed to create HNSW dump directory: {e}"),
            }
            .build()
        })?;

        let guard = self.inner.read().expect("hnsw lock poisoned");
        if let Some(ref hnsw) = *guard {
            use hnsw_rs::api::AnnT;
            hnsw.file_dump(dir, &self.config.persist_basename)
                .map_err(|e| {
                    HnswIndexSnafu {
                        message: format!("HNSW dump failed: {e}"),
                    }
                    .build()
                })?;
        }
        Ok(())
    }

    /// Returns the number of points currently in the index.
    pub fn len(&self) -> usize {
        let guard = self.inner.read().expect("hnsw lock poisoned");
        match *guard {
            Some(ref hnsw) => hnsw.get_nb_point(),
            None => 0,
        }
    }

    /// Returns true if the index contains no points.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the configured dimension.
    pub fn dim(&self) -> usize {
        self.config.dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(dim: usize) -> HnswConfig {
        HnswConfig {
            dim,
            max_nb_connection: 16,
            ef_construction: 200,
            max_layer: 16,
            max_elements: 1000,
            persist_dir: None,
            persist_basename: "test_hnsw".to_owned(),
        }
    }

    #[test]
    fn insert_and_search_roundtrip() {
        let index = HnswIndex::new(make_config(4));

        let v1 = vec![1.0, 0.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0, 0.0];
        let v3 = vec![0.9, 0.1, 0.0, 0.0];

        index.insert(&v1, 0);
        index.insert(&v2, 1);
        index.insert(&v3, 2);

        // Query close to v1: should return v1 (id=0) and v3 (id=2) as nearest
        let query = vec![0.95, 0.05, 0.0, 0.0];
        let results = index.search(&query, 2, 16);

        assert_eq!(results.len(), 2);
        let ids: Vec<usize> = results.iter().map(|r| r.data_id).collect();
        assert!(ids.contains(&0) || ids.contains(&2));
    }

    #[test]
    fn search_empty_index() {
        let index = HnswIndex::new(make_config(4));
        let results = index.search(&[0.5, 0.5, 0.0, 0.0], 5, 16);
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_k() {
        let index = HnswIndex::new(make_config(4));

        for i in 0..20_usize {
            #[expect(
                clippy::cast_precision_loss,
                reason = "test data — small indices fit in f32"
            )]
            let v = vec![i as f32, 0.0, 0.0, 0.0];
            index.insert(&v, i);
        }

        let results = index.search(&[10.0, 0.0, 0.0, 0.0], 5, 20);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn dump_and_load_roundtrip() {
        let dir = tempfile::TempDir::new().expect("temp dir creation succeeds");
        let config = HnswConfig {
            dim: 4,
            persist_dir: Some(dir.path().to_path_buf()),
            ..make_config(4)
        };

        // Insert and dump
        {
            let index = HnswIndex::new(config.clone());
            index.insert(&[1.0, 0.0, 0.0, 0.0], 0);
            index.insert(&[0.0, 1.0, 0.0, 0.0], 1);
            index.insert(&[0.0, 0.0, 1.0, 0.0], 2);
            assert_eq!(index.len(), 3);
            index.dump().expect("dump should succeed");
        }

        // Reload and verify
        {
            let index = HnswIndex::open_or_create(config).expect("reload should succeed");
            assert_eq!(index.len(), 3);

            let results = index.search(&[1.0, 0.0, 0.0, 0.0], 1, 16);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].data_id, 0);
        }
    }

    #[test]
    fn len_and_is_empty() {
        let index = HnswIndex::new(make_config(4));
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);

        index.insert(&[1.0, 0.0, 0.0, 0.0], 0);
        assert!(!index.is_empty());
        assert_eq!(index.len(), 1);
    }

    /// Verify sub-millisecond search at small scale.
    #[test]
    fn search_performance() {
        let index = HnswIndex::new(make_config(384));

        // Insert 1000 random-ish vectors
        for i in 0..1000 {
            let mut v = vec![0.0f32; 384];
            v[i % 384] = 1.0;
            v[(i * 7) % 384] += 0.5;
            index.insert(&v, i);
        }

        let query = vec![0.5f32; 384];
        let start = std::time::Instant::now();
        let _results = index.search(&query, 10, 32);
        let elapsed = start.elapsed();

        // Should complete well under 10ms at this scale
        assert!(
            elapsed.as_millis() < 10,
            "search took {elapsed:?}, expected sub-millisecond"
        );
    }
}
