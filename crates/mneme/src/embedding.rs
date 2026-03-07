//! Embedding provider trait and implementations.
//!
//! Defines the interface for text→vector embedding. Multiple backends:
//! - `fastembed-rs` (local, no API key, default for development)
//! - Voyage AI (production quality, API key required)
//! - Future: Ollama local models
//!
//! The trait is `Send + Sync` for use in async contexts.

use snafu::Snafu;

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
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl EmbeddingProvider for MockEmbeddingProvider {
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

    fn dimension(&self) -> usize {
        self.dim
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait requires &str return"
    )]
    fn model_name(&self) -> &str {
        "mock-embedding"
    }
}

// ---------------------------------------------------------------------------
// FastEmbed provider (local ONNX)
// ---------------------------------------------------------------------------

#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

/// Local embedding provider using fastembed-rs (ONNX Runtime).
///
/// Downloads and caches models on first use. Default model is
/// `BAAI/bge-small-en-v1.5` (384 dimensions). Thread-safe — the inner
/// `TextEmbedding` is `Send + Sync`.
#[cfg(feature = "fastembed")]
pub struct FastEmbedProvider {
    model: std::sync::Mutex<TextEmbedding>,
    model_name: String,
    dimension: usize,
}

#[cfg(feature = "fastembed")]
impl std::fmt::Debug for FastEmbedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FastEmbedProvider")
            .field("model_name", &self.model_name)
            .field("dimension", &self.dimension)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "fastembed")]
impl FastEmbedProvider {
    /// Create a provider with the given model name, or the default (`BGE-small-en-v1.5`).
    ///
    /// Model files are downloaded to the fastembed cache on first use.
    ///
    /// # Errors
    ///
    /// Returns [`EmbeddingError::InitFailed`] if model lookup or initialization fails.
    pub fn new(model_name: Option<&str>) -> EmbeddingResult<Self> {
        let embedding_model = match model_name {
            Some(name) => Self::resolve_model(name)?,
            None => EmbeddingModel::BGESmallENV15,
        };

        let model_info = TextEmbedding::get_model_info(&embedding_model).map_err(|e| {
            InitFailedSnafu {
                message: format!("failed to get model info: {e}"),
            }
            .build()
        })?;

        let dimension = model_info.dim;
        let code = model_info.model_code.clone();

        let options = InitOptions::new(embedding_model).with_show_download_progress(false);

        let model = TextEmbedding::try_new(options).map_err(|e| {
            InitFailedSnafu {
                message: format!("fastembed init failed: {e}"),
            }
            .build()
        })?;

        Ok(Self {
            model: std::sync::Mutex::new(model),
            model_name: code,
            dimension,
        })
    }

    fn resolve_model(name: &str) -> EmbeddingResult<EmbeddingModel> {
        TextEmbedding::list_supported_models()
            .into_iter()
            .find(|info| info.model_code == name)
            .map(|info| info.model)
            .ok_or_else(|| {
                InitFailedSnafu {
                    message: format!("unknown fastembed model: {name}"),
                }
                .build()
            })
    }
}

#[cfg(feature = "fastembed")]
impl EmbeddingProvider for FastEmbedProvider {
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        self.model
            .lock()
            .expect("fastembed model lock") // INVARIANT: lock held only for embed call, poisoned = prior panic
            .embed(vec![text], None)
            .map_err(|e| {
                EmbedFailedSnafu {
                    message: format!("{e}"),
                }
                .build()
            })?
            .into_iter()
            .next()
            .ok_or_else(|| {
                EmbedFailedSnafu {
                    message: "fastembed returned empty result".to_owned(),
                }
                .build()
            })
    }

    fn embed_batch(&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>> {
        self.model
            .lock()
            .expect("fastembed model lock") // INVARIANT: lock held only for embed call, poisoned = prior panic
            .embed(texts, None)
            .map_err(|e| {
                EmbedFailedSnafu {
                    message: format!("{e}"),
                }
                .build()
            })
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

/// Embedding provider configuration.
///
/// Available providers:
/// - `"mock"` — deterministic hash-based vectors for testing (always available)
/// - `"fastembed"` — local ONNX-based embeddings via fastembed-rs (requires `fastembed` feature)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmbeddingConfig {
    /// Provider type: `mock`, `fastembed`, `voyage`.
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
pub fn create_provider(config: &EmbeddingConfig) -> EmbeddingResult<Box<dyn EmbeddingProvider>> {
    match config.provider.as_str() {
        "mock" => {
            let dim = config.dimension.unwrap_or(384);
            Ok(Box::new(MockEmbeddingProvider::new(dim)))
        }
        #[cfg(feature = "fastembed")]
        "fastembed" => {
            let model = config.model.as_deref();
            Ok(Box::new(FastEmbedProvider::new(model)?))
        }
        #[cfg(not(feature = "fastembed"))]
        "fastembed" => InitFailedSnafu {
            message: "fastembed feature not enabled — build with --features fastembed".to_owned(),
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

    #[cfg(not(feature = "fastembed"))]
    #[test]
    fn fastembed_not_enabled_returns_error() {
        let config = EmbeddingConfig {
            provider: "fastembed".to_owned(),
            ..EmbeddingConfig::default()
        };
        let Err(err) = create_provider(&config) else {
            panic!("expected error for disabled fastembed feature");
        };
        let msg = err.to_string();
        assert!(
            msg.contains("not enabled"),
            "expected 'not enabled' in error, got: {msg}"
        );
    }

    #[cfg(feature = "fastembed")]
    mod fastembed_tests {
        use super::*;
        use std::sync::LazyLock;

        static PROVIDER: LazyLock<FastEmbedProvider> =
            LazyLock::new(|| FastEmbedProvider::new(None).expect("fastembed provider init"));

        #[test]
        fn fastembed_provider_initializes() {
            assert_eq!(PROVIDER.dimension(), 384);
        }

        #[test]
        fn fastembed_embed_produces_correct_dimension() {
            let vec = PROVIDER.embed("hello world").unwrap();
            assert_eq!(vec.len(), 384);
        }

        #[test]
        fn fastembed_embed_is_normalized() {
            let vec = PROVIDER.embed("normalize me").unwrap();
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 0.01, "expected unit norm, got {norm}");
        }

        #[test]
        fn fastembed_embed_deterministic() {
            let v1 = PROVIDER.embed("test input").unwrap();
            let v2 = PROVIDER.embed("test input").unwrap();
            assert_eq!(v1, v2);
        }

        #[test]
        fn fastembed_different_texts_differ() {
            let v1 = PROVIDER.embed("hello").unwrap();
            let v2 = PROVIDER.embed("world").unwrap();
            assert_ne!(v1, v2);
        }

        #[test]
        fn fastembed_batch_matches_individual() {
            let texts = ["hello", "world", "test"];
            let batch = PROVIDER.embed_batch(&texts).unwrap();
            for (i, text) in texts.iter().enumerate() {
                let individual = PROVIDER.embed(text).unwrap();
                assert_eq!(batch[i], individual);
            }
        }

        #[test]
        fn fastembed_provider_send_sync() {
            fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<FastEmbedProvider>();
        }
    }
}
