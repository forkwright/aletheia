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

    /// The embedding model mutex was poisoned by a prior panic.
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
    /// calls `embed` sequentially — providers should override for efficiency.
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
/// Not suitable for real semantic similarity — use only in tests.
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
        // Deterministic pseudo-embedding from text content.
        // Uses a simple multiplicative hash to spread bytes across dimensions.
        let mut vec = vec![0.0f32; self.dim];
        let bytes = text.as_bytes();
        let mut hash: u64 = 5381;
        for &b in bytes {
            hash = hash.wrapping_mul(33).wrapping_add(u64::from(b));
        }
        for (i, v) in vec.iter_mut().enumerate() {
            // Mix hash with position for per-dimension variation
            let h = hash
                .wrapping_mul(i as u64 + 1)
                .wrapping_add(i as u64 * 2_654_435_761);
            #[expect(clippy::cast_precision_loss, reason = "hash modulo fits in f32")]
            {
                *v = ((h % 10000) as f32 / 5000.0) - 1.0;
            }
        }
        // L2 normalize
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

// ---------------------------------------------------------------------------
// Candle provider (pure Rust, no C++ dependencies)
// ---------------------------------------------------------------------------

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
    /// Thread-safe via internal mutex.
    pub struct CandelProvider {
        model: std::sync::Mutex<BertModel>,
        tokenizer: std::sync::Mutex<Tokenizer>,
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
        /// Returns [`EmbeddingError::InitFailed`] if model download or initialization fails.
        #[instrument]
        pub fn new(model_repo: Option<&str>) -> EmbeddingResult<Self> {
            let repo_id = model_repo.unwrap_or(Self::DEFAULT_REPO);
            let device = Device::Cpu;

            // Download model files from HuggingFace Hub
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

            // Load config
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

            // Load model weights (safe buffered read, no mmap)
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

            // Load tokenizer
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
                model: std::sync::Mutex::new(model),
                tokenizer: std::sync::Mutex::new(tokenizer),
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
        fn encode_and_forward(&self, texts: &[&str]) -> EmbeddingResult<(Tensor, Tensor)> {
            let tokenizer = self
                .tokenizer
                .lock()
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

            let model = self
                .model
                .lock()
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
            let normalized = pooled
                .broadcast_div(&norm)
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
/// - `"mock"` — deterministic hash-based vectors for testing (always available)
/// - `"candle"` — local pure-Rust embeddings via candle (requires `embed-candle` feature)
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
        // "voyage" => { ... }   // M1.3 Phase 3: Voyage AI API
        other => InitFailedSnafu {
            message: format!("unknown embedding provider: {other}"),
        }
        .fail(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_provider_produces_correct_dimension() {
        let provider = MockEmbeddingProvider::new(384);
        let vec = provider.embed("hello world").unwrap();
        assert_eq!(vec.len(), 384);
    }

    #[test]
    fn mock_provider_is_deterministic() {
        let provider = MockEmbeddingProvider::new(64);
        let v1 = provider.embed("test input").unwrap();
        let v2 = provider.embed("test input").unwrap();
        assert_eq!(v1, v2);
    }

    #[test]
    fn mock_provider_different_texts_differ() {
        let provider = MockEmbeddingProvider::new(64);
        let v1 = provider.embed("hello").unwrap();
        let v2 = provider.embed("world").unwrap();
        assert_ne!(v1, v2);
    }

    #[test]
    fn mock_provider_is_normalized() {
        let provider = MockEmbeddingProvider::new(128);
        let vec = provider.embed("normalize me").unwrap();
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "expected unit norm, got {norm}");
    }

    #[test]
    fn batch_embed_matches_individual() {
        let provider = MockEmbeddingProvider::new(64);
        let texts = ["hello", "world", "test"];
        let batch = provider.embed_batch(&texts).unwrap();
        for (i, text) in texts.iter().enumerate() {
            let individual = provider.embed(text).unwrap();
            assert_eq!(batch[i], individual);
        }
    }

    #[test]
    fn create_mock_provider() {
        let config = EmbeddingConfig::default();
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.dimension(), 384);
        assert_eq!(provider.model_name(), "mock-embedding");
    }

    #[test]
    fn create_unknown_provider_fails() {
        let config = EmbeddingConfig {
            provider: "nonexistent".to_owned(),
            ..EmbeddingConfig::default()
        };
        assert!(create_provider(&config).is_err());
    }

    #[test]
    fn mock_provider_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockEmbeddingProvider>();
    }

    #[test]
    fn embedding_empty_input() {
        let provider = MockEmbeddingProvider::new(64);
        let result = provider.embed("");
        assert!(
            result.is_ok(),
            "empty string should produce a valid embedding"
        );
        let vec = result.unwrap();
        assert_eq!(vec.len(), 64);
    }

    #[test]
    fn embedding_long_input() {
        let provider = MockEmbeddingProvider::new(128);
        let long_text = "word ".repeat(10_000);
        let result = provider.embed(&long_text);
        assert!(result.is_ok(), "long input should succeed");
        let vec = result.unwrap();
        assert_eq!(vec.len(), 128);
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 0.01,
            "long input embedding should be normalized, got {norm}"
        );
    }

    #[test]
    fn embedding_provider_switching() {
        let small = create_provider(&EmbeddingConfig {
            provider: "mock".to_owned(),
            dimension: Some(64),
            ..EmbeddingConfig::default()
        })
        .unwrap();

        let large = create_provider(&EmbeddingConfig {
            provider: "mock".to_owned(),
            dimension: Some(256),
            ..EmbeddingConfig::default()
        })
        .unwrap();

        assert_eq!(small.dimension(), 64);
        assert_eq!(large.dimension(), 256);

        let v_small = small.embed("test").unwrap();
        let v_large = large.embed("test").unwrap();
        assert_eq!(v_small.len(), 64);
        assert_eq!(v_large.len(), 256);
        assert_ne!(v_small.len(), v_large.len());
    }

    #[test]
    fn create_provider_custom_dimension() {
        let config = EmbeddingConfig {
            provider: "mock".to_owned(),
            dimension: Some(512),
            ..EmbeddingConfig::default()
        };
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.dimension(), 512);

        let vec = provider.embed("custom dim").unwrap();
        assert_eq!(vec.len(), 512);
    }

    #[test]
    fn embedding_batch_empty_list() {
        let provider = MockEmbeddingProvider::new(64);
        let result = provider.embed_batch(&[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn mock_provider_consistent_dimension() {
        let provider = MockEmbeddingProvider::new(256);
        assert_eq!(provider.dimension(), 256);
        let vec = provider.embed("consistency check").unwrap();
        assert_eq!(
            vec.len(),
            provider.dimension(),
            "dimension() must match actual vector length"
        );
    }

    #[test]
    fn mock_provider_batch_empty() {
        let provider = MockEmbeddingProvider::new(128);
        let result = provider.embed_batch(&[]).unwrap();
        assert!(result.is_empty(), "batch of empty slice returns empty vec");
    }

    #[test]
    fn mock_provider_different_texts_same_dim() {
        let provider = MockEmbeddingProvider::new(96);
        let inputs = ["alpha", "beta", "gamma", "delta", ""];
        for input in &inputs {
            let vec = provider.embed(input).unwrap();
            assert_eq!(
                vec.len(),
                96,
                "all inputs must produce vectors of configured dimension"
            );
        }
    }

    #[test]
    fn create_provider_mock_config() {
        let config = EmbeddingConfig {
            provider: "mock".to_owned(),
            model: Some("custom-model".to_owned()),
            dimension: Some(768),
            api_key: None,
        };
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.dimension(), 768);
        assert_eq!(provider.model_name(), "mock-embedding");
        let vec = provider.embed("test").unwrap();
        assert_eq!(vec.len(), 768);
    }

    #[test]
    fn embed_empty_string() {
        let provider = MockEmbeddingProvider::new(64);
        let result = provider.embed("");
        assert!(result.is_ok(), "embedding empty string must not panic");
        let vec = result.unwrap();
        assert_eq!(vec.len(), 64);
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            norm < 1.1,
            "empty string embedding should be normalized or zero"
        );
    }

    #[test]
    fn embed_batch_single_item() {
        let provider = MockEmbeddingProvider::new(64);
        let single = provider.embed("solo").unwrap();
        let batch = provider.embed_batch(&["solo"]).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(
            batch[0], single,
            "batch of one must match single embed result"
        );
    }

    #[test]
    fn mock_embed_normalized() {
        let provider = MockEmbeddingProvider::new(256);
        let inputs = ["alpha", "bravo", "charlie delta echo"];
        for input in &inputs {
            let vec = provider.embed(input).unwrap();
            let magnitude: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!(
                (magnitude - 1.0).abs() < 0.001,
                "vector for {input:?} should be L2-normalized, got magnitude {magnitude}"
            );
        }
    }

    #[test]
    fn mock_embed_batch_matches_single() {
        let provider = MockEmbeddingProvider::new(128);
        let texts = ["foo bar", "baz qux", "hello world", "rust lang", ""];
        let batch = provider.embed_batch(&texts).unwrap();
        assert_eq!(batch.len(), texts.len());
        for (i, text) in texts.iter().enumerate() {
            let single = provider.embed(text).unwrap();
            assert_eq!(
                batch[i], single,
                "batch[{i}] must equal single embed for {text:?}"
            );
        }
    }

    #[test]
    fn mock_model_name() {
        let provider = MockEmbeddingProvider::new(64);
        assert_eq!(provider.model_name(), "mock-embedding");
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn embedding_dimensions_constant(input in "[a-zA-Z ]{1,100}") {
                let provider = MockEmbeddingProvider::new(384);
                let vec = provider.embed(&input).unwrap();
                prop_assert_eq!(vec.len(), 384);
                let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
                prop_assert!((norm - 1.0).abs() < 0.01, "norm was {}", norm);
            }
        }
    }

    #[cfg(not(feature = "embed-candle"))]
    #[test]
    fn candle_not_enabled_returns_error() {
        let config = EmbeddingConfig {
            provider: "candle".to_owned(),
            ..EmbeddingConfig::default()
        };
        let Err(err) = create_provider(&config) else {
            panic!("expected error for disabled embed-candle feature");
        };
        let msg = err.to_string();
        assert!(
            msg.contains("not enabled"),
            "expected 'not enabled' in error, got: {msg}"
        );
    }

    #[test]
    fn lock_poisoned_error_returns_err_not_panic() {
        use std::sync::Mutex;

        // Poison a mutex by panicking inside a thread while holding it.
        let m: Mutex<u32> = Mutex::new(0);
        let _ = std::panic::catch_unwind(|| {
            let _guard = m.lock().unwrap();
            panic!("intentional poison");
        });
        assert!(m.is_poisoned(), "mutex must be poisoned after thread panic");

        // Simulate what embed() does: map_err to LockPoisoned.
        let result: EmbeddingResult<()> = m
            .lock()
            .map_err(|_poison| LockPoisonedSnafu.build())
            .map(|_| ());
        assert!(
            matches!(result, Err(EmbeddingError::LockPoisoned { .. })),
            "poisoned lock must produce EmbeddingError::LockPoisoned"
        );
    }

    #[test]
    fn lock_poisoned_error_formats() {
        let err = LockPoisonedSnafu.build();
        assert_eq!(
            err.to_string(),
            "embedding model lock poisoned",
            "LockPoisoned display must match spec"
        );
    }

    #[cfg(feature = "embed-candle")]
    mod candle_tests {
        use super::*;
        use std::sync::LazyLock;

        static PROVIDER: LazyLock<CandelProvider> =
            LazyLock::new(|| CandelProvider::new(None).expect("candle provider init"));

        #[test]
        fn candle_provider_initializes() {
            assert_eq!(PROVIDER.dimension(), 384);
        }

        #[test]
        fn candle_embed_produces_correct_dimension() {
            let vec = PROVIDER.embed("hello world").unwrap();
            assert_eq!(vec.len(), 384);
        }

        #[test]
        fn candle_embed_is_normalized() {
            let vec = PROVIDER.embed("normalize me").unwrap();
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 0.01, "expected unit norm, got {norm}");
        }

        #[test]
        fn candle_embed_deterministic() {
            let v1 = PROVIDER.embed("test input").unwrap();
            let v2 = PROVIDER.embed("test input").unwrap();
            assert_eq!(v1, v2);
        }

        #[test]
        fn candle_different_texts_differ() {
            let v1 = PROVIDER.embed("hello").unwrap();
            let v2 = PROVIDER.embed("world").unwrap();
            assert_ne!(v1, v2);
        }

        #[test]
        fn candle_batch_matches_individual() {
            let texts = ["hello", "world", "test"];
            let batch = PROVIDER.embed_batch(&texts).unwrap();
            for (i, text) in texts.iter().enumerate() {
                let individual = PROVIDER.embed(text).unwrap();
                assert_eq!(batch[i], individual);
            }
        }

        #[test]
        fn candle_provider_send_sync() {
            fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<CandelProvider>();
        }
    }
}
