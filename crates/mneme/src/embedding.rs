//! Embedding provider trait and implementations.
//!
//! Defines the interface for text→vector embedding. Multiple backends:
//! - `candle` (local, pure Rust, no C++ deps, default for development)
//! - Voyage AI (production quality, API key required)
//! - Future: Ollama local models
//!
//! The trait is `Send + Sync` for use in async contexts.

use snafu::Snafu;
use tracing::instrument;

/// Errors from embedding operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum EmbeddingError {
    /// The embedding model failed to initialize.
    #[snafu(display("embedding model init failed: {message}"))]
    InitFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding a text chunk failed.
    #[snafu(display("embedding failed: {message}"))]
    EmbedFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The embedding model `RwLock` was poisoned by a prior panic.
    #[snafu(display("embedding model lock poisoned"))]
    LockPoisoned {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result type for embedding operations.
pub type EmbeddingResult<T> = std::result::Result<T, EmbeddingError>;

/// Trait for text→vector embedding providers.
///
/// Implementations must be `Send + Sync` for use across async boundaries.
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text chunk. Returns a vector of floats.
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>>;

    /// Embed multiple text chunks in a batch. Default implementation
    /// calls `embed` sequentially: providers should override for efficiency.
    fn embed_batch(&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// The dimensionality of output vectors.
    fn dimension(&self) -> usize;

    /// The model name/identifier.
    fn model_name(&self) -> &str;
}

/// A mock embedding provider for testing.
///
/// Produces deterministic vectors based on text hash.
/// Not suitable for real semantic similarity: use only in tests.
#[derive(Debug)]
pub struct MockEmbeddingProvider {
    dim: usize,
}

impl MockEmbeddingProvider {
    /// Create a mock provider with the given dimension.
    #[must_use]
    #[instrument]
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl EmbeddingProvider for MockEmbeddingProvider {
    #[instrument(skip(self, text))]
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        let mut vec = vec![0.0f32; self.dim];
        let bytes = text.as_bytes();
        let mut hash: u64 = 5381;
        for &b in bytes {
            hash = hash.wrapping_mul(33).wrapping_add(u64::from(b));
        }
        for (i, v) in vec.iter_mut().enumerate() {
            let h = hash
                .wrapping_mul(i as u64 + 1)
                .wrapping_add(i as u64 * 2_654_435_761);
            #[expect(clippy::cast_precision_loss, reason = "hash modulo fits in f32")]
            {
                *v = ((h % 10000) as f32 / 5000.0) - 1.0;
            }
        }
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        Ok(vec)
    }

    #[instrument(skip(self))]
    fn dimension(&self) -> usize {
        self.dim
    }

    #[instrument(skip(self))]
    fn model_name(&self) -> &str {
        "mock-embedding"
    }
}

#[cfg(feature = "embed-candle")]
mod candle_provider {
    use super::{
        EmbedFailedSnafu, EmbeddingProvider, EmbeddingResult, InitFailedSnafu, LockPoisonedSnafu,
    };
    use candle_core::{DType, Device, Tensor};
    use candle_nn::VarBuilder;
    use candle_transformers::models::bert::{BertModel, Config as BertConfig};
    use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer};
    use tracing::instrument;

    /// Local embedding provider using candle (pure Rust).
    ///
    /// Downloads and caches models from `HuggingFace` Hub on first use.
    /// Default model is `BAAI/bge-small-en-v1.5` (384 dimensions).
    ///
    /// Thread-safe via `RwLock`: multiple concurrent reads (embedding requests)
    /// proceed in parallel. Write locks are only needed for model reload.
    pub struct CandelProvider {
        model: std::sync::RwLock<BertModel>,
        tokenizer: std::sync::RwLock<Tokenizer>,
        model_name: String,
        dimension: usize,
        device: Device,
    }

    impl std::fmt::Debug for CandelProvider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("CandelProvider")
                .field("model_name", &self.model_name)
                .field("dimension", &self.dimension)
                .finish_non_exhaustive()
        }
    }

    impl CandelProvider {
        /// Default `HuggingFace` model repo for `BGE-small-en-v1.5`.
        const DEFAULT_REPO: &str = "BAAI/bge-small-en-v1.5";
        /// Create a provider with the given model repo, or the default (`BAAI/bge-small-en-v1.5`).
        ///
        /// Model files are downloaded to the `HuggingFace` cache on first use.
        ///
        /// # Errors
        ///
        /// Returns `EmbeddingError::InitFailed` if model download or initialization fails.
        #[instrument]
        pub fn new(model_repo: Option<&str>) -> EmbeddingResult<Self> {
            let repo_id = model_repo.unwrap_or(Self::DEFAULT_REPO);
            let device = Device::Cpu;

            let api = hf_hub::api::sync::Api::new().map_err(|e| {
                InitFailedSnafu {
                    message: format!("hf-hub API init failed: {e}"),
                }
                .build()
            })?;
            let repo = api.model(repo_id.to_owned());

            let config_path = repo.get("config.json").map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to download config.json: {e}"),
                }
                .build()
            })?;
            let tokenizer_path = repo.get("tokenizer.json").map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to download tokenizer.json: {e}"),
                }
                .build()
            })?;
            let weights_path = repo.get("model.safetensors").map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to download model.safetensors: {e}"),
                }
                .build()
            })?;

            let config_str = std::fs::read_to_string(&config_path).map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to read config.json: {e}"),
                }
                .build()
            })?;
            let config: BertConfig = serde_json::from_str(&config_str).map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to parse config.json: {e}"),
                }
                .build()
            })?;
            let dimension = config.hidden_size;

            let weights_data = std::fs::read(&weights_path).map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to read model weights: {e}"),
                }
                .build()
            })?;
            let vb = VarBuilder::from_buffered_safetensors(weights_data, DType::F32, &device)
                .map_err(|e| {
                    InitFailedSnafu {
                        message: format!("failed to parse model weights: {e}"),
                    }
                    .build()
                })?;

            let model = BertModel::load(vb, &config).map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to load BERT model: {e}"),
                }
                .build()
            })?;

            let mut tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to load tokenizer: {e}"),
                }
                .build()
            })?;
            tokenizer.with_padding(Some(PaddingParams {
                strategy: PaddingStrategy::BatchLongest,
                ..Default::default()
            }));

            Ok(Self {
                model: std::sync::RwLock::new(model),
                tokenizer: std::sync::RwLock::new(tokenizer),
                model_name: repo_id.to_owned(),
                dimension,
                device,
            })
        }

        /// Map a candle error to an [`EmbeddingError`].
        fn candle_err(msg: &str) -> impl FnOnce(candle_core::Error) -> super::EmbeddingError + '_ {
            move |e| {
                EmbedFailedSnafu {
                    message: format!("{msg}: {e}"),
                }
                .build()
            }
        }

        /// Tokenize, run model forward pass, and return raw hidden states + attention mask.
        ///
        /// Uses read locks on both tokenizer and model, allowing multiple
        /// concurrent embedding requests to proceed in parallel.
        fn encode_and_forward(&self, texts: &[&str]) -> EmbeddingResult<(Tensor, Tensor)> {
            // WHY: Read lock allows concurrent tokenization across callers.
            // Tokenizer::encode_batch takes &self, no mutation needed.
            let tokenizer = self
                .tokenizer
                .read()
                .map_err(|_poison| LockPoisonedSnafu.build())?;

            let encodings = tokenizer.encode_batch(texts.to_vec(), true).map_err(|e| {
                EmbedFailedSnafu {
                    message: format!("tokenization failed: {e}"),
                }
                .build()
            })?;
            drop(tokenizer);

            let token_ids: Vec<Tensor> = encodings
                .iter()
                .map(|enc| Tensor::new(enc.get_ids(), &self.device))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Self::candle_err("tensor creation"))?;
            let input_ids =
                Tensor::stack(&token_ids, 0).map_err(Self::candle_err("tensor stack"))?;
            let token_type_ids = input_ids
                .zeros_like()
                .map_err(Self::candle_err("zeros_like"))?;

            let attention_masks: Vec<Tensor> = encodings
                .iter()
                .map(|enc| Tensor::new(enc.get_attention_mask(), &self.device))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Self::candle_err("attention mask tensor"))?;
            let attention_mask =
                Tensor::stack(&attention_masks, 0).map_err(Self::candle_err("mask stack"))?;

            // WHY: Read lock allows concurrent forward passes.
            // BertModel::forward takes &self, no mutation needed.
            let model = self
                .model
                .read()
                .map_err(|_poison| LockPoisonedSnafu.build())?;
            let embeddings = model
                .forward(&input_ids, &token_type_ids, Some(&attention_mask))
                .map_err(Self::candle_err("model forward pass"))?;
            drop(model);

            Ok((embeddings, attention_mask))
        }

        /// Mean-pool hidden states using attention mask, then L2-normalize.
        fn pool_and_normalize(
            embeddings: &Tensor,
            attention_mask: &Tensor,
            batch_size: usize,
        ) -> EmbeddingResult<Vec<Vec<f32>>> {
            let mask_f32 = attention_mask
                .unsqueeze(2)
                .and_then(|t| t.to_dtype(DType::F32))
                .map_err(Self::candle_err("mask expansion"))?;
            let summed = embeddings
                .broadcast_mul(&mask_f32)
                .and_then(|t| t.sum(1))
                .map_err(Self::candle_err("masked sum"))?;
            let mask_sum = mask_f32.sum(1).map_err(Self::candle_err("mask sum"))?;
            let pooled = summed
                .broadcast_div(&mask_sum)
                .map_err(Self::candle_err("pooling div"))?;

            let norm = pooled
                .sqr()
                .and_then(|t| t.sum_keepdim(1))
                .and_then(|t| t.sqrt())
                .map_err(Self::candle_err("norm computation"))?;
            // WHY: Clamp norm to prevent NaN from zero-norm input after L2 normalization.
            let norm_safe = norm
                .clamp(f32::EPSILON, f32::MAX)
                .map_err(Self::candle_err("norm clamp"))?;
            let normalized = pooled
                .broadcast_div(&norm_safe)
                .map_err(Self::candle_err("normalization"))?;

            let mut results = Vec::with_capacity(batch_size);
            for i in 0..batch_size {
                let vec: Vec<f32> = normalized
                    .get(i)
                    .and_then(|r| r.to_vec1())
                    .map_err(Self::candle_err("tensor extraction"))?;
                results.push(vec);
            }
            Ok(results)
        }

        /// Run forward pass and return mean-pooled, L2-normalized embeddings.
        fn forward_embed(&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>> {
            if texts.is_empty() {
                return Ok(vec![]);
            }
            let (embeddings, attention_mask) = self.encode_and_forward(texts)?;
            Self::pool_and_normalize(&embeddings, &attention_mask, texts.len())
        }
    }

    #[cfg(test)]
    #[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
    mod tests {
        use super::*;
        use candle_core::{DType, Device, Tensor};

        /// Zero-norm input must not produce NaN after L2 normalisation.
        ///
        /// When mean-pooling over an all-zero hidden-state tensor the resulting
        /// pooled vector has norm 0.  Without the clamp this becomes 0/0 = NaN.
        #[test]
        fn pool_and_normalize_zero_input_no_nan() {
            let device = Device::Cpu;
            let embeddings =
                Tensor::zeros(&[1usize, 2usize, 4usize], DType::F32, &device).expect("zero tensor");
            let attention_mask =
                Tensor::ones(&[1usize, 2usize], DType::F32, &device).expect("ones mask");
            let result = CandelProvider::pool_and_normalize(&embeddings, &attention_mask, 1)
                .expect("pool_and_normalize on zero input must not fail");
            assert_eq!(result.len(), 1, "batch size must be preserved");
            for v in &result[0] {
                assert!(!v.is_nan(), "zero-norm input must not produce NaN, got {v}");
            }
        }
    }

    impl EmbeddingProvider for CandelProvider {
        #[instrument(skip(self, text))]
        fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
            let mut results = self.forward_embed(&[text])?;
            results.pop().ok_or_else(|| {
                EmbedFailedSnafu {
                    message: "candle returned empty result".to_owned(),
                }
                .build()
            })
        }

        #[instrument(skip(self, texts), fields(batch_size = texts.len()))]
        fn embed_batch(&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>> {
            self.forward_embed(texts)
        }

        #[instrument(skip(self))]
        fn dimension(&self) -> usize {
            self.dimension
        }

        #[instrument(skip(self))]
        fn model_name(&self) -> &str {
            &self.model_name
        }
    }
}

