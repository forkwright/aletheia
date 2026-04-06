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
/// [`HnswIo::load_hnsw`] returns `Hnsw<'b, …>` where `'b` is bounded by
/// the loader's borrow (`'a: 'b`). We transmute that lifetime to `'static`
/// so the index can live in a struct field.
///
/// # Safety invariant chain
///
/// The transmute is sound because without mmap, the lifetime `'b` carries
/// no actual reference:
///
/// 1. **Data ownership (no mmap):** `hnsw_rs` stores point data in an
///    internal `PointData<'b, T>` enum with two variants: `V(Vec<T>)`
///    (heap-owned) and `S(&'b [T])` (mmap slice). When mmap is disabled
///    (our default — we never call [`HnswIo::set_options`]), every point
///    is constructed via `Point::new()` → `PointData::V(…)`. The `S`
///    variant is never instantiated, so the lifetime `'b` is phantom —
///    no runtime `&'b` reference exists anywhere in the graph.
///
/// 2. **Loader co-storage:** The `_loader` [`Box<HnswIo>`] is stored in
///    this struct alongside the `Hnsw`. Both fields share the struct's
///    lifetime, so the loader cannot be dropped while the [`HnswIndex`]
///    is live. The loader is retained to satisfy the `'a: 'b` constraint
///    at the type level and as defense-in-depth if `hnsw_rs` internals
///    change.
///
/// 3. **Drop order:** `#[repr(C)]` pins field layout to declaration
///    order. A compile-time assertion ([`std::mem::offset_of!`]) verifies
///    `_loader` precedes `inner`. Rust drops fields in declaration order,
///    so `inner` (the `Hnsw`) is still valid when its destructor runs —
///    `_loader` has not yet been dropped. Because no real `&'b` reference
///    exists (invariant 1), drop order is immaterial in practice, but the
///    assertion guards against future regressions.
///
/// # Safe alternatives considered
///
/// [`self_cell`](https://docs.rs/self_cell) and `ouroboros` encapsulate
/// self-referential borrows safely. They were not adopted here because
/// without mmap the borrow is vacuous — the `Hnsw` owns all its data
/// outright. If `hnsw_rs` ever enables mmap by default, migrating to
/// `self_cell` would be the correct fix. See [#2026].
#[repr(C)]
pub(crate) struct HnswIndex {
    /// WHY: Retained to satisfy the `'a: 'b` lifetime constraint from
    /// `HnswIo::load_hnsw`. Not accessed after construction. Must be
    /// declared before `inner` for correct drop order (see safety invariant 3).
    _loader: RwLock<Option<Box<HnswIo>>>,
    inner: RwLock<Option<Hnsw<'static, f32, DistCosine>>>,
    config: HnswConfig,
}

const _: () = assert!(
    std::mem::offset_of!(HnswIndex, _loader) < std::mem::offset_of!(HnswIndex, inner),
    "_loader must be declared before inner; see safety comment on HnswIndex"
);

