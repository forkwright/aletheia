//! Search traits and adapters for recall retrieval.

use mneme::embedding::EmbeddingProvider;
use mneme::knowledge::RecallResult as KnowledgeRecallResult;

use crate::error;

/// Abstracts BM25 text search for recall when no embedding provider is available.
///
/// Used as fallback when the embedding provider is in mock mode.
/// `KnowledgeStore` implements this when the `mneme-engine` feature is available.
pub(crate) trait TextSearch: Send + Sync {
    /// Search by text (BM25) and return the `k` best-matching results.
    fn search_text(&self, query: &str, k: usize) -> error::Result<Vec<KnowledgeRecallResult>>;
}

/// Abstracts vector knowledge search.
///
/// `KnowledgeStore` implements this when the `mneme-engine` feature is available.
/// For tests, use `MockVectorSearch`.
pub trait VectorSearch: Send + Sync {
    /// Search for the `k` nearest vectors with HNSW `ef` parameter.
    fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: usize,
        ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>>;

    /// Run tiered query-rewrite search when the backing store supports it.
    #[cfg(feature = "knowledge-store")]
    fn search_tiered(
        &self,
        _query: &str,
        _query_vec: Vec<f32>,
        _k: usize,
        _ef: usize,
        _rewrite_provider: &dyn mneme::query_rewrite::RewriteProvider,
    ) -> Option<error::Result<Vec<KnowledgeRecallResult>>> {
        None
    }
}

// Trait implementations and adapter types are in a separate module
// to avoid trait-impl colocation.
mod search_impl;

#[cfg(feature = "knowledge-store")]
pub use search_impl::{KnowledgeTextSearch, KnowledgeVectorSearch};

/// Embed a query string into a vector via the given provider.
pub(super) fn embed(query: &str, provider: &dyn EmbeddingProvider) -> error::Result<Vec<f32>> {
    provider.embed(query).map_err(|e| {
        error::RecallEmbeddingSnafu {
            message: e.to_string(),
        }
        .build()
    })
}

/// Run a vector search with default HNSW ef=50.
pub(super) fn vector_search(
    search: &dyn VectorSearch,
    query_vec: Vec<f32>,
    k: usize,
) -> error::Result<Vec<KnowledgeRecallResult>> {
    search.search_vectors(query_vec, k, 50).map_err(|e| {
        error::RecallSearchSnafu {
            message: e.to_string(),
        }
        .build()
    })
}

/// Run tiered search when available, otherwise fall back to vector search.
#[cfg(feature = "knowledge-store")]
pub(super) fn vector_search_tiered(
    search: &dyn VectorSearch,
    query: &str,
    query_vec: Vec<f32>,
    k: usize,
    rewrite_provider: &dyn mneme::query_rewrite::RewriteProvider,
) -> error::Result<Vec<KnowledgeRecallResult>> {
    if let Some(result) = search.search_tiered(query, query_vec.clone(), k, 50, rewrite_provider) {
        // WHY: `search_tiered` already wraps engine errors in `RecallSearchSnafu`
        // (see search/search_impl.rs); return its result directly rather than
        // wrapping a second time. The prior double-wrap produced the duplicated
        // "recall search failed: recall search failed:" message. See #4156.
        return result;
    }
    vector_search(search, query_vec, k)
}
