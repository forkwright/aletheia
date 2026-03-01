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
            let h = hash.wrapping_mul(i as u64 + 1).wrapping_add(i as u64 * 2_654_435_761);
            #[allow(clippy::cast_precision_loss)]
            { *v = ((h % 10000) as f32 / 5000.0) - 1.0; }
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

    #[allow(clippy::unnecessary_literal_bound)]
    fn model_name(&self) -> &str {
        "mock-embedding"
    }
}

/// Embedding provider configuration.
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
        // "fastembed" => { ... } // M1.3 Phase 2: fastembed-rs integration
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
}
