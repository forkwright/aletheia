//! Search traits and adapters for recall retrieval.

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::RecallResult as KnowledgeRecallResult;

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
