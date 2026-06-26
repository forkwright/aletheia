//! `OpenAI`-compatible HTTP embedding provider.
//!
//! Talks to any endpoint that exposes the `OpenAI` `/v1/embeddings`
//! surface — `OpenAI` itself, llama.cpp `--server`, or any compatible proxy.
//! Enabled by the `openai-embed` Cargo feature.
//!
//! # Local llama-server
//!
//! Point `base_url` at the local OpenAI-compatible embedding endpoint, such as
//! `http://127.0.0.1:5005/v1` for a local Qwen3-Embedding-0.6B service.

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::{
    EmbedFailedSnafu, EmbeddingProvider, EmbeddingResult, InitFailedSnafu, ModelProvenance,
};

/// Configuration for an `OpenAI`-compatible embedding provider.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct OpenAiCompatConfig {
    /// Base URL for the target endpoint — typically ends in `/v1`. Example:
    /// `http://127.0.0.1:5005/v1` for a local llama.cpp server.
    pub base_url: String,
    /// Optional bearer token for authenticated endpoints. Loopback llama.cpp
    /// accepts any value (or no auth at all); `OpenAI` requires a real key.
    pub api_key: Option<koina::secret::SecretString>,
    /// Model ID to request from the endpoint.
    pub model: String,
    /// Expected output dimension. Used by [`EmbeddingProvider::dimension`].
    pub dimension: usize,
}

/// `OpenAI` `/v1/embeddings`-compatible embedding provider.
///
/// Holds an async `reqwest::Client`. The sync [`EmbeddingProvider`] trait
/// methods bridge to the async HTTP call with `tokio::task::block_in_place`,
/// which yields the scheduler slot to other tasks while the request is in
/// flight. Callers should invoke this provider from a Tokio runtime context
/// (the normal Aletheia recall path); a short-lived fallback runtime is created
/// only for legacy synchronous callers that do not already have one.
pub struct OpenAiEmbeddingProvider {
    client: Client,
    base_url: String,
    api_key: Option<koina::secret::SecretString>,
    model: String,
    dimension: usize,
    provenance: ModelProvenance,
}

impl std::fmt::Debug for OpenAiEmbeddingProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiEmbeddingProvider")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("dimension", &self.dimension)
            .field("authenticated", &self.api_key.is_some())
            .field("provenance", &self.provenance)
            .finish_non_exhaustive()
    }
}

/// Wire request body for `/v1/embeddings`.
#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

/// Single embedding entry in the response.
#[derive(Debug, Deserialize)]
struct EmbeddingEntry {
    embedding: Vec<f32>,
    index: usize,
}

/// Wire response body from `/v1/embeddings`.
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingEntry>,
    model: String,
}

impl OpenAiEmbeddingProvider {
    /// Create a provider from the given config.
    ///
    /// # Errors
    ///
    /// Returns `EmbeddingError::InitFailed` if the HTTP client cannot be built.
    #[instrument]
    pub fn new(config: &OpenAiCompatConfig) -> EmbeddingResult<Self> {
        Self::with_provider("openai-compat", config)
    }

