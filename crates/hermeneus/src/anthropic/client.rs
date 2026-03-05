//! Anthropic Messages API provider.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::SecretString;
use snafu::ResultExt;
use tracing::{info, info_span};

use std::collections::HashMap;

use aletheia_koina::credential::{CredentialProvider, CredentialSource};
use crate::error::{self, Result};
use crate::provider::{LlmProvider, ModelPricing, ProviderConfig};
use crate::types::{CompletionRequest, CompletionResponse};

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
    "claude-sonnet-4-20250514",
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

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str return")]
    fn name(&self) -> &str {
        "static"
    }
}

fn build_http_client() -> Result<Client> {
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
    /// This is an `AnthropicProvider`-specific method. The sync `LlmProvider`
    /// trait only exposes `complete()`. When the trait goes async in M2, this
    /// will become the primary implementation.
    #[expect(
        clippy::too_many_lines,
        reason = "streaming retry loop with span recording at each exit point"
    )]
    pub fn complete_streaming(
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
                std::thread::sleep(backoff_delay(attempt, last_error.as_ref()));
            }

            let headers = self.build_headers()?;

            // HTTP-level errors (connection, non-200 status)
            let response = match self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers)
                .body(body.clone())
                .send()
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(super::error::map_request_error(&e));
                    continue;
                }
            };

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let err = super::error::map_error_response(response);
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

            // SSE stream — track whether content has been emitted
            let reader = std::io::BufReader::new(response);
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
                        estimate_cost(&self.pricing, &request.model, resp.usage.input_tokens, resp.usage.output_tokens),
                        true,
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

    fn build_headers(&self) -> Result<HeaderMap> {
        let credential = self.credential_provider.get_credential().ok_or_else(|| {
            error::AuthFailedSnafu {
                message: "no credential available from provider".to_owned(),
            }
            .build()
        })?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&credential.secret)
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );
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
    fn execute_with_retry(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
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
                std::thread::sleep(backoff_delay(attempt, last_error.as_ref()));
            }

            let headers = self.build_headers()?;

            let response = match self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers)
                .body(body.clone())
                .send()
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(super::error::map_request_error(&e));
                    continue;
                }
            };

            let status = response.status().as_u16();

            if response.status().is_success() {
                let text = response.text().map_err(|e| {
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
                        estimate_cost(&self.pricing, &request.model, resp.usage.input_tokens, resp.usage.output_tokens),
                        true,
                    );
                }
                return parsed;
            }

            let err = super::error::map_error_response(response);

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
    fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        self.execute_with_retry(request)
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
mod tests {
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::error::Error;
    use crate::provider::{LlmProvider, ProviderConfig};
    use crate::types::{CompletionRequest, CompletionResponse, Content, Message, Role};

    /// Build a provider and call `complete()` on a blocking thread.
    ///
    /// `reqwest::blocking::Client` panics if constructed or used inside a tokio
    /// async context, so wiremock tests dispatch everything to `spawn_blocking`.
    async fn complete_on_blocking_thread(
        config: ProviderConfig,
        request: CompletionRequest,
    ) -> crate::error::Result<CompletionResponse> {
        tokio::task::spawn_blocking(move || {
            let provider = AnthropicProvider::from_config(&config)?;
            provider.complete(&request)
        })
        .await
        .expect("spawn_blocking join")
    }

    fn test_config_with(base_url: &str) -> ProviderConfig {
        ProviderConfig {
            provider_type: "anthropic".to_owned(),
            api_key: Some("test-key".to_owned()),
            base_url: Some(base_url.to_owned()),
            default_model: None,
            max_retries: Some(0),
            pricing: HashMap::new(),
        }
    }

