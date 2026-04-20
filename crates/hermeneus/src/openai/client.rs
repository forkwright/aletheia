//! OpenAI-compatible HTTP client implementing [`LlmProvider`].
//!
//! Talks to any endpoint that exposes the OpenAI `/v1/chat/completions`
//! surface — OpenAI itself, llama.cpp `--server`, ollama, vllm, or any
//! compatible proxy. The wire translation lives in [`super::wire`]; this
//! module is the transport, retry, and registration shell.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use tracing::{Instrument as _, info, info_span};

use koina::secret::SecretString;

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::health::{HealthConfig, ProviderHealthTracker};
use crate::provider::LlmProvider;
use crate::types::{CompletionRequest, CompletionResponse};

use super::error::{map_error_response, map_request_error};
use super::wire::{ChatCompletionRequest, ChatCompletionResponse, parse_sse_response};

/// Build an HTTP client with sensible timeouts shared by Anthropic and
/// OpenAI-compatible providers.
fn build_http_client() -> Result<Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_mins(2))
        .build()
        .map_err(|e| {
            error::ProviderInitSnafu {
                message: format!("failed to build HTTP client: {e}"),
            }
            .build()
        })
}

/// Configuration for an OpenAI-compatible provider instance.
#[derive(Debug, Clone)]
pub struct OpenAiProviderConfig {
    /// Operator-facing label used for logs, metrics, and `name()`.
    pub name: String,
    /// Base URL for the target endpoint — typically ends in `/v1`. Example:
    /// `http://127.0.0.1:8088/v1` for a local llama.cpp server. TLS is
    /// required unless the URL is loopback.
    pub base_url: String,
    /// Optional bearer token for authenticated endpoints. Loopback llama.cpp
    /// and ollama accept any value (or no auth at all); OpenAI requires a
    /// real key.
    pub api_key: Option<SecretString>,
    /// Model IDs this provider advertises support for. Determines routing
    /// in the [`ProviderRegistry`](crate::provider::ProviderRegistry).
    pub models: Vec<String>,
    /// Per-request timeout override. Defaults to 2 minutes (matches
    /// Anthropic's non-streaming default).
    pub request_timeout: Duration,
    /// Maximum retries on transient failures (5xx, timeout, connection
    /// reset). Defaults to 3.
    pub max_retries: u32,
}

impl Default for OpenAiProviderConfig {
    fn default() -> Self {
        Self {
            name: "openai-compatible".to_owned(),
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key: None,
            models: Vec::new(),
            request_timeout: Duration::from_mins(2),
            max_retries: 3,
        }
    }
}

/// Returns true when the URL is safe to use without TLS.
///
/// WHY: delegates to `koina::http::is_plaintext_loopback_url` so the
/// plaintext HTTP scheme literal lives in exactly one audited place
/// (see `SECURITY/insecure-transport`).
fn is_loopback_url(url: &str) -> bool {
    koina::http::is_plaintext_loopback_url(url)
}

/// OpenAI Chat Completions-compatible LLM provider.
pub struct OpenAiProvider {
    client: Client,
    config: OpenAiProviderConfig,
    /// Owned `&'static str` slice of model IDs for [`LlmProvider::supported_models`].
    /// Leaked once at construction — the provider lives for the server lifetime.
    model_refs: &'static [&'static str],
    health: Arc<ProviderHealthTracker>,
}

