//! Side-query memory relevance selector.
//!
//! Uses a lighter model to pre-filter memory entries from a manifest before
//! the full 6-factor recall scoring runs. Implements `already_surfaced` tracking
//! to avoid re-selecting previously injected memories, and an LRU cache to skip
//! redundant side-queries for unchanged (query, manifest) pairs.
//!
//! Adapted from CC's `findRelevantMemories.ts` pattern: side-query to a cheaper
//! model selects top-N from a name+description manifest. `alreadySurfaced` set
//! prevents re-selecting files shown in prior turns.

use std::collections::HashSet;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use snafu::Snafu;
use tracing::{debug, instrument};

use crate::manifest::MemoryManifest;

/// Default maximum entries returned by a single side-query.
const DEFAULT_MAX_RESULTS: usize = 5;

/// Default cache time-to-live in seconds.
const DEFAULT_CACHE_TTL_SECS: u64 = 300;

/// Default maximum cache entries.
const DEFAULT_CACHE_CAPACITY: usize = 64;

/// Errors from side-query operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum SideQueryError {
    /// The ranking model call failed.
    #[snafu(display("side-query ranker failed: {message}"))]
    RankerFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Trait for ranking memory entries via a side-query to a lightweight model.
///
/// Implementations send the formatted manifest and query to an LLM and parse
/// the response into a ranked list of source IDs. The trait is synchronous to
/// match the existing recall pipeline's sync trait pattern
/// ([`EmbeddingProvider`](crate::embedding::EmbeddingProvider),
/// [`VectorSearch`]).
pub trait SideQueryRanker: Send + Sync {
    /// Rank memory entries by relevance to the query.
    ///
    /// # Arguments
    ///
    /// * `query` — The user's query or conversation context.
    /// * `manifest_text` — Formatted manifest from [`MemoryManifest::format`].
    /// * `max_results` — Maximum number of entries to return.
    ///
    /// # Returns
    ///
    /// Source IDs of the most relevant memory entries, in ranked order.
    ///
    /// # Errors
    ///
    /// Returns [`SideQueryError::RankerFailed`] if the model call or response
    /// parsing fails.
    fn rank_memories(
        &self,
        query: &str,
        manifest_text: &str,
        max_results: usize,
    ) -> Result<Vec<String>, SideQueryError>;
}

/// Configuration for the side-query selector.
#[derive(Debug, Clone)]
pub struct SideQueryConfig {
    /// Maximum number of results to return per query.
    pub max_results: usize,
    /// Cache entry time-to-live in seconds.
    pub cache_ttl_secs: u64,
    /// Maximum number of cached entries.
    pub cache_capacity: usize,
    /// Whether side-query pre-filtering is enabled.
    pub enabled: bool,
}

impl Default for SideQueryConfig {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_MAX_RESULTS,
            cache_ttl_secs: DEFAULT_CACHE_TTL_SECS,
            cache_capacity: DEFAULT_CACHE_CAPACITY,
            enabled: true,
        }
    }
}

/// Result from a side-query selection.
#[derive(Debug, Clone)]
pub struct SideQueryResult {
    /// Source IDs selected by the side-query, in relevance order.
    pub selected_ids: Vec<String>,
    /// Whether this result was served from cache.
    pub from_cache: bool,
}

impl fmt::Display for SideQueryResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SideQueryResult({} selected, cached={})",
            self.selected_ids.len(),
            self.from_cache
        )
    }
}

/// A single entry in the relevance cache.
struct CacheEntry {
    selected_ids: Vec<String>,
    created_at: Instant,
}

/// Bounded LRU relevance cache.
///
/// Keys are a combined hash of (query, `manifest_text`). Entries expire
/// after a configurable TTL. Front = LRU, back = MRU.
pub(crate) struct RelevanceCache {
    // NOTE: linear scan is fine for small capacity (default 64).
    entries: Vec<(u64, CacheEntry)>,
    capacity: usize,
    ttl: Duration,
}

impl RelevanceCache {
    /// Create a new cache with the given capacity and TTL.
    pub(crate) fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
            ttl,
        }
    }

    /// Look up a cached result. Returns `None` on miss or expiry.
    pub(crate) fn get(&mut self, key: u64) -> Option<Vec<String>> {
        let now = Instant::now();
        // PERF: linear scan is fine for small capacity (default 64).
        let pos = self.entries.iter().position(|(k, _)| *k == key)?;

        if now.saturating_duration_since(self.entries.get(pos)?.1.created_at) > self.ttl {
            self.entries.remove(pos);
            return None;
        }

        let ids = self.entries.get(pos)?.1.selected_ids.clone();
        
        let pair = self.entries.remove(pos);
        self.entries.push(pair);
        Some(ids)
    }

    /// Insert a result, evicting the LRU entry if at capacity.
    pub(crate) fn insert(&mut self, key: u64, selected_ids: Vec<String>) {
        
        self.entries.retain(|(k, _)| *k != key);
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }
        self.entries.push((
            key,
            CacheEntry {
                selected_ids,
                created_at: Instant::now(),
            },
        ));
    }

    /// Number of cached entries.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Side-query selector: pre-filters memories using a lightweight model.
