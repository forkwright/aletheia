//! OpenAI HTTP client implementing [`LlmProvider`].
//!
//! Talks to either the first-party OpenAI `/v1/responses` surface or the
//! OpenAI-compatible `/v1/chat/completions` surface used by llama.cpp
//! `--server`, ollama, vllm, and compatible proxies. The wire translation
//! lives in [`super::wire`]; this module is the transport, retry, and
//! registration shell.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Response};
use tracing::{Instrument as _, info, info_span};

use koina::secret::SecretString;

use crate::RetryPolicy;
use crate::anthropic::StreamEvent;
use crate::anthropic::pricing::estimate_cost;
use crate::concurrency::{AdaptiveConcurrencyLimiter, ConcurrencyConfig, RequestOutcome};
use crate::error::{self, Result};
use crate::health::{HealthConfig, ProviderHealthTracker};
use crate::provider::{DeploymentTarget, LlmProvider, MatchKind, ModelPricing};
use crate::types::{CompletionRequest, CompletionResponse};

use super::error::{map_error_response, map_request_error};
use super::wire::{
    ChatCompletionRequest, ChatCompletionResponse, ResponsesRequest, ResponsesResponse,
    parse_chat_sse_response, parse_responses_sse_response,
};

/// OpenAI HTTP API family used by this provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OpenAiApiFamily {
    /// OpenAI `/v1/chat/completions` and compatible local/proxy endpoints.
    #[default]
    ChatCompletions,
    /// OpenAI first-party `/v1/responses` endpoint.
    Responses,
}

impl OpenAiApiFamily {
    fn endpoint_path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat/completions",
            Self::Responses => "responses",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat-completions",
            Self::Responses => "responses",
        }
    }
}

/// Build an HTTP client with sensible timeouts shared by Anthropic and
/// OpenAI-compatible providers.
fn build_http_client() -> Result<Client> {
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::debug!("rustls crypto provider was already installed");
    }
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
#[derive(Clone)]
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
    /// Retry attempts and exponential backoff policy for transient failures.
    pub retry_policy: RetryPolicy,
    /// Adaptive concurrency limiter settings for this provider instance.
    pub concurrency: ConcurrencyConfig,
    /// Which OpenAI API family to speak. Defaults to Chat Completions for
    /// backwards-compatible local `OpenAI`-compatible endpoints; Aletheia's
    /// first-party `openai` config path sets this to [`OpenAiApiFamily::Responses`].
    pub api_family: OpenAiApiFamily,
    /// Where this provider's traffic terminates, gating which
    /// [`FactSensitivity`](mneme::knowledge::FactSensitivity) the recall
    /// pipeline is allowed to send to it (#3736, #3404, #3413).
    ///
    /// Defaults to [`DeploymentTarget::Cloud`] — the safe assumption that
    /// matches the trait default so existing TOML configurations without an
    /// explicit `deployment_target` key keep their Cloud-classified
    /// behaviour. Operators running a loopback llama.cpp, logismos, or
    /// ollama endpoint MUST set this to `local_hosted` or `embedded` in
    /// `aletheia.toml` so the recall filter lets `Internal` /
    /// `Confidential` facts through to the non-cloud boundary.
    pub deployment_target: DeploymentTarget,
    /// Per-model pricing for cost metrics (optional).
    ///
    /// WHY(#4628): local/compatible providers have no built-in pricing
    /// table; operators that route priced models (e.g. GPT-4o) through
    /// OpenAI-compatible configs must supply rates here so cost telemetry
    /// is non-zero. Unpriced models record `0.0` with a trace warning.
    pub pricing: HashMap<String, ModelPricing>,
}

impl std::fmt::Debug for OpenAiProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProviderConfig")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("models", &self.models)
            .field("request_timeout", &self.request_timeout)
            .field("retry_policy", &self.retry_policy)
            .field("concurrency", &self.concurrency)
            .field("api_family", &self.api_family)
            .field("deployment_target", &self.deployment_target)
            .field("pricing_models", &self.pricing.len())
            .finish()
    }
}

impl Default for OpenAiProviderConfig {
    fn default() -> Self {
        Self {
            name: "openai-compatible".to_owned(),
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key: None,
            models: Vec::new(),
            request_timeout: Duration::from_mins(2),
            retry_policy: RetryPolicy::default(),
            concurrency: ConcurrencyConfig::default(),
            api_family: OpenAiApiFamily::ChatCompletions,
            // WHY(#3736): mirror the trait default so `..Default::default()`
            // does not silently downgrade a caller-specified target.
            deployment_target: DeploymentTarget::Cloud,
            // NOTE: empty by default — unpriced models record 0.0 cost
            pricing: HashMap::new(),
        }
    }
}