impl OpenAiProvider {
    /// Construct a provider from the given config.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::ProviderInit`] if the HTTP client cannot be
    /// built or the base URL is non-loopback HTTP (credentials would be
    /// sent in cleartext).
    pub fn new(config: OpenAiProviderConfig) -> Result<Self> {
        if !config.base_url.starts_with("https://") && !is_loopback_url(&config.base_url) {
            return Err(error::ProviderInitSnafu {
                message: format!(
                    "OpenAI-compatible base URL must use HTTPS or loopback (got {:?})",
                    config.base_url
                ),
            }
            .build());
        }

        let client = build_http_client()?;
        let model_refs = leak_models(&config.models);

        info!(
            provider = %config.name,
            base_url = %config.base_url,
            models = ?config.models,
            authenticated = config.api_key.is_some(),
            "OpenAI-compatible provider initialized"
        );

        Ok(Self {
            client,
            config,
            model_refs,
            health: Arc::new(ProviderHealthTracker::new(HealthConfig::default())),
        })
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("accept", HeaderValue::from_static("application/json"));

        if let Some(key) = &self.config.api_key {
            let value = HeaderValue::from_str(&format!("Bearer {}", key.expose_secret())).map_err(
                |_e| {
                    error::AuthFailedSnafu {
                        message: "API key contains invalid header characters".to_owned(),
                    }
                    .build()
                },
            )?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        Ok(headers)
    }

    fn credential_source(&self) -> &'static str {
        if self.config.api_key.is_some() {
            "api-key"
        } else {
            "none"
        }
    }

    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let span = info_span!("llm_call",
            llm.provider = %self.config.name,
            llm.model = %request.model,
            llm.duration_ms = tracing::field::Empty,
            llm.tokens_in = tracing::field::Empty,
            llm.tokens_out = tracing::field::Empty,
            llm.status = tracing::field::Empty,
            llm.retries = tracing::field::Empty,
            llm.stream = false,
        );
        self.execute_inner(request).instrument(span).await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "retry loop with span recording and metric emission at each exit point"
    )]
    async fn execute_inner(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        if let Err(health) = self.health.check_available() {
            return Err(error::ApiRequestSnafu {
                message: format!("provider circuit-breaker open: {health:?}"),
            }
            .build());
        }

        let start = Instant::now();
        let wire = ChatCompletionRequest::from_request(request, None)?;
        let body = serde_json::to_string(&wire).map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to serialize request: {e}"),
            }
            .build()
        })?;

        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let mut last_error: Option<error::Error> = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(backoff_delay(attempt)).await;
            }
            let headers = self.build_headers()?;

            let response = match self
                .client
                .post(&url)
                .headers(headers)
                .body(body.clone())
                .timeout(self.config.request_timeout)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let err = map_request_error(&e);
                    self.health.record_error(&err);
                    if !err.is_retryable() {
                        return Err(err);
                    }
                    last_error = Some(err);
                    continue;
                }
            };

            let status = response.status().as_u16();
            if response.status().is_success() {
                let text = response.text().await.map_err(|e| {
                    error::ApiRequestSnafu {
                        message: format!("failed to read response body: {e}"),
                    }
                    .build()
                })?;
                let parsed: ChatCompletionResponse = serde_json::from_str(&text).map_err(|e| {
                    error::ApiRequestSnafu {
                        message: format!("failed to parse OpenAI response: {e}"),
                    }
                    .build()
                })?;
                let resp = parsed
                    .into_response()
                    .map_err(|msg| error::ApiRequestSnafu { message: msg }.build())?;
                self.health.record_success();
                tracing::Span::current().record("llm.tokens_in", resp.usage.input_tokens);
                tracing::Span::current().record("llm.tokens_out", resp.usage.output_tokens);
                tracing::Span::current().record("llm.status", "ok");
                tracing::Span::current().record("llm.retries", attempt);
                info!(
                    provider = %self.config.name,
                    model = %request.model,
                    tokens_in = resp.usage.input_tokens,
                    tokens_out = resp.usage.output_tokens,
                    "OpenAI-compatible call complete"
                );
                crate::metrics::record_completion(
                    &self.config.name,
                    resp.usage.input_tokens,
                    resp.usage.output_tokens,
                    0.0,
                    true,
                );
                crate::metrics::record_latency(&request.model, "ok", start.elapsed().as_secs_f64());
                return Ok(resp);
            }

            let err = map_error_response(response, &request.model, self.credential_source()).await;
            self.health.record_error(&err);
            if !err.is_retryable() {
                tracing::Span::current().record("llm.retries", attempt);
                tracing::Span::current().record(
                    "llm.status",
                    if status == 401 {
                        "auth_failed"
                    } else {
                        "error"
                    },
                );
                return Err(err);
            }
            last_error = Some(err);
        }

        tracing::Span::current().record("llm.retries", self.config.max_retries);
        tracing::Span::current().record("llm.status", "error");
        crate::metrics::record_completion(&self.config.name, 0, 0, 0.0, false);
        crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());
        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "request failed after all retries".to_owned(),
            }
            .build()
        }))
    }

    async fn execute_streaming(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        if let Err(health) = self.health.check_available() {
            return Err(error::ApiRequestSnafu {
                message: format!("provider circuit-breaker open: {health:?}"),
            }
            .build());
        }

        let wire = ChatCompletionRequest::from_request(request, Some(true))?;
        let body = serde_json::to_string(&wire).map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to serialize request: {e}"),
            }
            .build()
        })?;
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let headers = self.build_headers()?;

        let mut response = self
            .client
            .post(&url)
            .headers(headers)
            .body(body)
            .timeout(Duration::from_mins(10))
            .send()
            .await
            .map_err(|e| map_request_error(&e))?;

        if !response.status().is_success() {
            return Err(
                map_error_response(response, &request.model, self.credential_source()).await,
            );
        }

        let resp = parse_sse_response(&mut response, on_event).await?;
        self.health.record_success();
        Ok(resp)
    }
}