    /// Create a provider from the given config with an explicit provider name.
    ///
    /// `provider` is recorded in [`EmbeddingProvider::provenance`] and should be
    /// `openai-compat` or `voyage` for the corresponding configuration.
    ///
    /// # Errors
    ///
    /// Returns `EmbeddingError::InitFailed` if the HTTP client cannot be built.
    #[instrument]
    pub fn with_provider(provider: &str, config: &OpenAiCompatConfig) -> EmbeddingResult<Self> {
        // WHY: reqwest with rustls-no-provider needs an explicit crypto provider.
        // This is idempotent — subsequent calls silently fail and are ignored.
        // kanon:ignore RUST/no-silent-result-swallow — install_default() returns Err once the provider is already installed; that's the intended steady state, not an error.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_mins(1))
            .build()
            .map_err(|e| {
                InitFailedSnafu {
                    message: format!("failed to build HTTP client: {e}"),
                }
                .build()
            })?;

        let base_url = config.base_url.trim_end_matches('/').to_owned();
        Ok(Self {
            client,
            base_url: base_url.clone(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            dimension: config.dimension,
            provenance: ModelProvenance {
                provider: provider.to_owned(),
                model: Some(config.model.clone()),
                base_url: Some(base_url),
                dimension: Some(config.dimension),
            },
        })
    }

    /// Build a synchronous `reqwest::Request` for the given texts.
    fn build_request(&self, texts: &[String]) -> EmbeddingResult<reqwest::Request> {
        let url = format!("{}/embeddings", self.base_url);
        let body = EmbeddingRequest {
            model: &self.model,
            input: texts,
        };

        let mut request = self
            .client
            .post(&url)
            .json(&body)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .build()
            .map_err(|e| {
                EmbedFailedSnafu {
                    message: format!("failed to build request: {e}"),
                }
                .build()
            })?;

        if let Some(key) = &self.api_key {
            let value =
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", key.expose_secret()))
                    .map_err(|e| {
                        EmbedFailedSnafu {
                            message: format!("invalid API key for header: {e}"),
                        }
                        .build()
                    })?;
            request
                .headers_mut()
                .insert(reqwest::header::AUTHORIZATION, value);
        }

        Ok(request)
    }

    /// WHY: `block_in_place` yields the async worker thread to other tasks while
    /// the HTTP request is outstanding. The fallback runtime is only used when
    /// the provider is called from a thread that is not already managed by a
    /// Tokio runtime (legacy synchronous entry points or tests).
    fn block_on_request<F, R>(&self, fut: F) -> EmbeddingResult<R>
    where
        F: std::future::Future<Output = EmbeddingResult<R>> + Send + 'static,
        R: Send,
    {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(move || handle.block_on(fut)),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        InitFailedSnafu {
                            message: format!("failed to build temporary Tokio runtime: {e}"),
                        }
                        .build()
                    })?;
                rt.block_on(fut)
            }
        }
    }

    fn embed_inner(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let request = self.build_request(texts)?;
        let client = self.client.clone();
        let expected_model = self.model.clone();
        let batch_size = texts.len();

        let start = std::time::Instant::now();
        let result = self.block_on_request(execute_embedding_request(
            client,
            request,
            expected_model,
            batch_size,
        ));
        crate::metrics::record_embedding_duration(&self.model, start.elapsed().as_secs_f64());
        result
    }
}

/// Execute a prepared embedding request and parse the response.
async fn execute_embedding_request(
    client: Client,
    request: reqwest::Request,
    expected_model: String,
    batch_size: usize,
) -> EmbeddingResult<Vec<Vec<f32>>> {
    let response = client.execute(request).await.map_err(|e| {
        EmbedFailedSnafu {
            message: format!("HTTP request failed: {e}"),
        }
        .build()
    })?;

    let status = response.status().as_u16();
    if !response.status().is_success() {
        // kanon:ignore RUST/no-result-unwrap-or-default — we are already on the error path; empty body is an acceptable degraded message when the response body cannot be read.
        let body = response.text().await.unwrap_or_default();
        // WHY: bounded slice so we do not paste a multi-megabyte HTML
        // error page into the error chain.
        let trimmed: String = body.chars().take(512).collect();
        return EmbedFailedSnafu {
            message: format!("embedding API returned HTTP {status}: {trimmed}"),
        }
        .fail();
    }

    let parsed: EmbeddingResponse = response.json().await.map_err(|e| {
        EmbedFailedSnafu {
            message: format!("failed to parse embedding response: {e}"),
        }
        .build()
    })?;

    // WHY: model drift check — a misconfigured endpoint or server-side model
    // substitution corrupts embedding indexes that assume a fixed embedding space.
    if parsed.model != expected_model {
        tracing::warn!(
            configured_model = %expected_model,
            returned_model = %parsed.model,
            "embedding provider returned a different model than configured; embedding space may be inconsistent"
        );
    }

    let mut results = vec![Vec::new(); batch_size];
    for entry in parsed.data {
        if entry.index >= batch_size {
            return EmbedFailedSnafu {
                message: format!(
                    "embedding response index {} out of bounds (batch size {})",
                    entry.index, batch_size
                ),
            }
            .fail();
        }
        match results.get_mut(entry.index) {
            Some(slot) => *slot = entry.embedding,
            None => {
                return EmbedFailedSnafu {
                    message: format!("missing embedding for index {}", entry.index),
                }
                .fail();
            }
        }
    }

    for (i, result) in results.iter().enumerate() {
        if result.is_empty() {
            return EmbedFailedSnafu {
                message: format!("missing embedding for index {i}"),
            }
            .fail();
        }
    }

    Ok(results)
}

