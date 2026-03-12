//! Anthropic Messages API provider.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::SecretString;
use snafu::ResultExt;
use tracing::{info, info_span};

use std::collections::HashMap;

use crate::error::{self, Result};
use crate::provider::{LlmProvider, ModelPricing, ProviderConfig};
use crate::types::{CompletionRequest, CompletionResponse, TokenCount};
use aletheia_koina::credential::{CredentialProvider, CredentialSource};

use super::stream::{StreamAccumulator, StreamEvent, parse_sse_stream};
use super::wire::WireRequest;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_API_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_RETRIES: u32 = 3;

const BACKOFF_BASE_MS: u64 = 1000;
const BACKOFF_FACTOR: u64 = 2;
const BACKOFF_MAX_MS: u64 = 30_000;

static SUPPORTED_MODELS: &[&str] = &[
    "claude-opus-4-6",
    "claude-opus-4-20250514",
    "claude-sonnet-4-6",
    "claude-sonnet-4-20250514",
    "claude-haiku-4-5",
    "claude-haiku-4-5-20251001",
];

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    client: Client,
    credential_provider: Arc<dyn CredentialProvider>,
    base_url: String,
    api_version: String,
    max_retries: u32,
    pricing: HashMap<String, ModelPricing>,
}

/// Static credential provider for backward-compatible `from_config()`.
struct StaticCredentialProvider {
    key: SecretString,
}

impl CredentialProvider for StaticCredentialProvider {
    fn get_credential(&self) -> Option<aletheia_koina::credential::Credential> {
        use secrecy::ExposeSecret;
        Some(aletheia_koina::credential::Credential {
            secret: self.key.expose_secret().to_owned(),
            source: CredentialSource::Environment,
        })
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait requires &str return"
    )]
    fn name(&self) -> &str {
        "static"
    }
}

fn build_http_client() -> Result<Client> {
    // reqwest 0.13 with rustls-no-provider requires an explicit crypto provider.
    // install_default() is idempotent — subsequent calls return Err and are ignored.
    let _ = rustls::crypto::ring::default_provider().install_default();

    Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| {
            error::ProviderInitSnafu {
                message: format!("failed to build HTTP client: {e}"),
            }
            .build()
        })
}

impl AnthropicProvider {
    /// Create a provider from configuration with a static API key.
    ///
    /// # Errors
    /// Returns `ProviderInit` if `api_key` is missing.
    pub fn from_config(config: &ProviderConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_ref()
            .filter(|k| !k.is_empty())
            .ok_or_else(|| {
                error::ProviderInitSnafu {
                    message: "api_key is required for Anthropic provider".to_owned(),
                }
                .build()
            })?;

        Ok(Self {
            client: build_http_client()?,
            credential_provider: Arc::new(StaticCredentialProvider {
                key: SecretString::from(api_key.clone()),
            }),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            api_version: DEFAULT_API_VERSION.to_owned(),
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            pricing: config.pricing.clone(),
        })
    }

    /// Create a provider with a dynamic credential provider.
    ///
    /// The credential is resolved per-request via `provider.get_credential()`,
    /// enabling mid-session token rotation and background OAuth refresh.
    pub fn with_credential_provider(
        provider: Arc<dyn CredentialProvider>,
        config: &ProviderConfig,
    ) -> Result<Self> {
        Ok(Self {
            client: build_http_client()?,
            credential_provider: provider,
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            api_version: DEFAULT_API_VERSION.to_owned(),
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            pricing: config.pricing.clone(),
        })
    }