/// Exponential backoff without jitter for the OpenAI retry loop.
fn backoff_delay(attempt: u32) -> Duration {
    let base_ms: u64 = 500;
    let cap_ms: u64 = 30_000;
    let delay = base_ms.saturating_mul(1_u64 << attempt.min(6));
    Duration::from_millis(delay.min(cap_ms))
}

/// Leak a `Vec<String>` of model IDs to a `&'static [&'static str]` slice.
///
/// Called once at provider construction so [`LlmProvider::supported_models`]
/// can return a borrowed static slice — the provider outlives every request
/// in normal operation, and leaking keeps the trait signature (`&[&str]`)
/// simple without forcing every caller into dynamic storage.
fn leak_models(models: &[String]) -> &'static [&'static str] {
    let leaked: Vec<&'static str> = models
        .iter()
        .map(|s| {
            let boxed: Box<str> = s.clone().into_boxed_str();
            let static_ref: &'static str = Box::leak(boxed);
            static_ref
        })
        .collect();
    Box::leak(leaked.into_boxed_slice())
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("name", &self.config.name)
            .field("base_url", &self.config.base_url)
            .field("models", &self.config.models)
            .field("authenticated", &self.config.api_key.is_some())
            .finish_non_exhaustive()
    }
}

impl LlmProvider for OpenAiProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute(request))
    }

    fn supported_models(&self) -> &[&str] {
        self.model_refs
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn complete_streaming<'a>(
        &'a self,
        request: &'a CompletionRequest,
        on_event: &'a mut (dyn FnMut(StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute_streaming(request, on_event))
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn rejects_plain_http_to_non_loopback() {
        let config = OpenAiProviderConfig {
            base_url: "http://evil.example.com/v1".to_owned(),
            ..Default::default()
        };
        let err = OpenAiProvider::new(config).unwrap_err();
        assert!(err.to_string().contains("HTTPS"));
    }

    #[test]
    fn accepts_loopback_http() {
        let config = OpenAiProviderConfig {
            name: "local".to_owned(),
            base_url: "http://127.0.0.1:8088/v1".to_owned(),
            models: vec!["qwen".to_owned()],
            ..Default::default()
        };
        let provider = OpenAiProvider::new(config).unwrap();
        assert_eq!(provider.name(), "local");
        assert!(provider.supports_model("qwen"));
    }

    #[test]
    fn accepts_https() {
        let config = OpenAiProviderConfig {
            name: "cloud".to_owned(),
            base_url: "https://api.openai.com/v1".to_owned(),
            models: vec!["gpt-4o".to_owned()],
            ..Default::default()
        };
        let provider = OpenAiProvider::new(config).unwrap();
        assert!(provider.supports_model("gpt-4o"));
        assert!(!provider.supports_model("nonexistent"));
    }
}
