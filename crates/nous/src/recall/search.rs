//! Search traits and adapters for recall retrieval.

#[cfg(feature = "knowledge-store")]
use std::sync::Arc;

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::RecallResult as KnowledgeRecallResult;
#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;

use crate::error;

/// Abstracts BM25 text search for recall when no embedding provider is available.
///
/// Used as fallback when the embedding provider is in mock mode.
/// `KnowledgeStore` implements this when the `mneme-engine` feature is available.
pub(crate) trait TextSearch: Send + Sync {
    /// Search by text (BM25) and return the `k` best-matching results.
    fn search_text(&self, query: &str, k: usize) -> error::Result<Vec<KnowledgeRecallResult>>;
}

/// Bridges [`aletheia_mneme::knowledge_store::KnowledgeStore::search_text_for_recall`] to [`TextSearch`].
#[cfg(feature = "knowledge-store")]
pub(crate) struct KnowledgeTextSearch {
    store: Arc<KnowledgeStore>,
}

#[cfg(feature = "knowledge-store")]
impl KnowledgeTextSearch {
    /// Create a text search adapter wrapping the given knowledge store.
    #[must_use]
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[cfg(feature = "knowledge-store")]
impl TextSearch for KnowledgeTextSearch {
    fn search_text(&self, query: &str, k: usize) -> error::Result<Vec<KnowledgeRecallResult>> {
        let k_i64 = i64::try_from(k).unwrap_or(i64::MAX);
        self.store
            .search_text_for_recall(query, k_i64)
            .map_err(|e| {
                error::RecallSearchSnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }
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

/// Bridges [`KnowledgeStore::search_vectors`] to the [`VectorSearch`] trait.
#[cfg(feature = "knowledge-store")]
pub struct KnowledgeVectorSearch {
    store: Arc<KnowledgeStore>,
}

#[cfg(feature = "knowledge-store")]
impl KnowledgeVectorSearch {
    /// Create a vector search adapter wrapping the given knowledge store.
    #[must_use]
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[cfg(feature = "knowledge-store")]
impl VectorSearch for KnowledgeVectorSearch {
    fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: usize,
        ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>> {
        let k_i64 = i64::try_from(k).unwrap_or(i64::MAX);
        let ef_i64 = i64::try_from(ef).unwrap_or(i64::MAX);
        self.store
            .search_vectors(query_vec, k_i64, ef_i64)
            .map_err(|e| {
                error::RecallSearchSnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }
}

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
