//! HNSW hot-swap infrastructure for embedding model upgrades.
//!
//! Provides zero-downtime model switching with Recall@K evaluation gating.
//! The swap process:
//! 1. Load new model into a temporary EmbeddingProvider
//! 2. Re-embed sample facts using the new model
//! 3. Run Recall@K evaluation comparing old vs new embeddings
//! 4. If threshold met: rebuild HNSW index with new embeddings
//! 5. Atomic swap: replace the active embedding provider reference

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use snafu::Snafu;
use tracing::instrument;

use crate::embedding::{create_provider, EmbeddingConfig, EmbeddingProvider};
use crate::embedding_eval::{compare_models, EvalDataset, EvalError, ModelMetrics};
use crate::hnsw_index::{HnswConfig, HnswIndex};

/// Errors from embedding swap operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, location) are self-documenting via display format"
)]
pub enum SwapError {
    /// Failed to initialize the new embedding provider.
    #[snafu(display("new model init failed: {message}"))]
    InitFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Evaluation failed during swap validation.
    #[snafu(display("evaluation failed: {source}"))]
    EvalFailed {
        source: EvalError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HNSW index operation failed.
    #[snafu(display("HNSW index error: {message}"))]
    HnswIndex {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding operation failed during re-embedding.
    #[snafu(display("embedding failed: {message}"))]
    EmbedFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Sample corpus is empty - cannot evaluate.
    #[snafu(display("sample corpus is empty: cannot evaluate new model"))]
    EmptySample {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result type for swap operations.
pub type SwapResult<T> = std::result::Result<T, SwapError>;

/// Configuration for embedding model hot-swap.
#[derive(Debug, Clone)]
pub struct EmbeddingSwapConfig {
    /// Path to the new model weights/directory.
    pub new_model_path: PathBuf,
    /// Minimum Recall@K threshold for promotion (e.g., 0.75).
    pub eval_threshold_recall_at_k: f64,
    /// K value for Recall@K evaluation (e.g., 5).
    pub k: usize,
    /// Benchmark queries for evaluation.
    pub eval_queries: Vec<String>,
    /// Optional: path to evaluation dataset file (JSONL format).
    /// If provided, overrides `eval_queries`.
    pub eval_dataset_path: Option<PathBuf>,
}

impl EmbeddingSwapConfig {
    /// Create a new swap config with the given model path and threshold.
    #[must_use]
    pub fn new(new_model_path: PathBuf, threshold: f64, k: usize) -> Self {
        Self {
            new_model_path,
            eval_threshold_recall_at_k: threshold,
            k,
            eval_queries: Vec::new(),
            eval_dataset_path: None,
        }
    }

    /// Set evaluation queries.
    #[must_use]
    pub fn with_queries(mut self, queries: Vec<String>) -> Self {
        self.eval_queries = queries;
        self
    }

    /// Set evaluation dataset path.
    #[must_use]
    pub fn with_dataset_path(mut self, path: PathBuf) -> Self {
        self.eval_dataset_path = Some(path);
        self
    }
}

/// Result of a swap attempt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SwapResult {
    /// Name/identifier of the old model.
    pub old_model: String,
    /// Name/identifier of the new model.
    pub new_model: String,
    /// Achieved Recall@K score for the new model.
    pub eval_recall_at_k: f64,
    /// Whether the new model was promoted to active.
    pub promoted: bool,
    /// Duration of the swap operation in milliseconds.
    pub duration_ms: u64,
    /// Detailed metrics for the new model (if evaluation succeeded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_model_metrics: Option<ModelMetrics>,
    /// Failure reason (if swap failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

/// A sample fact for re-embedding during swap evaluation.
#[derive(Debug, Clone)]
pub struct SampleFact {
    /// Fact ID.
    pub id: String,
    /// Fact content text.
    pub content: String,
    /// Current embedding vector (from old model).
    pub old_embedding: Vec<f32>,
}

/// Manager for embedding model hot-swapping.
///
/// Holds references to the active embedding provider and HNSW index,
/// allowing atomic swap of both during model upgrades.
pub struct EmbeddingSwapManager {
    active_provider: Arc<dyn EmbeddingProvider>,
    hnsw_index: Arc<HnswIndex>,
    hnsw_config: HnswConfig,
}

impl EmbeddingSwapManager {
    /// Create a new swap manager with the given active provider and index.
    pub fn new(
        active_provider: Arc<dyn EmbeddingProvider>,
        hnsw_index: Arc<HnswIndex>,
        hnsw_config: HnswConfig,
    ) -> Self {
        Self {
            active_provider,
            hnsw_index,
            hnsw_config,
        }
    }

    /// Attempt to swap to a new embedding model.
    ///
    /// # Steps
    /// 1. Load new model into a temporary provider
    /// 2. Re-embed sample facts using the new model
    /// 3. Run Recall@K evaluation comparing old vs new embeddings
    /// 4. If new model meets threshold: rebuild HNSW index with new embeddings
    /// 5. Atomic swap: replace the active embedding provider reference
    ///
    /// # Errors
    /// Returns `SwapError` if initialization, evaluation, or index operations fail.
    #[instrument(skip(self, sample_facts), fields(sample_size = sample_facts.len()))]
    pub fn attempt_swap(
        &mut self,
        config: &EmbeddingSwapConfig,
        sample_facts: Vec<SampleFact>,
    ) -> SwapResult<SwapResult> {
        let start = Instant::now();
        let old_model_name = self.active_provider.model_name().to_owned();

        // Check sample corpus
        if sample_facts.is_empty() {
            return EmptySampleSnafu.fail();
        }

        // Step 1: Load new model into a temporary provider
        let new_provider = self.load_new_model(config)?;
        let new_model_name = new_provider.model_name().to_owned();

        // Build evaluation corpus from sample facts
        let corpus: Vec<(String, String)> = sample_facts
            .iter()
            .map(|f| (f.id.clone(), f.content.clone()))
            .collect();

        // Load or build evaluation dataset
        let dataset = self.load_or_build_dataset(config, &corpus)?;

        // Step 2 & 3: Run evaluation comparing old vs new model
        let eval_result = self
            .run_evaluation(&*self.active_provider, &*new_provider, &dataset, &corpus, config.k)
            .map_err(|e| SwapError::EvalFailed {
                source: e,
                })?;

        let new_metrics = eval_result.candidate.clone();
        let recall_at_k = new_metrics.as_ref().map(|m| m.recall_at_k).unwrap_or(0.0);
        let meets_threshold = recall_at_k >= config.eval_threshold_recall_at_k;

        if !meets_threshold {
            let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            tracing::warn!(
                old_model = %old_model_name,
                new_model = %new_model_name,
                recall_at_k = %recall_at_k,
                threshold = %config.eval_threshold_recall_at_k,
                "new model failed evaluation threshold"
            );
            return Ok(SwapResult {
                old_model: old_model_name,
                new_model: new_model_name,
                eval_recall_at_k: recall_at_k,
                promoted: false,
                duration_ms,
                new_model_metrics: new_metrics,
                failure_reason: Some(format!(
                    "Recall@K {} below threshold {}",
                    recall_at_k, config.eval_threshold_recall_at_k
                )),
            });
        }

        // Step 4: Rebuild HNSW index with new embeddings
        self.rebuild_hnsw_index(&sample_facts, &*new_provider)?;

        // Step 5: Atomic swap of the active provider
        // Note: In a real implementation, this would use Arc::swap or similar
        // For now, we update our local reference
        self.active_provider = new_provider;

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        tracing::info!(
            old_model = %old_model_name,
            new_model = %new_model_name,
            recall_at_k = %recall_at_k,
            duration_ms = %duration_ms,
            "embedding model swap completed successfully"
        );

        Ok(SwapResult {
            old_model: old_model_name,
            new_model: new_model_name,
            eval_recall_at_k: recall_at_k,
            promoted: true,
            duration_ms,
            new_model_metrics: new_metrics,
            failure_reason: None,
        })
    }

    /// Load the new model from the configured path.
    fn load_new_model(&self, config: &EmbeddingSwapConfig) -> SwapResult<Arc<dyn EmbeddingProvider>> {
        // Build an EmbeddingConfig that points to the new model
        let embed_config = EmbeddingConfig {
            provider: "candle".to_owned(),
            model: Some(config.new_model_path.to_string_lossy().to_string()),
            dimension: Some(self.hnsw_config.dim),
            api_key: None,
        };

        create_provider(&embed_config)
            .map(Arc::from)
            .map_err(|e| SwapError::InitFailed {
                message: e.to_string(),
                location: snafu::Location::new(),
            })
    }

    /// Load evaluation dataset from file or build from queries.
    fn load_or_build_dataset(
        &self,
        config: &EmbeddingSwapConfig,
        corpus: &[(String, String)],
    ) -> SwapResult<EvalDataset> {
        // If dataset path provided, load from file
        if let Some(ref path) = config.eval_dataset_path {
            let dataset = crate::embedding_eval::EvalDataset::from_jsonl_file(path)
                .map_err(|e| SwapError::EvalFailed {
                    source: e,
                    })?;
            return Ok(dataset);
        }

        // Otherwise, build dataset from eval_queries and corpus
        // Each query's relevant IDs are the corpus items most similar under current model
        let queries: Vec<crate::embedding_eval::EvalQuery> = config
            .eval_queries
            .iter()
            .map(|q| {
                // Find top-K most similar corpus items under current model
                let relevant_ids = self.find_relevant_for_query(q, corpus, config.k);
                crate::embedding_eval::EvalQuery {
                    query: q.clone(),
                    relevant_ids,
                    description: None,
                }
            })
            .collect();

        Ok(EvalDataset { queries })
    }

    /// Find relevant corpus IDs for a query using the current model.
    fn find_relevant_for_query(
        &self,
        query: &str,
        corpus: &[(String, String)],
        k: usize,
    ) -> Vec<String> {
        let Ok(query_vec) = self.active_provider.embed(query) else {
            return Vec::new();
        };

        // Compute similarities and take top k
        let mut scored: Vec<(String, f32)> = corpus
            .iter()
            .filter_map(|(id, text)| {
                self.active_provider.embed(text).ok().map(|emb| {
                    let sim = cosine_similarity(&query_vec, &emb);
                    (id.clone(), sim)
                })
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored.into_iter().map(|(id, _)| id).collect()
    }

    /// Run evaluation comparing baseline and candidate models.
    fn run_evaluation(
        &self,
        baseline: &dyn EmbeddingProvider,
        candidate: &dyn EmbeddingProvider,
        dataset: &EvalDataset,
        corpus: &[(String, String)],
        k: usize,
    ) -> Result<crate::embedding_eval::EvalRunResult, EvalError> {
        compare_models(baseline, Some(candidate), dataset, corpus, k)
    }

    /// Rebuild the HNSW index with new embeddings.
    fn rebuild_hnsw_index(
        &mut self,
        sample_facts: &[SampleFact],
        new_provider: &dyn EmbeddingProvider,
    ) -> SwapResult<()> {
        // Create a new index with the same config
        let new_index = HnswIndex::new(self.hnsw_config.clone());

        // Re-embed all sample facts with the new model and insert
        for fact in sample_facts {
            let new_embedding = new_provider.embed(&fact.content).map_err(|e| {
                SwapError::EmbedFailed {
                    message: e.to_string(),
                    location: snafu::Location::new(),
                }
            })?;

            // Parse the fact ID as usize for HNSW data_id
            let data_id = fact
                .id
                .parse::<usize>()
                .unwrap_or_else(|_| fast_hash(&fact.id));

            new_index.insert(&new_embedding, data_id).map_err(|e| {
                SwapError::HnswIndex {
                    message: e.to_string(),
                    location: snafu::Location::new(),
                }
            })?;
        }

        // In a production implementation, we would:
        // 1. Re-embed ALL facts (not just samples) from the knowledge store
        // 2. Build the complete new index
        // 3. Perform an atomic swap of the index reference

        // For now, we just update our reference
        self.hnsw_index = new_index;

        Ok(())
    }

    /// Get a reference to the active embedding provider.
    pub fn active_provider(&self) -> &dyn EmbeddingProvider {
        &*self.active_provider
    }

    /// Get a reference to the HNSW index.
    pub fn hnsw_index(&self) -> &HnswIndex {
        &self.hnsw_index
    }
}

/// Fast hash function for string IDs to usize.
#[expect(
    clippy::cast_possible_truncation,
    reason = "u64→usize: hash output truncated intentionally for index bucketing"
)]
fn fast_hash(s: &str) -> usize {
    let mut hash: u64 = 5381;
    for b in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(b));
    }
    hash as usize
}

/// Cosine similarity between two L2-normalized f32 vectors (dot product).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic")]
mod tests {
    use super::*;
    use crate::embedding::MockEmbeddingProvider;

    fn mock_provider() -> Arc<dyn EmbeddingProvider> {
        Arc::new(MockEmbeddingProvider::new(384))
    }

    fn mock_hnsw_config() -> HnswConfig {
        HnswConfig {
            dim: 384,
            max_nb_connection: 16,
            ef_construction: 200,
            max_layer: 16,
            max_elements: 1000,
            persist_dir: None,
            persist_basename: "test_swap".to_owned(),
        }
    }

    #[test]
    fn swap_config_builder() {
        let config = EmbeddingSwapConfig::new(PathBuf::from("/models/new"), 0.75, 5)
            .with_queries(vec!["test query".to_owned()]);

        assert_eq!(config.new_model_path, PathBuf::from("/models/new"));
        assert!((config.eval_threshold_recall_at_k - 0.75).abs() < f64::EPSILON);
        assert_eq!(config.k, 5);
        assert_eq!(config.eval_queries.len(), 1);
    }

    #[test]
    fn swap_result_serialization() {
        let result = SwapResult {
            old_model: "old-model".to_owned(),
            new_model: "new-model".to_owned(),
            eval_recall_at_k: 0.85,
            promoted: true,
            duration_ms: 1234,
            new_model_metrics: None,
            failure_reason: None,
        };

        let json = serde_json::to_string(&result).expect("serialize");
        assert!(json.contains("old-model"));
        assert!(json.contains("new-model"));
        assert!(json.contains("0.85"));
    }

    #[test]
    fn manager_creation() {
        let provider = mock_provider();
        let index = HnswIndex::new(mock_hnsw_config());
        let manager =
            EmbeddingSwapManager::new(provider, index, mock_hnsw_config());

        assert_eq!(manager.active_provider.model_name(), "mock-embedding");
    }

    #[test]
    fn empty_sample_fails() {
        let provider = mock_provider();
        let index = HnswIndex::new(mock_hnsw_config());
        let mut manager =
            EmbeddingSwapManager::new(provider, index, mock_hnsw_config());

        let config = EmbeddingSwapConfig::new(PathBuf::from("/models/new"), 0.75, 5);
        let result = manager.attempt_swap(&config, vec![]);

        assert!(matches!(result, Err(SwapError::EmptySample { .. })));
    }

    #[test]
    fn cosine_similarity_computation() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![0.0_f32, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 0.001);

        let c = vec![1.0_f32, 0.0, 0.0];
        let same = cosine_similarity(&a, &c);
        assert!((same - 1.0).abs() < 0.001);
    }
}
