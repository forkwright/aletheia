//! Anthropic Messages API provider.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use snafu::ResultExt;
use tracing::{Instrument as _, info, info_span, warn};

use aletheia_koina::credential::{CredentialProvider, CredentialSource};
use aletheia_koina::secret::SecretString;

use crate::error::{self, Result};
use crate::health::{HealthConfig, ProviderHealthTracker};
use crate::provider::{LlmProvider, ModelPricing, ProviderConfig};
use crate::types::{CompletionRequest, CompletionResponse};

use super::stream::{StreamAccumulator, StreamEvent, parse_sse_response};
use super::wire::WireRequest;

use crate::models::{DEFAULT_API_VERSION, DEFAULT_BASE_URL, DEFAULT_MAX_RETRIES, SUPPORTED_MODELS};

use super::pricing::{backoff_delay, estimate_cost};

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    // kanon:ignore RUST/pub-visibility
    client: Client,
    credential_provider: Arc<dyn CredentialProvider>,
    base_url: String,
    api_version: String,
    max_retries: u32,
    pricing: HashMap<String, ModelPricing>,
    health: Arc<ProviderHealthTracker>,
}

/// Static credential provider for backward-compatible `from_config()`.
struct StaticCredentialProvider {
    key: SecretString,
}

impl CredentialProvider for StaticCredentialProvider {
    fn get_credential(&self) -> Option<aletheia_koina::credential::Credential> {
        Some(aletheia_koina::credential::Credential {
            secret: self.key.clone(),
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
    // WHY: reqwest 0.13 with rustls-no-provider requires an explicit crypto provider.
    // install_default() is idempotent: subsequent calls return Err and are ignored.
    let _ = rustls::crypto::ring::default_provider().install_default();

    Client::builder()
        .connect_timeout(Duration::from_secs(10))
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
    /// Merge operator-provided pricing on top of built-in defaults.
    ///
    /// Built-in pricing covers all first-party Anthropic models. Operator
    /// overrides win when both maps contain the same key, so custom rates
    /// are respected while models the operator did not configure (e.g.
    /// background-task models like Haiku) still have pricing.
    fn merge_pricing(config: &ProviderConfig) -> HashMap<String, ModelPricing> {
        let mut merged = ProviderConfig::default().pricing;
        merged.extend(config.pricing.clone());
        merged
    }

    /// Create a provider from configuration with a static API key.
    ///
    /// # Errors
    /// Returns `ProviderInit` if `api_key` is missing.
    #[must_use = "returns configured provider or error"]
    pub fn from_config(config: &ProviderConfig) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        let api_key = config
            .api_key
            .as_ref()
            .filter(|k| !k.expose_secret().is_empty())
            .ok_or_else(|| {
                error::ProviderInitSnafu {
                    message: "api_key is required for Anthropic provider".to_owned(),
                }
                .build()
            })?;

        let provider = Self {
            client: build_http_client()?,
            credential_provider: Arc::new(StaticCredentialProvider {
                key: api_key.clone(),
            }),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            api_version: DEFAULT_API_VERSION.to_owned(),
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            pricing: Self::merge_pricing(config),
            health: Arc::new(ProviderHealthTracker::new(HealthConfig::default())),
        };
        // WARNING: credentials sent in HTTP headers -- non-HTTPS base URLs expose them in transit
        if !provider.base_url.starts_with("https://") {
            warn!(base_url = %provider.base_url, "API base URL is not HTTPS -- credentials may be transmitted in cleartext");
        }
        Ok(provider)
    }

    /// Create a provider with a dynamic credential provider.
    ///
    /// The credential is resolved per-request via `provider.get_credential()`,
    /// enabling mid-session token rotation and background OAuth refresh.
    pub fn with_credential_provider(
        // kanon:ignore RUST/pub-visibility
        provider: Arc<dyn CredentialProvider>,
        config: &ProviderConfig,
    ) -> Result<Self> {
        let provider = Self {
            client: build_http_client()?,
            credential_provider: provider,
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            api_version: DEFAULT_API_VERSION.to_owned(),
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            pricing: Self::merge_pricing(config),
            health: Arc::new(ProviderHealthTracker::new(HealthConfig::default())),
        };
        // WARNING: credentials sent in HTTP headers -- non-HTTPS base URLs expose them in transit
        if !provider.base_url.starts_with("https://") {
            warn!(base_url = %provider.base_url, "API base URL is not HTTPS -- credentials may be transmitted in cleartext");
        }
        Ok(provider)
    }

    /// Streaming completion: accumulates into a final `CompletionResponse`
    /// while emitting deltas to the callback.
    ///
    /// Retries on transient errors (overloaded, rate-limited) with exponential
    /// backoff, but **only if no content has been emitted** to the callback yet.
    /// Once deltas have been streamed, a retry would produce duplicate/corrupt
    /// output, so mid-content errors propagate immediately.
    ///
    /// This is an `AnthropicProvider`-specific method. The `LlmProvider`
    /// trait only exposes `complete()`.
    pub async fn complete_streaming(
        &self,
        request: &CompletionRequest,
        mut on_event: impl FnMut(StreamEvent) + Send,
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
        self.complete_streaming_inner(request, &mut on_event)
            .instrument(span)
            .await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "streaming retry loop with span recording at each exit point"
    )]
    async fn complete_streaming_inner(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        if let Err(health) = self.health.check_available() {
            tracing::warn!(?health, "circuit-breaker open; streaming request rejected");
            return Err(error::ApiRequestSnafu {
                message: format!("provider circuit-breaker open: {health:?}"),
            }
            .build());
        }

        let start = Instant::now();
        let mut ttft: Option<std::time::Duration> = None;

        let request = &self.maybe_prepend_oauth_identity(request);
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

            let (token_prefix, credential_source) = self.credential_log_info();
            let headers = self.build_headers()?;

            let mut response = match self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers)
                .body(body.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let err = super::error::map_request_error(&e);
                    self.health.record_error(&err);
                    last_error = Some(err);
                    continue;
                }
            };

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let err = super::error::map_error_response(
                    response,
                    &request.model,
                    &token_prefix,
                    &credential_source,
                )
                .await;
                self.health.record_error(&err);
                if status == 401 || ((400..500).contains(&status) && status != 429) {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::as_conversions,
                        reason = "u128→u64: LLM call duration fits in u64"
                    )]
                    {
                        tracing::Span::current()
                            .record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
                    }
                    tracing::Span::current().record("llm.retries", attempt);
                    if status == 401 {
                        tracing::Span::current().record("llm.status", "auth_failed");
                    } else {
                        tracing::Span::current().record("llm.status", "error");
                    }
                    return Err(err);
                }
                last_error = Some(err);
                continue;
            }

            let mut accumulator = StreamAccumulator::new();
            let mut content_started = false;

            let stream_result = parse_sse_response(&mut response, &mut accumulator, &mut |event| {
                if matches!(
                    event,
                    StreamEvent::TextDelta { .. }
                        | StreamEvent::ThinkingDelta { .. }
                        | StreamEvent::InputJsonDelta { .. }
                ) {
                    if ttft.is_none() {
                        ttft = Some(start.elapsed());
                    }
                    content_started = true;
                }
                on_event(event);
            })
            .await;

            match stream_result {
                Ok(()) => {
                    let resp = accumulator.finish();
                    self.health.record_success();
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::as_conversions,
                        reason = "u128→u64: LLM call duration fits in u64"
                    )]
                    {
                        tracing::Span::current()
                            .record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
                    }
                    tracing::Span::current().record("llm.tokens_in", resp.usage.input_tokens);
                    tracing::Span::current().record("llm.tokens_out", resp.usage.output_tokens);
                    tracing::Span::current().record("llm.status", "ok");
                    tracing::Span::current().record("llm.retries", attempt);
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
                    crate::metrics::record_latency(
                        &request.model,
                        "ok",
                        start.elapsed().as_secs_f64(),
                    );
                    if let Some(ttft_dur) = ttft {
                        crate::metrics::record_ttft(&request.model, "ok", ttft_dur.as_secs_f64());
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    // WHY: If content was already streamed, we can't retry: it would
                    // produce duplicates. Propagate immediately.
                    if content_started {
                        tracing::error!("SSE error after content started streaming — cannot retry");
                        self.health.record_error(&e);
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::as_conversions,
                            reason = "u128→u64: LLM call duration fits in u64"
                        )]
                        {
                            tracing::Span::current()
                                .record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
                        }
                        tracing::Span::current().record("llm.retries", attempt);
                        tracing::Span::current().record("llm.status", "error");
                        crate::metrics::record_latency(
                            &request.model,
                            "error",
                            start.elapsed().as_secs_f64(),
                        );
                        return Err(e);
                    }
                    if matches!(e, error::Error::RateLimited { .. }) {
                        tracing::warn!("SSE stream returned retryable error before content");
                        self.health.record_error(&e);
                        last_error = Some(e);
                        continue;
                    }
                    self.health.record_error(&e);
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::as_conversions,
                        reason = "u128→u64: LLM call duration fits in u64"
                    )]
                    {
                        tracing::Span::current()
                            .record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
                    }
                    tracing::Span::current().record("llm.retries", attempt);
                    tracing::Span::current().record("llm.status", "error");
                    return Err(e);
                }
            }
        }

        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "u128→u64: LLM call duration fits in u64"
        )]
        {
            tracing::Span::current().record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
        }
        tracing::Span::current().record("llm.retries", self.max_retries);
        tracing::Span::current().record("llm.status", "error");

        crate::metrics::record_completion("anthropic", 0, 0, 0.0, false);
        crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());

        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "streaming request failed after all retries".to_owned(),
            }
            .build()
        }))
    }

    /// Return `(token_prefix, credential_source)` strings for diagnostic logging.
    ///
    /// The token prefix is the first 15 characters of the current secret value;
    /// the credential source is the [`CredentialSource`] display string.
    /// Returns empty strings when no credential is available.
    fn credential_log_info(&self) -> (String, String) {
        match self.credential_provider.get_credential() {
            Some(cred) => {
                let s = cred.secret.expose_secret();
                let prefix = s.get(..15).unwrap_or(s).to_owned();
                let source = cred.source.to_string();
                (prefix, source)
            }
            None => (String::new(), String::new()),
        }
    }

    /// Prepend the Claude Code system prompt identity when using OAuth.
    ///
    /// Anthropic gates Sonnet/Opus access on OAuth tokens behind a server-side
    /// system prompt check. The Messages API inspects the system field and only
    /// allows higher-tier models when the prompt begins with the CC identity.
    /// Haiku works without this. This matches Claude Code's own behavior.
    fn maybe_prepend_oauth_identity(&self, request: &CompletionRequest) -> CompletionRequest {
        let credential = self.credential_provider.get_credential();
        let Some(cred) = credential else {
            return request.clone();
        };
        if cred.source != CredentialSource::OAuth {
            return request.clone();
        }

        // WHY: Anthropic requires the EXACT CC identity as the system field for
        // OAuth tokens to access Sonnet/Opus. Any additional content in the system
        // field causes 400. The actual bootstrap prompt moves into messages as
        // the first System-role message. This matches how CC itself works.
        let mut req = request.clone();
        if let Some(existing_system) = req.system.take() {
            // Insert bootstrap as a User message with system context label,
            // since System-role messages get extracted back into the system field
            // by the wire layer. The LLM treats the first User message as context.
            req.messages.insert(
                0,
                crate::types::Message {
                    role: crate::types::Role::User,
                    content: crate::types::Content::Text(format!(
                        "[System context]\n\n{existing_system}"
                    )),
                },
            );
        }
        req.system = Some("You are Claude Code, Anthropic's official CLI for Claude.".to_owned());
        req.cache_system = false;
        req
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let credential = self.credential_provider.get_credential().ok_or_else(|| {
            error::AuthFailedSnafu {
                message: "no credential available from provider".to_owned(),
            }
            .build()
        })?;

        let secret_value = credential.secret.expose_secret();
        if secret_value.is_empty() {
            return Err(error::AuthFailedSnafu {
                message: "credential secret is empty, cannot build Authorization header".to_owned(),
            }
            .build());
        }

        let mut headers = HeaderMap::new();
        if credential.source == CredentialSource::OAuth {
            let value = HeaderValue::from_str(&format!("Bearer {secret_value}")).map_err(|_e| {
                error::AuthFailedSnafu {
                    message: "credential contains invalid header characters".to_owned(),
                }
                .build()
            })?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
            // WHY: Anthropic requires this beta header to accept OAuth tokens
            // on the Messages API. Without it, the API returns 401
            // "OAuth authentication is currently not supported."
            headers.insert(
                "anthropic-beta",
                HeaderValue::from_static("oauth-2025-04-20"),
            );
        } else {
            let value = HeaderValue::from_str(secret_value).map_err(|_e| {
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
        self.execute_with_retry_inner(request)
            .instrument(span)
            .await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "retry loop with span recording at each exit point"
    )]
    async fn execute_with_retry_inner(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse> {
        if let Err(health) = self.health.check_available() {
            tracing::warn!(
                ?health,
                "circuit-breaker open; non-streaming request rejected"
            );
            return Err(error::ApiRequestSnafu {
                message: format!("provider circuit-breaker open: {health:?}"),
            }
            .build());
        }

        let start = Instant::now();

        // WHY: Anthropic gates Sonnet/Opus access on OAuth tokens behind a system
        // prompt identity check. Without the CC identity prefix, only Haiku-tier
        // models are available. This matches what Claude Code itself sends.
        let request = &self.maybe_prepend_oauth_identity(request);

        let wire = WireRequest::from_request(request, None);
        let body = serde_json::to_string(&wire).context(error::ParseResponseSnafu)?;

        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(backoff_delay(attempt, last_error.as_ref())).await;
            }

            let (token_prefix, credential_source) = self.credential_log_info();
            let headers = self.build_headers()?;

            // codequality:ignore — HTTPS enforced by constructor (from_config / with_credential_provider)
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
                    let err = super::error::map_request_error(&e);
                    self.health.record_error(&err);
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
                let parsed = super::error::parse_response_body::<super::wire::WireResponse>(&text)
                    .and_then(|r| {
                        r.into_response()
                            .map_err(|msg| error::ApiRequestSnafu { message: msg }.build())
                    });
                if let Ok(ref resp) = parsed {
                    self.health.record_success();
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::as_conversions,
                        reason = "u128→u64: LLM call duration fits in u64"
                    )]
                    {
                        tracing::Span::current()
                            .record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
                    }
                    tracing::Span::current().record("llm.tokens_in", resp.usage.input_tokens);
                    tracing::Span::current().record("llm.tokens_out", resp.usage.output_tokens);
                    tracing::Span::current().record("llm.status", "ok");
                    tracing::Span::current().record("llm.retries", attempt);
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
                    crate::metrics::record_latency(
                        &request.model,
                        "ok",
                        start.elapsed().as_secs_f64(),
                    );
                }
                return parsed;
            }

            let err = super::error::map_error_response(
                response,
                &request.model,
                &token_prefix,
                &credential_source,
            )
            .await;
            self.health.record_error(&err);

            if status == 401 || ((400..500).contains(&status) && status != 429) {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::as_conversions,
                    reason = "u128→u64: LLM call duration fits in u64"
                )]
                {
                    tracing::Span::current()
                        .record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
                }
                tracing::Span::current().record("llm.retries", attempt);
                if status == 401 {
                    tracing::Span::current().record("llm.status", "auth_failed");
                } else if status == 429 {
                    tracing::Span::current().record("llm.status", "rate_limited");
                } else {
                    tracing::Span::current().record("llm.status", "error");
                }
                return Err(err);
            }

            last_error = Some(err);
        }

        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "u128→u64: LLM call duration fits in u64"
        )]
        {
            tracing::Span::current().record("llm.duration_ms", start.elapsed().as_millis() as u64); // kanon:ignore RUST/as-cast
        }
        tracing::Span::current().record("llm.retries", self.max_retries);
        tracing::Span::current().record("llm.status", "error");

        crate::metrics::record_completion("anthropic", 0, 0, 0.0, false);
        crate::metrics::record_latency(&request.model, "error", start.elapsed().as_secs_f64());

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

    fn supports_streaming(&self) -> bool {
        true
    }

    fn complete_streaming<'a>(
        &'a self,
        request: &'a CompletionRequest,
        on_event: &'a mut (dyn FnMut(StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.complete_streaming_inner(request, on_event))
    }
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