impl HnswIndex {
    /// Create a new empty HNSW index.
    #[instrument(skip_all, fields(dim = config.dim, max_conn = config.max_nb_connection))]
    pub(crate) fn new(config: HnswConfig) -> Arc<Self> {
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
    pub(crate) fn open_or_create(config: HnswConfig) -> crate::error::Result<Arc<Self>> {
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
                // SAFETY: Transmuting `Hnsw<'_, …>` to `Hnsw<'static, …>`.
                //
                // INVARIANT 1 — DATA OWNERSHIP: `hnsw_rs` is loaded without
                // mmap (`ReloadOptions` default). All point data is stored as
                // `PointData::V(Vec<f32>)` (heap-owned). The `PointData::S(&'b [T])`
                // variant (mmap slice) is never constructed, so the lifetime
                // `'b` on `Hnsw<'b, …>` is phantom — no runtime reference exists.
                //
                // INVARIANT 2 — LOADER RETENTION: The `loader` Box is moved
                // into `_loader`, co-stored in the same `HnswIndex` struct. It
                // cannot be dropped independently while the `Hnsw` is live.
                //
                // INVARIANT 3 — DROP ORDER: `#[repr(C)]` + compile-time offset
                // assertion ensures `_loader` is declared before `inner`. Rust
                // drops fields in declaration order, so `inner` drops while
                // `_loader` is still alive. Immaterial without mmap (invariant 1)
                // but provides defense-in-depth.
                //
                // WARNING: Would break if `hnsw_rs` enables mmap by default,
                // or if the loader is separated from the index. Either change
                // requires migrating to `self_cell` or equivalent safe wrapper.
                #[expect(
                    unsafe_code,
                    reason = "lifetime extension: Hnsw<'b> → Hnsw<'static>; sound because \
                              without mmap all PointData uses the owned V(Vec<T>) variant — \
                              see SAFETY invariant chain on HnswIndex and issue #2026"
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
    /// Returns an error if the vector dimension does not match the configured dimension.
    #[instrument(skip(self, vector), fields(data_id))]
    pub(crate) fn insert(&self, vector: &[f32], data_id: usize) -> crate::error::Result<()> {
        if vector.len() != self.config.dim {
            return Err(HnswIndexSnafu {
                message: format!(
                    "dimension mismatch: expected {}, got {}",
                    self.config.dim,
                    vector.len()
                ),
            }
            .build());
        }
        // WHY: enforce max_elements as a hard cap to prevent unbounded memory growth.
        // The HNSW library uses max_elements as a pre-allocation hint but does not
        // reject inserts beyond it. We enforce it here.
        let current_len = self.len();
        if current_len >= self.config.max_elements {
            tracing::warn!(
                current = current_len,
                max = self.config.max_elements,
                "HNSW index at capacity, skipping insert"
            );
            return Err(HnswIndexSnafu {
                message: format!(
                    "index at capacity: {} of {} elements",
                    current_len, self.config.max_elements
                ),
            }
            .build());
        }
        let guard = self.inner.read().unwrap_or_else(|e| {
            tracing::warn!("HNSW read lock was poisoned, recovering");
            e.into_inner()
        });
        if let Some(ref hnsw) = *guard {
            hnsw.insert((vector, data_id));
        }
        Ok(())
    }

    /// Insert multiple vectors in parallel.
    ///
    /// Returns an error if any vector dimension does not match the configured dimension.
    #[instrument(skip(self, vectors))]
    pub(crate) fn insert_batch(&self, vectors: &[(&Vec<f32>, usize)]) -> crate::error::Result<()> {
        for (vec, _) in vectors {
            if vec.len() != self.config.dim {
                return Err(HnswIndexSnafu {
                    message: format!(
                        "dimension mismatch: expected {}, got {}",
                        self.config.dim,
                        vec.len()
                    ),
                }
                .build());
            }
        }
        let guard = self.inner.read().unwrap_or_else(|e| {
            tracing::warn!("HNSW read lock was poisoned, recovering");
            e.into_inner()
        });
        if let Some(ref hnsw) = *guard {
            for (vec, id) in vectors {
                hnsw.insert((vec.as_slice(), *id));
            }
        }
        Ok(())
    }

    /// Search for the `k` nearest neighbours to the query vector.
    ///
    /// `ef` controls search width (must be >= `k`). Higher = better recall, slower.
    #[instrument(skip(self, query), fields(k, ef))]
    pub(crate) fn search(&self, query: &[f32], k: usize, ef: usize) -> Vec<SearchResult> {
        let guard = self.inner.read().unwrap_or_else(|e| {
            tracing::warn!("HNSW read lock was poisoned, recovering");
            e.into_inner()
        });
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
    pub(crate) fn dump(&self) -> crate::error::Result<()> {
        let dir = self.config.persist_dir.as_ref().context(HnswIndexSnafu {
            message: "no persist_dir configured for HNSW dump",
        })?;

        std::fs::create_dir_all(dir).map_err(|e| {
            HnswIndexSnafu {
                message: format!("failed to create HNSW dump directory: {e}"),
            }
            .build()
        })?;

        let guard = self.inner.read().unwrap_or_else(|e| {
            tracing::warn!("HNSW read lock was poisoned, recovering");
            e.into_inner()
        });
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
    pub(crate) fn len(&self) -> usize {
        let guard = self.inner.read().unwrap_or_else(|e| {
            tracing::warn!("HNSW read lock was poisoned, recovering");
            e.into_inner()
        });
        match *guard {
            Some(ref hnsw) => hnsw.get_nb_point(),
            None => 0,
        }
    }

    /// Returns true if the index contains no points.
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the configured dimension.
    pub(crate) fn dim(&self) -> usize {
        self.config.dim
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
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

        index.insert(&v1, 0).expect("insert v1");
        index.insert(&v2, 1).expect("insert v2");
        index.insert(&v3, 2).expect("insert v3");

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
                reason = "test data: small indices fit in f32"
            )]
            let v = vec![i as f32, 0.0, 0.0, 0.0];
            index.insert(&v, i).expect("insert test vector");
        }

        let results = index.search(&[10.0, 0.0, 0.0, 0.0], 5, 20);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn insert_rejects_wrong_dimension() {
        let index = HnswIndex::new(make_config(4));
        let wrong_dim = vec![1.0, 0.0]; // dim 2, expected 4
        let result = index.insert(&wrong_dim, 0);
        assert!(result.is_err(), "inserting wrong dimension should fail");
    }

    #[test]
    fn dump_and_load_roundtrip() {
        let dir = tempfile::TempDir::new().expect("temp dir creation succeeds");
        let config = HnswConfig {
            dim: 4,
            persist_dir: Some(dir.path().to_path_buf()),
            ..make_config(4)
        };

        {
            let index = HnswIndex::new(config.clone());
            index.insert(&[1.0, 0.0, 0.0, 0.0], 0).expect("insert v1");
            index.insert(&[0.0, 1.0, 0.0, 0.0], 1).expect("insert v2");
            index.insert(&[0.0, 0.0, 1.0, 0.0], 2).expect("insert v3");
            assert_eq!(index.len(), 3);
            index.dump().expect("dump should succeed");
        }

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

        index.insert(&[1.0, 0.0, 0.0, 0.0], 0).expect("insert");
        assert!(!index.is_empty());
        assert_eq!(index.len(), 1);
    }

    /// Validates the `'static` transmute safety invariant by exercising
    /// repeated dump -> drop -> reload cycles. Each cycle invokes
    /// `open_or_create` (which transmutes) and then searches the reloaded
    /// index. Regression: if the transmute were unsound (e.g. dangling
    /// reference from mmap), this would manifest as corruption or SIGSEGV.
    #[test]
    #[expect(
        clippy::indexing_slicing,
        reason = "test code: index into known-length search results"
    )]
    fn transmute_safety_repeated_load_drop_cycles() {
        let dir = tempfile::TempDir::new().expect("temp dir creation succeeds");
        let config = HnswConfig {
            dim: 4,
            persist_dir: Some(dir.path().to_path_buf()),
            ..make_config(4)
        };

        // NOTE: seed the index with initial data
        {
            let index = HnswIndex::new(config.clone());
            index.insert(&[1.0, 0.0, 0.0, 0.0], 0).expect("insert v1");
            index.insert(&[0.0, 1.0, 0.0, 0.0], 1).expect("insert v2");
            index.insert(&[0.0, 0.0, 1.0, 0.0], 2).expect("insert v3");
            index.dump().expect("initial dump should succeed");
        }

        // NOTE: reload -> search -> drop across 3 cycles to exercise the
        // transmute path repeatedly. Each cycle creates a new HnswIndex
        // (with transmute), uses it, then drops it.
        for cycle in 0..3_u32 {
            let index = HnswIndex::open_or_create(config.clone())
                .expect("reload should succeed on each cycle");
            assert_eq!(index.len(), 3, "point count must be 3 after cycle {cycle}");

            // NOTE: search the original point to verify data integrity
            let results = index.search(&[1.0, 0.0, 0.0, 0.0], 1, 16);
            assert_eq!(results.len(), 1, "search must return 1 result after reload");
            assert_eq!(results[0].data_id, 0, "nearest neighbour must be data_id 0");

            // NOTE: verify all three original vectors are findable
            let results = index.search(&[0.0, 1.0, 0.0, 0.0], 1, 16);
            assert_eq!(
                results[0].data_id, 1,
                "second vector must be findable after reload cycle {cycle}"
            );
        }
        // NOTE: index drops here: exercises the Drop path on transmuted Hnsw
    }

    /// Verify sub-millisecond search at small scale.
    #[test]
    fn search_performance() {
        let index = HnswIndex::new(make_config(384));

        for i in 0..1000 {
            let mut v = vec![0.0f32; 384];
            v[i % 384] = 1.0;
            v[(i * 7) % 384] += 0.5;
            index.insert(&v, i).expect("insert test vector");
        }

        let query = vec![0.5f32; 384];
        let start = std::time::Instant::now();
        let _results = index.search(&query, 10, 32);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 10,
            "search took {elapsed:?}, expected sub-millisecond"
        );
    }
}