#[cfg(feature = "embed-candle")]
pub use candle_provider::CandelProvider;

/// Embedding provider configuration.
///
/// Available providers:
/// - `"mock"`: deterministic hash-based vectors for testing (always available)
/// - `"candle"`: local pure-Rust embeddings via candle (requires `embed-candle` feature)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmbeddingConfig {
    /// Provider type: `mock`, `candle`, `voyage`.
    pub provider: String,
    /// Model name (provider-specific).
    pub model: Option<String>,
    /// Output dimension (auto-detected from model if not set).
    pub dimension: Option<usize>,
    /// API key (for cloud providers).
    pub api_key: Option<String>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: "mock".to_owned(),
            model: None,
            dimension: Some(384),
            api_key: None,
        }
    }
}

/// Create an embedding provider from configuration.
///
/// # Errors
/// Returns an error if the provider cannot be initialized.
#[instrument(skip(config), fields(provider = %config.provider))]
pub fn create_provider(config: &EmbeddingConfig) -> EmbeddingResult<Box<dyn EmbeddingProvider>> {
    match config.provider.as_str() {
        "mock" => {
            let dim = config.dimension.unwrap_or(384);
            Ok(Box::new(MockEmbeddingProvider::new(dim)))
        }
        #[cfg(feature = "embed-candle")]
        "candle" => {
            let model = config.model.as_deref();
            Ok(Box::new(CandelProvider::new(model)?))
        }
        #[cfg(not(feature = "embed-candle"))]
        "candle" => InitFailedSnafu {
            message: "embed-candle feature not enabled — build with --features embed-candle"
                .to_owned(),
        }
        .fail(),
        other => InitFailedSnafu {
            message: format!("unknown embedding provider: {other}"),
        }
        .fail(),
    }
}

#[cfg(test)]
#[path = "embedding_tests.rs"]
mod tests;