///
/// Wraps a [`SideQueryRanker`] with `already_surfaced` tracking and LRU
/// caching. Designed to run as a pre-filter stage before the 6-factor
/// recall scoring in [`RecallEngine`](crate::recall::RecallEngine).
pub struct SideQuerySelector {
    config: SideQueryConfig,
    // WHY: std::sync::Mutex — lock never held across .await.
    already_surfaced: Mutex<HashSet<String>>,
    cache: Mutex<RelevanceCache>,
}

impl SideQuerySelector {
    /// Create a new selector with the given configuration.
    #[must_use]
    pub fn new(config: SideQueryConfig) -> Self {
        let cache = RelevanceCache::new(
            config.cache_capacity,
            Duration::from_secs(config.cache_ttl_secs),
        );
        Self {
            config,
            already_surfaced: Mutex::new(HashSet::new()),
            cache: Mutex::new(cache),
        }
    }

    /// Select relevant memory entries from the manifest.
    ///
    /// Filters out `already_surfaced` entries, checks the cache, and
    /// falls back to the ranker on cache miss.
    ///
    /// # Errors
    ///
    /// Returns [`SideQueryError`] if the ranker call fails and no cached
    /// result is available.
    #[must_use = "selection result should be used for pre-filtering"]
    #[instrument(skip_all, fields(manifest_len = manifest.len()))]
    pub fn select(
        &self,
        query: &str,
        manifest: &MemoryManifest,
        ranker: &dyn SideQueryRanker,
    ) -> Result<SideQueryResult, SideQueryError> {
        if !self.config.enabled || manifest.is_empty() {
            return Ok(SideQueryResult {
                selected_ids: Vec::new(),
                from_cache: false,
            });
        }

        let filtered = self.filter_surfaced(manifest);
        if filtered.is_empty() {
            debug!("all manifest entries already surfaced");
            return Ok(SideQueryResult {
                selected_ids: Vec::new(),
                from_cache: false,
            });
        }

        let manifest_text = filtered.format();
        let cache_key = compute_cache_key(query, &manifest_text);

        {
            let mut cache = self
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(ids) = cache.get(cache_key) {
                debug!(count = ids.len(), "side-query cache hit");
                return Ok(SideQueryResult {
                    selected_ids: ids,
                    from_cache: true,
                });
            }
        }

        // NOTE: cache miss — call the ranker.
        let selected = ranker.rank_memories(query, &manifest_text, self.config.max_results)?;

        debug!(count = selected.len(), "side-query selected memories");

        // NOTE: store in cache.
        {
            let mut cache = self
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.insert(cache_key, selected.clone());
        }

        Ok(SideQueryResult {
            selected_ids: selected,
            from_cache: false,
        })
    }

    /// Mark source IDs as surfaced so they won't be re-selected.
    pub fn mark_surfaced(&self, ids: &[String]) {
        let mut surfaced = self
            .already_surfaced
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for id in ids {
            surfaced.insert(id.clone());
        }
    }

    /// Check whether a source ID has already been surfaced.
    #[must_use]
    pub fn is_surfaced(&self, id: &str) -> bool {
        self.already_surfaced
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
    }

    /// Number of entries in the relevance cache.
    #[must_use]
    pub fn cache_len(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Build a filtered manifest excluding already-surfaced entries.
    fn filter_surfaced(&self, manifest: &MemoryManifest) -> MemoryManifest {
        let surfaced = self
            .already_surfaced
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let filtered: Vec<_> = manifest
            .headers()
            .iter()
            .filter(|h| !surfaced.contains(&h.source_id))
            .cloned()
            .collect();
        MemoryManifest::from_headers(filtered)
    }
}

impl fmt::Debug for SideQuerySelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SideQuerySelector")
            .field("config", &self.config)
            .field(
                "surfaced_count",
                &self.already_surfaced.lock().map(|s| s.len()).unwrap_or(0),
            )
            .field("cache_len", &self.cache_len())
            .finish_non_exhaustive()
    }
}

/// Compute a cache key from query and manifest text.
///
/// Uses [`DefaultHasher`] (`SipHash`) which is fast and collision-resistant
/// for in-memory session-scoped caching.
pub(crate) fn compute_cache_key(query: &str, manifest_text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    manifest_text.hash(&mut hasher);
    hasher.finish()
}