    fn test_request() -> CompletionRequest {
        CompletionRequest {
            model: "claude-opus-4-20250514".to_owned(),
            system: None,
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
            }],
            max_tokens: 128,
            tools: vec![],
            temperature: None,
            thinking: None,
            stop_sequences: vec![],
        }
    }

    fn valid_wire_response_json() -> serde_json::Value {
        serde_json::json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello from test"}],
            "model": "claude-opus-4-20250514",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })
    }

    // --- from_config tests ---

    #[test]
    fn from_config_missing_api_key() {
        let config = ProviderConfig {
            api_key: None,
            ..ProviderConfig::default()
        };
        let err = AnthropicProvider::from_config(&config).expect_err("should fail without key");
        assert!(
            matches!(err, Error::ProviderInit { .. }),
            "expected ProviderInit, got: {err:?}"
        );
    }

    #[test]
    fn from_config_empty_api_key() {
        let config = ProviderConfig {
            api_key: Some(String::new()),
            ..ProviderConfig::default()
        };
        let err = AnthropicProvider::from_config(&config).expect_err("should fail with empty key");
        assert!(
            matches!(err, Error::ProviderInit { .. }),
            "expected ProviderInit, got: {err:?}"
        );
    }

    #[test]
    fn from_config_valid() {
        let config = ProviderConfig {
            api_key: Some("sk-test-123".to_owned()),
            base_url: Some("https://custom.api.example.com".to_owned()),
            ..ProviderConfig::default()
        };
        let provider = AnthropicProvider::from_config(&config).expect("valid config");
        let debug = format!("{provider:?}");
        assert!(
            debug.contains("custom.api.example.com"),
            "debug should show base_url: {debug}"
        );
    }

    // --- wiremock integration tests ---

    #[tokio::test]
    async fn complete_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config_with(&server.uri());
        let response = complete_on_blocking_thread(config, test_request())
            .await
            .expect("complete");
        assert_eq!(response.id, "msg_test");
        assert_eq!(response.stop_reason, crate::types::StopReason::EndTurn);
        assert_eq!(response.usage.input_tokens, 10);
    }

    #[tokio::test]
    async fn complete_auth_failure_not_retried() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "type": "error",
                "error": {"type": "authentication_error", "message": "invalid api key"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut config = test_config_with(&server.uri());
        config.max_retries = Some(2);
        let err = complete_on_blocking_thread(config, test_request())
            .await
            .expect_err("should fail");
        assert!(
            matches!(err, Error::AuthFailed { .. }),
            "expected AuthFailed, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn complete_bad_request_not_retried() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "type": "error",
                "error": {"type": "invalid_request_error", "message": "bad input"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut config = test_config_with(&server.uri());
        config.max_retries = Some(2);
        let err = complete_on_blocking_thread(config, test_request())
            .await
            .expect_err("should fail");
        assert!(
            matches!(err, Error::ApiError { status: 400, .. }),
            "expected ApiError 400, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn complete_rate_limited_no_retry() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "type": "error",
                "error": {"type": "rate_limit_error", "message": "rate limited"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config_with(&server.uri());
        let err = complete_on_blocking_thread(config, test_request())
            .await
            .expect_err("should fail");
        assert!(
            matches!(err, Error::RateLimited { .. }),
            "expected RateLimited, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn complete_server_error_no_retry() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config_with(&server.uri());
        let err = complete_on_blocking_thread(config, test_request())
            .await
            .expect_err("should fail");
        assert!(
            matches!(err, Error::ApiError { status: 500, .. }),
            "expected ApiError 500, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn complete_malformed_body() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config_with(&server.uri());
        let err = complete_on_blocking_thread(config, test_request())
            .await
            .expect_err("should fail");
        assert!(
            matches!(err, Error::ParseResponse { .. }),
            "expected ParseResponse, got: {err:?}"
        );
    }

    // --- estimate_cost unit tests ---

    #[test]
    fn estimate_cost_opus() {
        let pricing = HashMap::new();
        let cost = estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100);
        assert!((cost - 0.0225).abs() < 0.0001);
    }

    #[test]
    fn estimate_cost_sonnet() {
        let pricing = HashMap::new();
        let cost = estimate_cost(&pricing, "claude-sonnet-4-20250514", 1000, 100);
        assert!((cost - 0.0045).abs() < 0.0001);
    }

    #[test]
    fn estimate_cost_haiku() {
        let pricing = HashMap::new();
        let cost = estimate_cost(&pricing, "claude-haiku-4-5-20251001", 1000, 100);
        assert!((cost - 0.0012).abs() < 0.0001);
    }

    #[test]
    fn estimate_cost_unknown_defaults_to_sonnet() {
        let pricing = HashMap::new();
        let cost_unknown = estimate_cost(&pricing, "some-unknown-model", 1000, 100);
        let cost_sonnet = estimate_cost(&pricing, "claude-sonnet-4-20250514", 1000, 100);
        assert!((cost_unknown - cost_sonnet).abs() < 0.0001);
    }

    #[test]
    fn estimate_cost_uses_config_pricing() {
        let mut pricing = HashMap::new();
        pricing.insert(
            "custom-model".to_owned(),
            ModelPricing {
                input_cost_per_mtok: 10.0,
                output_cost_per_mtok: 50.0,
            },
        );
        let cost = estimate_cost(&pricing, "custom-model", 1000, 100);
        // (1000 * 10.0 + 100 * 50.0) / 1_000_000 = 15000 / 1_000_000 = 0.015
        assert!((cost - 0.015).abs() < 0.0001);
    }

    #[test]
    fn estimate_cost_config_overrides_default() {
        let mut pricing = HashMap::new();
        pricing.insert(
            "claude-opus-4-20250514".to_owned(),
            ModelPricing {
                input_cost_per_mtok: 20.0,
                output_cost_per_mtok: 100.0,
            },
        );
        let cost = estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100);
        // (1000 * 20.0 + 100 * 100.0) / 1_000_000 = 30000 / 1_000_000 = 0.03
        assert!((cost - 0.03).abs() < 0.0001);
    }

    // --- backoff_delay unit tests ---

    #[test]
    fn backoff_delay_respects_retry_after() {
        let err = error::RateLimitedSnafu {
            retry_after_ms: 5000_u64,
        }
        .build();
        let delay = backoff_delay(1, Some(&err));
        assert_eq!(delay, Duration::from_millis(5000));
    }

    #[test]
    fn backoff_delay_exponential_growth() {
        let d1 = backoff_delay(1, None);
        let d2 = backoff_delay(2, None);
        let d3 = backoff_delay(3, None);
        assert!(d1 < d2, "attempt 2 should be longer than attempt 1");
        assert!(d2 < d3, "attempt 3 should be longer than attempt 2");
        assert!(
            d3 <= Duration::from_millis(BACKOFF_MAX_MS + BACKOFF_MAX_MS / 4),
            "delay should be capped near BACKOFF_MAX_MS"
        );
    }
}