/// Returns true when the URL is safe to use without TLS.
///
/// WHY: delegate to `koina::http::is_plaintext_loopback_url` so the plaintext
/// HTTP scheme literal lives in exactly one audited place.
fn is_loopback_url(url: &str) -> bool {
    koina::http::is_plaintext_loopback_url(url)
}

/// OpenAI Chat Completions-compatible LLM provider.
pub struct OpenAiProvider {
    client: Client,
    config: OpenAiProviderConfig,
    health: Arc<ProviderHealthTracker>,
    concurrency: Arc<AdaptiveConcurrencyLimiter>,
    /// Merged pricing table (config-supplied; empty = unpriced local model).
    pricing: HashMap<String, ModelPricing>,
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

        // WHY(#4894): First-party OpenAI requires a real API key. Loopback
        // OpenAI-compatible endpoints may omit auth, but the Responses API
        // targeted at api.openai.com must be authenticated.
        if config.api_family == OpenAiApiFamily::Responses
            && config.api_key.is_none()
            && !is_loopback_url(&config.base_url)
        {
            return Err(error::ProviderInitSnafu {
                message: "first-party OpenAI provider requires an API key; set api_key_env or use provider_type = \"openai-compatible\" for loopback endpoints".to_owned(),
            }.build());
        }

        let client = build_http_client()?;

        info!(
            provider = %config.name,
            base_url = %config.base_url,
            api_family = %config.api_family.as_str(),
            models = ?config.models,
            authenticated = config.api_key.is_some(),
            "OpenAI provider initialized"
        );