    /// Streaming completion — accumulates into a final `CompletionResponse`
    /// while emitting deltas to the callback.
    ///
    /// Retries on transient errors (overloaded, rate-limited) with exponential
    /// backoff, but **only if no content has been emitted** to the callback yet.
    /// Once deltas have been streamed, a retry would produce duplicate/corrupt
    /// output, so mid-content errors propagate immediately.
    ///
    /// This is an `AnthropicProvider`-specific method. The `LlmProvider`
    /// trait only exposes `complete()`.
    #[expect(
        clippy::too_many_lines,
        reason = "streaming retry loop with span recording at each exit point"
    )]
    pub async fn complete_streaming(
        &self,
        request: &CompletionRequest,
        mut on_event: impl FnMut(StreamEvent),
    ) -> Result<CompletionResponse> {
        let span = info_span!("llm_call",
            llm.provider = "anthropic",
            llm.model = %request.model,
            llm.duration_ms = tracing::field::Empty,
            llm.tokens_in = tracing::field::Empty,
            llm.tokens_out = tracing::field::Empty,
            llm.status = tracing::field::Empty,
            llm.retries = tracing::field::Empty,
            llm.stream = true,
        );
        let _guard = span.enter();
        let start = Instant::now();

        let wire = WireRequest::from_request(request, Some(true));
        let body = serde_json::to_string(&wire).context(error::ParseResponseSnafu)?;

        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tracing::warn!(
                    attempt,
                    max = self.max_retries,
                    "retrying streaming request after transient error"
                );
                tokio::time::sleep(backoff_delay(attempt, last_error.as_ref())).await;
            }

            let headers = self.build_headers()?;

            // HTTP-level errors (connection, non-200 status)
            let response = match self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers)
                .body(body.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(super::error::map_request_error(&e));
                    continue;
                }
            };

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let err = super::error::map_error_response(response).await;
                // Non-retryable HTTP status: 401, 400-level (except 429)
                if status == 401 || ((400..500).contains(&status) && status != 429) {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "LLM call duration fits in u64"
                    )]
                    {
                        span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
                    }
                    span.record("llm.retries", attempt);
                    if status == 401 {
                        span.record("llm.status", "auth_failed");
                    } else {
                        span.record("llm.status", "error");
                    }
                    return Err(err);
                }
                last_error = Some(err);
                continue;
            }

            // Read the full response body and parse SSE from it
            let response_bytes = response.bytes().await.map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!("failed to read streaming response body: {e}"),
                }
                .build()
            })?;
            let reader = std::io::Cursor::new(response_bytes);
            let mut accumulator = StreamAccumulator::new();
            let mut content_started = false;

            let stream_result = parse_sse_stream(reader, &mut accumulator, &mut |event| {
                if matches!(
                    event,
                    StreamEvent::TextDelta { .. }
                        | StreamEvent::ThinkingDelta { .. }
                        | StreamEvent::InputJsonDelta { .. }
                ) {
                    content_started = true;
                }
                on_event(event);
            });

            match stream_result {
                Ok(()) => {
                    let resp = accumulator.finish();
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "LLM call duration fits in u64"
                    )]
                    {
                        span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
                    }
                    span.record("llm.tokens_in", resp.usage.input_tokens);
                    span.record("llm.tokens_out", resp.usage.output_tokens);
                    span.record("llm.status", "ok");
                    span.record("llm.retries", attempt);
                    info!(
                        model = %request.model,
                        tokens_in = resp.usage.input_tokens,
                        tokens_out = resp.usage.output_tokens,
                        cost = %format!("~${:.4}", estimate_cost(&self.pricing, &request.model, resp.usage.input_tokens, resp.usage.output_tokens)),
                        "LLM call complete"
                    );
                    crate::metrics::record_completion(
                        "anthropic",
                        resp.usage.input_tokens,
                        resp.usage.output_tokens,
                        estimate_cost(
                            &self.pricing,
                            &request.model,
                            resp.usage.input_tokens,
                            resp.usage.output_tokens,
                        ),
                        true,
                    );
                    crate::metrics::record_cache_tokens(
                        "anthropic",
                        resp.usage.cache_read_tokens,
                        resp.usage.cache_write_tokens,
                    );
                    return Ok(resp);
                }
                Err(e) => {
                    // If content was already streamed, we can't retry — it would
                    // produce duplicates. Propagate immediately.
                    if content_started {
                        tracing::error!("SSE error after content started streaming — cannot retry");
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "LLM call duration fits in u64"
                        )]
                        {
                            span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
                        }
                        span.record("llm.retries", attempt);
                        span.record("llm.status", "error");
                        return Err(e);
                    }
                    // Only retry RateLimited (overloaded/429); other errors are terminal.
                    if matches!(e, error::Error::RateLimited { .. }) {
                        tracing::warn!("SSE stream returned retryable error before content");
                        last_error = Some(e);
                        continue;
                    }
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "LLM call duration fits in u64"
                    )]
                    {
                        span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
                    }
                    span.record("llm.retries", attempt);
                    span.record("llm.status", "error");
                    return Err(e);
                }
            }
        }

        #[expect(
            clippy::cast_possible_truncation,
            reason = "LLM call duration fits in u64"
        )]
        {
            span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
        }
        span.record("llm.retries", self.max_retries);
        span.record("llm.status", "error");

        crate::metrics::record_completion("anthropic", 0, 0, 0.0, false);

        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "streaming request failed after all retries".to_owned(),
            }
            .build()
        }))
    }

    /// Count tokens for a request via the Anthropic `count_tokens` endpoint.
    pub async fn count_tokens_request(&self, request: &CompletionRequest) -> Result<TokenCount> {
        #[derive(serde::Deserialize)]
        struct CountResponse {
            input_tokens: u64,
        }

        let wire = WireRequest::from_request(request, None);
        let body = serde_json::to_string(&wire).context(error::ParseResponseSnafu)?;
        let headers = self.build_headers()?;

        let response = self
            .client
            .post(format!("{}/v1/messages/count_tokens", self.base_url))
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!("count_tokens request failed: {e}"),
                }
                .build()
            })?;

        if !response.status().is_success() {
            return Err(super::error::map_error_response(response).await);
        }

        let text = response.text().await.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to read count_tokens response: {e}"),
            }
            .build()
        })?;

        let parsed: CountResponse =
            serde_json::from_str(&text).context(error::ParseResponseSnafu)?;
        Ok(TokenCount {
            input_tokens: parsed.input_tokens,
        })
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let credential = self.credential_provider.get_credential().ok_or_else(|| {
            error::AuthFailedSnafu {
                message: "no credential available from provider".to_owned(),
            }
            .build()
        })?;

        if credential.secret.is_empty() {
            return Err(error::AuthFailedSnafu {
                message: "credential secret is empty — cannot build Authorization header"
                    .to_owned(),
            }
            .build());
        }

        let mut headers = HeaderMap::new();
        if credential.source == CredentialSource::OAuth {
            let value =
                HeaderValue::from_str(&format!("Bearer {}", credential.secret)).map_err(|_e| {
                    error::AuthFailedSnafu {
                        message: "credential contains invalid header characters".to_owned(),
                    }
                    .build()
                })?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
            headers.insert(
                "anthropic-beta",
                HeaderValue::from_static("oauth-2025-04-20"),
            );
        } else {
            let value = HeaderValue::from_str(&credential.secret).map_err(|_e| {
                error::AuthFailedSnafu {
                    message: "API key contains invalid header characters".to_owned(),
                }
                .build()
            })?;
            headers.insert("x-api-key", value);
        }
        headers.insert(
            "anthropic-version",
            HeaderValue::from_str(&self.api_version)
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_API_VERSION)),
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "retry loop with span recording at each exit point"
    )]
    async fn execute_with_retry(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let span = info_span!("llm_call",
            llm.provider = "anthropic",
            llm.model = %request.model,
            llm.duration_ms = tracing::field::Empty,
            llm.tokens_in = tracing::field::Empty,
            llm.tokens_out = tracing::field::Empty,
            llm.status = tracing::field::Empty,
            llm.retries = tracing::field::Empty,
            llm.stream = false,
        );
        let _guard = span.enter();
        let start = Instant::now();

        let wire = WireRequest::from_request(request, None);
        let body = serde_json::to_string(&wire).context(error::ParseResponseSnafu)?;

        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(backoff_delay(attempt, last_error.as_ref())).await;
            }

            let headers = self.build_headers()?;

            let response = match self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers)
                .body(body.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(super::error::map_request_error(&e));
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
                let parsed = super::error::parse_response_body::<super::wire::WireResponse>(&text)
                    .and_then(|r| {
                        r.into_response()
                            .map_err(|msg| error::ApiRequestSnafu { message: msg }.build())
                    });
                if let Ok(ref resp) = parsed {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "LLM call duration fits in u64"
                    )]
                    {
                        span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
                    }
                    span.record("llm.tokens_in", resp.usage.input_tokens);
                    span.record("llm.tokens_out", resp.usage.output_tokens);
                    span.record("llm.status", "ok");
                    span.record("llm.retries", attempt);
                    info!(
                        model = %request.model,
                        tokens_in = resp.usage.input_tokens,
                        tokens_out = resp.usage.output_tokens,
                        cost = %format!("~${:.4}", estimate_cost(&self.pricing, &request.model, resp.usage.input_tokens, resp.usage.output_tokens)),
                        "LLM call complete"
                    );
                    crate::metrics::record_completion(
                        "anthropic",
                        resp.usage.input_tokens,
                        resp.usage.output_tokens,
                        estimate_cost(
                            &self.pricing,
                            &request.model,
                            resp.usage.input_tokens,
                            resp.usage.output_tokens,
                        ),
                        true,
                    );
                    crate::metrics::record_cache_tokens(
                        "anthropic",
                        resp.usage.cache_read_tokens,
                        resp.usage.cache_write_tokens,
                    );
                }
                return parsed;
            }

            let err = super::error::map_error_response(response).await;

            // Non-retryable: 401, 400-level (except 429).
            if status == 401 || ((400..500).contains(&status) && status != 429) {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "LLM call duration fits in u64"
                )]
                {
                    span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
                }
                span.record("llm.retries", attempt);
                if status == 401 {
                    span.record("llm.status", "auth_failed");
                } else if status == 429 {
                    span.record("llm.status", "rate_limited");
                } else {
                    span.record("llm.status", "error");
                }
                return Err(err);
            }

            // Retryable: 429, 5xx, network errors.
            last_error = Some(err);
        }

        #[expect(
            clippy::cast_possible_truncation,
            reason = "LLM call duration fits in u64"
        )]
        {
            span.record("llm.duration_ms", start.elapsed().as_millis() as u64);
        }
        span.record("llm.retries", self.max_retries);
        span.record("llm.status", "error");

        crate::metrics::record_completion("anthropic", 0, 0, 0.0, false);

        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "request failed after all retries".to_owned(),
            }
            .build()
        }))
    }
}

