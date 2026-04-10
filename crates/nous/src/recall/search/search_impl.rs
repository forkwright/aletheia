//! Search trait implementations for recall retrieval.

#[cfg(feature = "knowledge-store")]
use std::sync::Arc;

#[cfg(feature = "knowledge-store")]
use crate::error;

#[cfg(feature = "knowledge-store")]
use mneme::knowledge::RecallResult as KnowledgeRecallResult;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;

#[cfg(feature = "knowledge-store")]
use crate::recall::search::TextSearch;
#[cfg(feature = "knowledge-store")]
use crate::recall::search::VectorSearch;

/// Bridges [`mneme::knowledge_store::KnowledgeStore::search_text_for_recall`] to [`TextSearch`].
#[cfg(feature = "knowledge-store")]
pub struct KnowledgeTextSearch {
    store: Arc<KnowledgeStore>,
}

#[cfg(feature = "knowledge-store")]
impl KnowledgeTextSearch {
    /// Create a text search adapter wrapping the given knowledge store.
    #[must_use]
    pub(crate) fn new(store: Arc<KnowledgeStore>) -> Self {
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