        let pricing = config.pricing.clone();
        Ok(Self {
            client,
            concurrency: Arc::new(AdaptiveConcurrencyLimiter::new(
                config.name.clone(),
                config.concurrency.clone(),
            )),
            config,
            health: Arc::new(ProviderHealthTracker::new(HealthConfig::default())),
            pricing,
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
        self.execute_with_concurrency(request)
            .instrument(span)
            .await
    }

    async fn execute_with_concurrency(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse> {
        let permit = self.concurrency.acquire().await;
        let start = Instant::now();
        let result = self.execute_inner(request).await;
        permit.finish_with_latency(concurrency_outcome(&result), start.elapsed());
        result
    }

    async fn execute_inner(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        if let Err(health) = self.health.check_available() {
            return Err(error::ApiRequestSnafu {
                message: format!("provider circuit-breaker open: {health:?}"),
            }
            .build());
        }

        let start = Instant::now();
        let body = self.serialize_request(request, None)?;
        let url = self.endpoint_url();
        let api_family = self.config.api_family.as_str();

        let mut last_error: Option<error::Error> = None;

        for attempt in 0..=self.config.retry_policy.max_retries {
            if attempt > 0 {
                tokio::time::sleep(self.config.retry_policy.delay(attempt, last_error.as_ref()))
                    .await;
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
                let mut resp = self.parse_response_body(&text)?;
                self.health.record_success();
                let cost_usd = estimate_cost(
                    &self.pricing,
                    &request.model,
                    resp.usage.input_tokens,
                    resp.usage.output_tokens,
                );
                record_nonstream_success(
                    start,
                    attempt,
                    &self.config.name,
                    api_family,
                    request,
                    &mut resp,
                    cost_usd,
                );
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

        tracing::Span::current().record("llm.retries", self.config.retry_policy.max_retries);
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
        let span = info_span!("llm_call",
            llm.provider = %self.config.name,
            llm.model = %request.model,
            llm.duration_ms = tracing::field::Empty,
            llm.tokens_in = tracing::field::Empty,
            llm.tokens_out = tracing::field::Empty,
            llm.status = tracing::field::Empty,
            llm.retries = tracing::field::Empty,
            llm.stream = true,
        );
        self.execute_streaming_with_concurrency(request, on_event)
            .instrument(span)
            .await
    }

    async fn execute_streaming_with_concurrency(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        let permit = self.concurrency.acquire().await;
        let start = Instant::now();
        let result = self.execute_streaming_inner(request, on_event).await;
        permit.finish_with_latency(concurrency_outcome(&result), start.elapsed());
        result
    }

    #[expect(
        clippy::too_many_lines,
        reason = "WHY(#5044): streaming retry loop records each terminal path while applying configured retry policy"
    )]
    async fn execute_streaming_inner(
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

        let start = Instant::now();
        let body = self.serialize_request(request, Some(true))?;
        let url = self.endpoint_url();
        let mut last_error: Option<error::Error> = None;
        // WHY(#4887): once any content delta reaches on_event the caller has partial output;
        // retrying would duplicate it. Track across attempts — once true, never retry.
        let mut content_started = false;

        for attempt in 0..=self.config.retry_policy.max_retries {
            if attempt > 0 {
                tokio::time::sleep(self.config.retry_policy.delay(attempt, last_error.as_ref()))
                    .await;
            }

            let mut response = match self.send_streaming_request(&url, &body).await {
                Ok(r) => r,
                Err(err) => {
                    self.health.record_error(&err);
                    if !err.is_retryable() {
                        record_stream_failure(start, attempt, &self.config.name, request);
                        return Err(err);
                    }
                    last_error = Some(err);
                    continue;
                }
            };

            let status = response.status().as_u16();
            if !response.status().is_success() {
                let err =
                    map_error_response(response, &request.model, self.credential_source()).await;
                self.health.record_error(&err);
                if !err.is_retryable() {
                    record_stream_http_failure(start, attempt, &self.config.name, request, status);
                    return Err(err);
                }
                last_error = Some(err);
                continue;
            }

            let mut tracking_event = |event: StreamEvent| {
                if matches!(
                    event,
                    StreamEvent::TextDelta { .. }
                        | StreamEvent::ThinkingDelta { .. }
                        | StreamEvent::InputJsonDelta { .. }
                ) {
                    content_started = true;
                }
                on_event(event);
            };
            let resp = self
                .parse_streaming_response(&mut response, &mut tracking_event)
                .await;

            match resp {
                Ok(mut resp) => {
                    self.health.record_success();
                    let cost_usd = estimate_cost(
                        &self.pricing,
                        &request.model,
                        resp.usage.input_tokens,
                        resp.usage.output_tokens,
                    );
                    record_stream_success(
                        start,
                        attempt,
                        &self.config.name,
                        request,
                        &mut resp,
                        cost_usd,
                    );
                    return Ok(resp);
                }
                Err(err) => {
                    self.health.record_error(&err);
                    if content_started {
                        // WHY(#4887): content already delivered; retry would duplicate output.
                        tracing::error!("SSE error after content started streaming; cannot retry");
                        record_stream_failure(start, attempt, &self.config.name, request);
                        return Err(err);
                    }
                    if !err.is_retryable() {
                        record_stream_failure(start, attempt, &self.config.name, request);
                        return Err(err);
                    }
                    last_error = Some(err);
                }
            }
        }

        tracing::Span::current().record("llm.duration_ms", elapsed_millis_u64(start));
        tracing::Span::current().record("llm.retries", self.config.retry_policy.max_retries);
        tracing::Span::current().record("llm.status", "error");
        crate::metrics::record_completion(&self.config.name, 0, 0, 0.0, false);
        crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());
        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "streaming request failed after all retries".to_owned(),
            }
            .build()
        }))
    }

    async fn send_streaming_request(&self, url: &str, body: &str) -> Result<Response> {
        let headers = self.build_headers()?;
        self.client
            .post(url)
            .headers(headers)
            .body(body.to_owned())
            .timeout(Duration::from_mins(10))
            .send()
            .await
            .map_err(|e| map_request_error(&e))
    }

    async fn parse_streaming_response(
        &self,
        response: &mut Response,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        match self.config.api_family {
            OpenAiApiFamily::ChatCompletions => parse_chat_sse_response(response, on_event).await,
            OpenAiApiFamily::Responses => parse_responses_sse_response(response, on_event).await,
        }
    }

    fn endpoint_url(&self) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            self.config.api_family.endpoint_path()
        )
    }

    fn serialize_request(
        &self,
        request: &CompletionRequest,
        stream: Option<bool>,
    ) -> Result<String> {
        let body = match self.config.api_family {
            OpenAiApiFamily::ChatCompletions => {
                let wire = ChatCompletionRequest::from_request(request, stream)?;
                serde_json::to_string(&wire)
            }
            OpenAiApiFamily::Responses => {
                let wire = ResponsesRequest::from_request(request, stream)?;
                serde_json::to_string(&wire)
            }
        };
        body.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to serialize request: {e}"),
            }
            .build()
        })
    }

    fn parse_response_body(&self, text: &str) -> Result<CompletionResponse> {
        match self.config.api_family {
            OpenAiApiFamily::ChatCompletions => {
                let parsed: ChatCompletionResponse = serde_json::from_str(text).map_err(|e| {
                    error::ApiRequestSnafu {
                        message: format!("failed to parse OpenAI chat response: {e}"),
                    }
                    .build()
                })?;
                parsed
                    .into_response()
                    .map_err(|msg| error::ApiRequestSnafu { message: msg }.build())
            }
            OpenAiApiFamily::Responses => {
                let parsed: ResponsesResponse = serde_json::from_str(text).map_err(|e| {
                    error::ApiRequestSnafu {
                        message: format!("failed to parse OpenAI Responses response: {e}"),
                    }
                    .build()
                })?;
                parsed
                    .into_response()
                    .map_err(|msg| error::ApiRequestSnafu { message: msg }.build())
            }
        }
    }
}