impl EmbeddingProvider for OpenAiEmbeddingProvider {
    #[instrument(skip(self, text))]
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        let mut results = self.embed_inner(&[text.to_owned()])?;
        results.pop().ok_or_else(|| {
            EmbedFailedSnafu {
                message: "OpenAI embedding returned empty result".to_owned(),
            }
            .build()
        })
    }

    #[instrument(skip(self, texts), fields(batch_size = texts.len()))]
    fn embed_batch(&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>> {
        let owned: Vec<String> = texts.iter().map(|s| (*s).to_owned()).collect();
        self.embed_inner(&owned)
    }

    #[instrument(skip(self))]
    fn dimension(&self) -> usize {
        self.dimension
    }

    #[instrument(skip(self))]
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provenance(&self) -> ModelProvenance {
        self.provenance.clone()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices asserted valid by construction"
)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn mock_provider(server: &MockServer) -> OpenAiEmbeddingProvider {
        OpenAiEmbeddingProvider::new(&OpenAiCompatConfig {
            base_url: format!("{}/v1", server.uri()),
            api_key: None,
            model: "qwen-embed".to_owned(),
            dimension: 384,
        })
        .expect("mock provider construct")
    }

    fn embedding_response(vectors: Vec<Vec<f32>>) -> serde_json::Value {
        let data: Vec<serde_json::Value> = vectors
            .into_iter()
            .enumerate()
            .map(|(i, vec)| {
                serde_json::json!({
                    "object": "embedding",
                    "embedding": vec,
                    "index": i
                })
            })
            .collect();
        serde_json::json!({
            "object": "list",
            "data": data,
            "model": "qwen-embed",
            "usage": { "prompt_tokens": 10, "total_tokens": 10 }
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn parse_embedding_response_single_vec() {
        let server = MockServer::start().await;
        let expected = vec![0.1_f32, 0.2, 0.3];
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(embedding_response(vec![expected.clone()])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let provider = mock_provider(&server);
        let result = provider.embed("hello").expect("embed should succeed");
        assert_eq!(result, vec![0.1_f32, 0.2, 0.3]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn parse_embedding_response_batch() {
        let server = MockServer::start().await;
        let v1 = vec![1.0_f32, 2.0, 3.0];
        let v2 = vec![4.0_f32, 5.0, 6.0];
        let v3 = vec![7.0_f32, 8.0, 9.0];

        // Return out of order to verify index-based reconstruction.
        let response = serde_json::json!({
            "object": "list",
            "data": [
                { "object": "embedding", "embedding": v2.clone(), "index": 1 },
                { "object": "embedding", "embedding": v3.clone(), "index": 2 },
                { "object": "embedding", "embedding": v1.clone(), "index": 0 }
            ],
            "model": "qwen-embed",
            "usage": { "prompt_tokens": 10, "total_tokens": 10 }
        });

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;

        let provider = mock_provider(&server);
        let result = provider
            .embed_batch(&["first", "second", "third"])
            .expect("batch embed should succeed");

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], vec![1.0, 2.0, 3.0]);
        assert_eq!(result[1], vec![4.0, 5.0, 6.0]);
        assert_eq!(result[2], vec![7.0, 8.0, 9.0]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embed_error_on_non_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
            .expect(1)
            .mount(&server)
            .await;

        let provider = mock_provider(&server);
        let result = provider.embed("hello");
        let err = result.expect_err("should error on 500");
        let msg = err.to_string();
        assert!(msg.contains("500"), "error should contain status: {msg}");
        assert!(
            msg.contains("internal server error"),
            "error should contain body: {msg}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_input_returns_empty_vec_without_http() {
        // No mock mounted — if we made an HTTP request it would fail.
        let server = MockServer::start().await;

        let provider = mock_provider(&server);
        let result = provider
            .embed_batch(&[])
            .expect("empty batch should succeed");
        assert!(result.is_empty(), "empty input should return empty vec");
    }
}