impl LlmProvider for AnthropicProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute_with_retry(request))
    }

    fn supported_models(&self) -> &[&str] {
        SUPPORTED_MODELS
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait requires &str return, static string is fine"
    )]
    fn name(&self) -> &str {
        "anthropic"
    }

    fn count_tokens<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TokenCount>>> + Send + 'a>> {
        Box::pin(async move { self.count_tokens_request(request).await.map(Some) })
    }

    fn supports_caching(&self) -> bool {
        true
    }

    fn supports_citations(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Estimate cost using config-based pricing, falling back to hardcoded defaults.
#[expect(
    clippy::cast_precision_loss,
    reason = "token counts are small enough for f64 precision"
)]
fn estimate_cost(
    pricing: &HashMap<String, ModelPricing>,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let (in_rate, out_rate) = if let Some(p) = pricing.get(model) {
        (p.input_cost_per_mtok, p.output_cost_per_mtok)
    } else if model.contains("opus") {
        (15.0, 75.0)
    } else if model.contains("haiku") {
        (0.80, 4.0)
    } else {
        (3.0, 15.0)
    };
    (input_tokens as f64 * in_rate + output_tokens as f64 * out_rate) / 1_000_000.0
}

pub(crate) fn backoff_delay(attempt: u32, last_error: Option<&error::Error>) -> Duration {
    if let Some(error::Error::RateLimited { retry_after_ms, .. }) = last_error {
        return Duration::from_millis(*retry_after_ms);
    }

    let base = BACKOFF_BASE_MS * BACKOFF_FACTOR.pow(attempt.saturating_sub(1));
    let capped = base.min(BACKOFF_MAX_MS);

    // ±25% jitter via integer math: range = capped / 4
    let jitter_range = capped / 4;
    let delay = if jitter_range > 0 {
        let offset = (u64::from(attempt) * 7 + 13) % (jitter_range * 2);
        capped - jitter_range + offset
    } else {
        capped
    };

    Duration::from_millis(delay.max(100))
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("credential_provider", &self.credential_provider.name())
            .field("base_url", &self.base_url)
            .field("api_version", &self.api_version)
            .field("max_retries", &self.max_retries)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