fn elapsed_millis_u64(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn record_nonstream_success(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    api_family: &str,
    request: &CompletionRequest,
    response: &mut CompletionResponse,
    cost_usd: f64,
) {
    response.duration_ms = Some(elapsed_millis_u64(start));
    tracing::Span::current().record("llm.tokens_in", response.usage.input_tokens);
    tracing::Span::current().record("llm.tokens_out", response.usage.output_tokens);
    tracing::Span::current().record("llm.status", "ok");
    tracing::Span::current().record("llm.retries", attempt);
    info!(
        provider = %provider_name,
        api_family,
        model = %request.model,
        tokens_in = response.usage.input_tokens,
        tokens_out = response.usage.output_tokens,
        "OpenAI call complete"
    );
    crate::metrics::record_completion(
        provider_name,
        response.usage.input_tokens,
        response.usage.output_tokens,
        cost_usd,
        true,
    );
    // WHY(#4658): OpenAI wire parsers populate cache_read_tokens; emit them so
    // cost/efficiency dashboards see prompt-cache activity.
    crate::metrics::record_cache_tokens(
        provider_name,
        response.usage.cache_read_tokens,
        response.usage.cache_write_tokens,
    );
    crate::metrics::record_latency(&request.model, "ok", start.elapsed().as_secs_f64());
}

fn record_stream_failure(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    request: &CompletionRequest,
) {
    tracing::Span::current().record("llm.duration_ms", elapsed_millis_u64(start));
    tracing::Span::current().record("llm.retries", attempt);
    tracing::Span::current().record("llm.status", "error");
    crate::metrics::record_completion(provider_name, 0, 0, 0.0, false);
    crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());
}

fn record_stream_http_failure(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    request: &CompletionRequest,
    status: u16,
) {
    tracing::Span::current().record("llm.duration_ms", elapsed_millis_u64(start));
    tracing::Span::current().record("llm.retries", attempt);
    tracing::Span::current().record(
        "llm.status",
        if status == 401 {
            "auth_failed"
        } else {
            "error"
        },
    );
    crate::metrics::record_completion(provider_name, 0, 0, 0.0, false);
    crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());
}

fn record_stream_success(
    start: Instant,
    attempt: u32,
    provider_name: &str,
    request: &CompletionRequest,
    response: &mut CompletionResponse,
    cost_usd: f64,
) {
    let duration_ms = elapsed_millis_u64(start);
    response.duration_ms = Some(duration_ms);
    tracing::Span::current().record("llm.duration_ms", duration_ms);
    tracing::Span::current().record("llm.tokens_in", response.usage.input_tokens);
    tracing::Span::current().record("llm.tokens_out", response.usage.output_tokens);
    tracing::Span::current().record("llm.status", "ok");
    tracing::Span::current().record("llm.retries", attempt);
    crate::metrics::record_completion(
        provider_name,
        response.usage.input_tokens,
        response.usage.output_tokens,
        cost_usd,
        true,
    );
    // WHY(#4658): Streaming completions accumulate cached prompt tokens in
    // usage.cache_read_tokens; record them for observability parity with
    // non-streaming completions.
    crate::metrics::record_cache_tokens(
        provider_name,
        response.usage.cache_read_tokens,
        response.usage.cache_write_tokens,
    );
    crate::metrics::record_latency(&request.model, "ok", start.elapsed().as_secs_f64());
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("name", &self.config.name)
            .field("base_url", &self.config.base_url)
            .field("api_family", &self.config.api_family)
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
        // WHY (#5259): dynamic OpenAI-compatible model lists are config-owned;
        // returning them as `&[&str]` would require leaking. Expose them through
        // [`Self::supported_model_list`] and use [`Self::match_specificity`] for
        // routing instead.
        &[]
    }

    fn supported_model_list(&self) -> Vec<std::borrow::Cow<'_, str>> {
        crate::provider::owned_model_list(&self.config.models)
    }

    fn supports_model(&self, model: &str) -> bool {
        self.config.models.iter().any(|m| m == model)
    }

    fn match_specificity(&self, model: &str) -> Option<MatchKind> {
        if self.supports_model(model) {
            Some(MatchKind::Exact)
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    /// WHY(#3736): OpenAI-compatible providers can still be loopback/self-hosted,
    /// so they must propagate the configured deployment target to recall filtering.
    fn deployment_target(&self) -> DeploymentTarget {
        self.config.deployment_target
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

fn concurrency_outcome(result: &Result<CompletionResponse>) -> RequestOutcome {
    match result {
        Ok(_) => RequestOutcome::Success,
        Err(err) if err.is_retryable() => RequestOutcome::Overload,
        Err(_) => RequestOutcome::Neutral,
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
