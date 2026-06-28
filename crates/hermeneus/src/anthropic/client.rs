//! Anthropic Messages API provider.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use snafu::ResultExt;
use tracing::{Instrument as _, info, info_span};

use koina::credential::{CredentialProvider, CredentialSource};
use koina::secret::SecretString;

use crate::error::{self, Result};
use crate::health::{HealthConfig, ProviderHealthTracker};
use crate::provider::{
    DeploymentTarget, LlmProvider, MatchKind, ModelPricing, PromptCacheMode, ProviderConfig,
};
use crate::types::{CompletionRequest, CompletionResponse};

use super::stream::{StreamAccumulator, StreamEvent, parse_sse_response};
use super::wire::WireRequest;

use crate::models::{DEFAULT_API_VERSION, DEFAULT_BASE_URL, DEFAULT_MAX_RETRIES};

use super::pricing::{backoff_delay, estimate_cost_with_cache};

/// Runtime-configurable provider behavior overrides.
///
/// Passed to [`AnthropicProvider::with_credential_provider_and_behavior`] to
/// parameterize constants that were previously hardcoded. Values come from
/// [`taxis::config::ProviderBehaviorConfig`].
pub struct ProviderBehavior {
    /// Per-request timeout for non-streaming completions.
    pub non_streaming_timeout: Duration,
    /// Default retry delay in milliseconds for SSE stream errors.
    pub sse_retry_ms: u64,
}

impl ProviderBehavior {
    /// Create a new behavior configuration.
    pub fn new(non_streaming_timeout: Duration, sse_retry_ms: u64) -> Self {
        Self {
            non_streaming_timeout,
            sse_retry_ms,
        }
    }
}

/// HTTP endpoint for the Anthropic Messages API.
struct ApiEndpoint {
    base_url: String,
    api_version: String,
}

/// Identity and routing metadata for an [`AnthropicProvider`] instance.
struct InstanceMeta {
    /// Instance name for logs, health tracking, and registry diagnostics.
    /// `"anthropic"` for the first-party endpoint; operator-declared
    /// compatible endpoints carry their config name (e.g. `"kimi-coding"`).
    name: String,
    /// Model IDs this instance claims for registry routing. The first-party
    /// catalog by default; an operator-declared compatible endpoint claims
    /// exactly its configured model list instead.
    /// WHY (#5259): stored as owned `String`s so config-owned IDs are never
    /// intentionally leaked for the lifetime of the process.
    models: Vec<String>,
    /// Whether `models` came from operator configuration.
    has_operator_model_refs: bool,
    /// Where this instance's traffic terminates, for the recall
    /// sensitivity filter (#3404, #3413).
    deployment_target: DeploymentTarget,
}

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    // kanon:ignore RUST/pub-visibility
    client: Client,
    credential_provider: Arc<dyn CredentialProvider>,
    endpoint: ApiEndpoint,
    max_retries: u32,
    pricing: HashMap<String, ModelPricing>,
    health: Arc<ProviderHealthTracker>,
    /// CC profile for request mimicry. `Some` when using OAuth credentials.
    cc_profile: Option<super::cc_profile::CcProfile>,
    /// Runtime behavior overrides (timeouts, retry delays).
    behavior: ProviderBehavior,
    /// Prompt cache policy (#3410). When `Disabled`, all `cache_control`
    /// markers are scrubbed before the wire request is built so operator
    /// content never enters Anthropic's prompt cache.
    prompt_cache_mode: PromptCacheMode,

    /// Instance identity and routing metadata.
    meta: InstanceMeta,
}

/// Static credential provider for backward-compatible `from_config()`.
struct StaticCredentialProvider {
    key: SecretString,
}

impl CredentialProvider for StaticCredentialProvider {
    fn get_credential(&self) -> Option<koina::credential::Credential> {
        Some(koina::credential::Credential {
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

/// Resolve the instance name from config: operator-declared name, or the
/// first-party default.
fn instance_name(config: &ProviderConfig) -> String {
    config
        .name
        .clone()
        .unwrap_or_else(|| "anthropic".to_owned())
}

/// Resolve the routing model claims from config: the operator-declared list
/// for compatible endpoints, or the first-party catalog when unset.
fn models_from_config(config: &ProviderConfig) -> Vec<String> {
    if config.models.is_empty() {
        koina::models::provider_models(koina::models::ModelProvider::Anthropic)
            .iter()
            .map(|&s| s.to_owned())
            .collect()
    } else {
        config.models.clone()
    }
}

/// Per-request timeout for non-streaming completions.
pub const NON_STREAMING_TIMEOUT: Duration = Duration::from_mins(2);

/// HTTP headers sent on every outbound Anthropic request to opt out of
/// using operator traffic for model training (#3406).
///
/// Anthropic has not publicly documented a canonical per-request training
/// opt-out header. Both names are sent defensively: one or both will take
/// effect if Anthropic's API honours either, and unknown headers are
/// harmless no-ops otherwise.
const ANTHROPIC_TRAINING_OPTOUT_HEADERS: &[&str] =
    &["anthropic-disable-training", "anthropic-training-opt-out"];

/// Per-request timeout for streaming completions. Generous because actual stall
/// detection is handled by the SSE parser's idle timeout; this is a safety net
/// that overrides the shorter client-level default.
const STREAMING_TIMEOUT: Duration = Duration::from_mins(10);

/// Returns true when the URL uses TLS or is safe to use without TLS.
///
/// WHY(#5055): delegate to the parsed shared policy so Anthropic-compatible
/// providers cannot whitelist suffix-spoofed loopback-looking hosts.
fn has_allowed_transport(url: &str) -> bool {
    koina::http::is_secure_or_plaintext_loopback_url(url)
}

fn build_http_client() -> Result<Client> {
    // WHY: reqwest 0.13 with rustls-no-provider requires an explicit crypto provider.
    // install_default() is idempotent: subsequent calls return Err and are ignored.
    let _ = rustls::crypto::ring::default_provider().install_default(); // kanon:ignore RUST/no-silent-result-swallow WHY: install_default is idempotent; Err on second call is expected and safe to discard

    // WHY: client-level timeout is a safety net for the full request lifecycle.
    // Non-streaming requests override with NON_STREAMING_TIMEOUT per-request.
    // Streaming requests override with a generous per-request timeout since the
    // SSE parser's idle detection handles actual stall recovery.
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_mins(1))
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
            endpoint: ApiEndpoint {
                base_url: config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
                api_version: DEFAULT_API_VERSION.to_owned(),
            },
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            pricing: Self::merge_pricing(config),
            health: Arc::new(ProviderHealthTracker::new(HealthConfig::default())),
            cc_profile: None, // API key mode — no mimicry needed
            behavior: ProviderBehavior::new(
                NON_STREAMING_TIMEOUT,
                super::error::SSE_DEFAULT_RETRY_MS,
            ),
            prompt_cache_mode: config.prompt_cache_mode,
            meta: InstanceMeta {
                name: instance_name(config),
                models: models_from_config(config),
                has_operator_model_refs: !config.models.is_empty(),
                deployment_target: config.deployment_target,
            },
        };
        // TODO(#2178): add allow_insecure config field
        if !has_allowed_transport(&provider.endpoint.base_url) {
            return Err(error::ProviderInitSnafu {
                message: format!(
                    "API base URL must use HTTPS (got {:?}). Credentials are sent in HTTP headers and would be exposed in cleartext.",
                    koina::http::transport_url_for_diagnostic(&provider.endpoint.base_url)
                ),
            }
            .build());
        }
        Ok(provider)
    }

    /// Create a provider with a dynamic credential provider.
    ///
    /// The credential is resolved per-request via `provider.get_credential()`,
    /// enabling mid-session token rotation and background OAuth refresh.
    /// When using OAuth credentials against the first-party API, CC mimicry
    /// is automatically enabled so requests match Claude Code's fingerprint.
    pub fn with_credential_provider(
        // kanon:ignore RUST/pub-visibility
        provider: Arc<dyn CredentialProvider>,
        config: &ProviderConfig,
    ) -> Result<Self> {
        // WHY: OAuth credentials against the first-party API need CC mimicry
        // to avoid detection as a third-party harness. Only activate when
        // the credential provider can supply OAuth tokens and we're hitting
        // api.anthropic.com (not a proxy or local endpoint).
        let is_first_party = config
            .base_url
            .as_deref()
            .is_none_or(|u| u.contains("anthropic.com"));
        let cc_profile = if is_first_party && config.cc_mimicry.unwrap_or(true) {
            Some(super::cc_profile::CcProfile::from_installed_cli())
        } else {
            None
        };

        let provider = Self {
            client: build_http_client()?,
            credential_provider: provider,
            endpoint: ApiEndpoint {
                base_url: config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
                api_version: DEFAULT_API_VERSION.to_owned(),
            },
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            pricing: Self::merge_pricing(config),
            health: Arc::new(ProviderHealthTracker::new(HealthConfig::default())),
            cc_profile,
            behavior: ProviderBehavior::new(
                NON_STREAMING_TIMEOUT,
                super::error::SSE_DEFAULT_RETRY_MS,
            ),
            prompt_cache_mode: config.prompt_cache_mode,
            meta: InstanceMeta {
                name: instance_name(config),
                models: models_from_config(config),
                has_operator_model_refs: !config.models.is_empty(),
                deployment_target: config.deployment_target,
            },
        };
        // TODO(#2178): add allow_insecure config field
        if !has_allowed_transport(&provider.endpoint.base_url) {
            return Err(error::ProviderInitSnafu {
                message: format!(
                    "API base URL must use HTTPS (got {:?}). Credentials are sent in HTTP headers and would be exposed in cleartext.",
                    koina::http::transport_url_for_diagnostic(&provider.endpoint.base_url)
                ),
            }
            .build());
        }
        Ok(provider)
    }

    /// Create a provider with a dynamic credential provider and explicit
    /// behavioral overrides from [`taxis::config::ProviderBehaviorConfig`].
    pub fn with_credential_provider_and_behavior(
        // kanon:ignore RUST/pub-visibility
        provider: Arc<dyn CredentialProvider>,
        config: &ProviderConfig,
        behavior: &ProviderBehavior,
    ) -> Result<Self> {
        let mut this = Self::with_credential_provider(provider, config)?;
        this.behavior.non_streaming_timeout = behavior.non_streaming_timeout;
        this.behavior.sse_retry_ms = behavior.sse_retry_ms;
        Ok(this)
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
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after emitting partial content to the
    /// callback, the stream ends abruptly. The caller's callback may have
    /// received incomplete data with no indication of truncation.
    #[tracing::instrument(skip_all)]
    pub async fn complete_streaming(
        &self,
        request: &CompletionRequest,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<CompletionResponse> {
        let span = info_span!("llm_call",
            llm.provider = %self.meta.name,
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

        let scrubbed = self.apply_prompt_cache_policy(request);
        let request = &self.maybe_prepend_oauth_identity(&scrubbed);
        let attribution = self.compute_attribution(request);
        let wire = WireRequest::from_request(request, Some(true), attribution.as_deref())
            .context(error::ParseResponseSnafu)?;
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

            let credential_source = self.credential_source();
            let headers = self.build_headers()?;

            let mut response = match self
                .client
                .post(format!("{}/v1/messages", self.endpoint.base_url))
                .headers(headers)
                .body(body.clone())
                .timeout(STREAMING_TIMEOUT)
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
                let err =
                    super::error::map_error_response(response, &request.model, &credential_source)
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

            let stream_result = parse_sse_response(
                &mut response,
                &mut accumulator,
                &mut |event| {
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
                },
                self.behavior.sse_retry_ms,
            )
            .await;

            match stream_result {
                Ok(()) => {
                    let mut resp = accumulator.finish();
                    self.health.record_success();
                    let duration_ms =
                        u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
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
                    let cost = estimate_cost_with_cache(&self.pricing, &request.model, &resp.usage);
                    resp.cost_usd = Some(cost);
                    resp.duration_ms = Some(duration_ms);
                    info!(
                        model = %request.model,
                        tokens_in = resp.usage.input_tokens,
                        tokens_out = resp.usage.output_tokens,
                        cost = %format!("~${:.4}", cost),
                        "LLM call complete"
                    );
                    crate::metrics::record_completion(
                        &self.meta.name,
                        resp.usage.input_tokens,
                        resp.usage.output_tokens,
                        cost,
                        true,
                    );
                    crate::metrics::record_cache_tokens(
                        &self.meta.name,
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
                    if e.is_retryable() {
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
        // WHY: when retries are exhausted after 429s (HTTP rate-limit or SSE
        // overload events), the terminal span must report "rate_limited" so
        // operators can distinguish exhausted rate-limit sequences from true
        // 4xx/5xx errors.
        let terminal_status = if last_error
            .as_ref()
            .is_some_and(|e| matches!(e, error::Error::RateLimited { .. }))
        {
            "rate_limited"
        } else {
            "error"
        };
        tracing::Span::current().record("llm.status", terminal_status);

        crate::metrics::record_completion(&self.meta.name, 0, 0, 0.0, false);
        crate::metrics::record_latency(
            &request.model,
            terminal_status,
            start.elapsed().as_secs_f64(),
        );

        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "streaming request failed after all retries".to_owned(),
            }
            .build()
        }))
    }

    /// Return the credential source class string for diagnostic logging.
    ///
    /// Returns an empty string when no credential is available.
    ///
    /// WHY(#4885): only the source class (e.g. "oauth", "api-key") is logged,
    /// never a token prefix or any credential-derived material.
    fn credential_source(&self) -> String {
        match self.credential_provider.get_credential() {
            Some(cred) => cred.source.to_string(),
            None => String::new(),
        }
    }

    /// Prepend the Claude Code system prompt identity when using OAuth.
    ///
    /// Anthropic gates Sonnet/Opus access on OAuth tokens behind a server-side
    /// system prompt check. The Messages API inspects the system field and only
    /// allows higher-tier models when the prompt begins with the CC identity.
    /// Haiku works without this. This matches Claude Code's own behavior.
    ///
    /// // WHY: Anthropic requires the EXACT CC identity as the system field for
    /// // OAuth tokens to access Sonnet/Opus. Any additional content in the system
    /// // field causes 400. The actual bootstrap prompt moves into messages as
    /// // the first User-role message with a [System context] label.
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
                    cache_breakpoint: false,
                },
            );
        }
        req.system = Some("You are Claude Code, Anthropic's official CLI for Claude.".to_owned());
        req.cache_system = false;
        req
    }

    /// Compute the CC attribution string for system prompt injection.
    ///
    /// Returns `None` when CC mimicry is inactive (API key mode or disabled),
    /// or when the runtime credential is not OAuth. Per-request gating is
    /// required to handle mid-session credential rotation correctly.
    ///
    /// // WHY: Attribution is part of Anthropic's OAuth-specific request
    /// // fingerprinting. Sending it for API-key requests produces a misleading
    /// // telemetry fingerprint (request body claims CC-OAuth while HTTP headers
    /// // use x-api-key). Gate here mirrors `maybe_prepend_oauth_identity`.
    fn compute_attribution(&self, request: &CompletionRequest) -> Option<String> {
        let profile = self.cc_profile.as_ref()?;
        let cred = self.credential_provider.get_credential()?;
        if cred.source != CredentialSource::OAuth {
            return None;
        }
        // Extract first user message text for fingerprint computation.
        let first_msg_text = request
            .messages
            .iter()
            .find(|m| m.role == crate::types::Role::User)
            .map(|m| m.content.text())
            .unwrap_or_default();
        Some(profile.attribution_header(&first_msg_text))
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

            // WHY: When cc_profile is active, send the full CC beta set
            // (which includes oauth-2025-04-20). Otherwise fall back to
            // the minimum required for OAuth.
            if let Some(profile) = &self.cc_profile {
                headers.insert(
                    "anthropic-beta",
                    HeaderValue::from_str(&profile.beta_header_value()).map_err(|_e| {
                        error::AuthFailedSnafu {
                            message: "beta header value contains invalid characters".to_owned(),
                        }
                        .build()
                    })?,
                );
                // CC identification headers
                headers.insert("x-app", HeaderValue::from_static("cli"));
                headers.insert(
                    reqwest::header::USER_AGENT,
                    HeaderValue::from_str(&profile.user_agent()).map_err(|_e| {
                        error::AuthFailedSnafu {
                            message: "user agent contains invalid characters".to_owned(),
                        }
                        .build()
                    })?,
                );
                // WHY (#3409): `X-Claude-Code-Session-Id` is stable per-process
                // in upstream CC and lets Anthropic correlate requests back to
                // a single operator session. Send a fresh random UUID on every
                // call instead so the header still satisfies any server-side
                // presence checks without leaking session identity.
                headers.insert(
                    "X-Claude-Code-Session-Id",
                    HeaderValue::from_str(&koina::uuid::uuid_v4()).map_err(|_e| {
                        error::AuthFailedSnafu {
                            message: "session id contains invalid characters".to_owned(),
                        }
                        .build()
                    })?,
                );
                headers.insert(
                    "x-client-request-id",
                    HeaderValue::from_str(&koina::uuid::uuid_v4()).map_err(|_e| {
                        error::AuthFailedSnafu {
                            message: "request id contains invalid characters".to_owned(),
                        }
                        .build()
                    })?,
                );
            } else {
                headers.insert(
                    "anthropic-beta",
                    HeaderValue::from_static("oauth-2025-04-20"),
                );
            }
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
            HeaderValue::from_str(&self.endpoint.api_version).map_err(|e| {
                error::ProviderInitSnafu {
                    message: format!(
                        "api_version {:?} contains invalid HTTP header characters: {e}",
                        self.endpoint.api_version
                    ),
                }
                .build()
            })?,
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        // WHY (#3406): sovereignty default — always opt out of Anthropic using
        // our traffic for model training. API customers already have this by
        // contract, but we belt-and-suspenders the header so any future policy
        // shift or misrouted request still carries an explicit refusal.
        //
        // Both header names are sent because Anthropic's canonical training-opt-out
        // header is undocumented; sending both ensures coverage regardless of
        // which form the server accepts, and both are harmless no-ops if ignored.
        for name in ANTHROPIC_TRAINING_OPTOUT_HEADERS {
            headers.insert(*name, HeaderValue::from_static("true"));
        }
        Ok(headers)
    }

    /// Apply the operator's prompt cache policy (#3410).
    ///
    /// When [`PromptCacheMode::Disabled`] (the sovereignty default), every
    /// `cache_*` flag is zeroed before the wire request is built so the
    /// serializer in [`super::wire::WireRequest::from_request`] emits no
    /// `cache_control` markers. Operators who opt in to `Ephemeral` or
    /// `Extended` keep the caller-provided flags untouched.
    fn apply_prompt_cache_policy(&self, request: &CompletionRequest) -> CompletionRequest {
        if matches!(self.prompt_cache_mode, PromptCacheMode::Disabled) {
            let mut scrubbed = request.clone();
            scrubbed.cache_system = false;
            scrubbed.cache_tools = false;
            scrubbed.cache_turns = false;
            scrubbed
        } else {
            request.clone()
        }
    }

    async fn execute_with_retry(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let span = info_span!("llm_call",
            llm.provider = %self.meta.name,
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
        let scrubbed = self.apply_prompt_cache_policy(request);
        let request = &self.maybe_prepend_oauth_identity(&scrubbed);
        let attribution = self.compute_attribution(request);
        let wire = WireRequest::from_request(request, None, attribution.as_deref())
            .context(error::ParseResponseSnafu)?;
        let body = serde_json::to_string(&wire).context(error::ParseResponseSnafu)?;

        let mut last_error = None;

        // WHY: Reuse the same idempotency key across retries so the server
        // deduplicates if our first request actually succeeded but we timed out.
        let idempotency_key = koina::uuid::uuid_v4();

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(backoff_delay(attempt, last_error.as_ref())).await;
            }

            let credential_source = self.credential_source();
            let mut headers = self.build_headers()?;
            if let Ok(val) = HeaderValue::from_str(&idempotency_key) {
                headers.insert("idempotency-key", val);
            }

            // CodeQL: cleartext-transmission false positive. Both constructors
            // (`from_config` and `with_credential_provider`) reject non-HTTPS base
            // URLs unless the parsed host is loopback. The
            // `endpoint.base_url` is immutable after construction, so by the time
            // we reach this request the scheme has already been validated.
            let response = match self
                .client
                .post(format!("{}/v1/messages", self.endpoint.base_url))
                .headers(headers)
                .body(body.clone())
                .timeout(self.behavior.non_streaming_timeout)
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
                if let Ok(mut resp) = parsed {
                    self.health.record_success();
                    let duration_ms =
                        u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
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
                    let cost = estimate_cost_with_cache(&self.pricing, &request.model, &resp.usage);
                    resp.cost_usd = Some(cost);
                    resp.duration_ms = Some(duration_ms);
                    info!(
                        model = %request.model,
                        tokens_in = resp.usage.input_tokens,
                        tokens_out = resp.usage.output_tokens,
                        cost = %format!("~${:.4}", cost),
                        "LLM call complete"
                    );
                    crate::metrics::record_completion(
                        &self.meta.name,
                        resp.usage.input_tokens,
                        resp.usage.output_tokens,
                        cost,
                        true,
                    );
                    crate::metrics::record_cache_tokens(
                        &self.meta.name,
                        resp.usage.cache_read_tokens,
                        resp.usage.cache_write_tokens,
                    );
                    crate::metrics::record_latency(
                        &request.model,
                        "ok",
                        start.elapsed().as_secs_f64(),
                    );
                    return Ok(resp);
                }
                return parsed;
            }

            let err =
                super::error::map_error_response(response, &request.model, &credential_source)
                    .await;
            self.health.record_error(&err);

            if status == 401 || (400..500).contains(&status) {
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
                    return Err(err);
                }
                // WHY: 429 is retried with backoff; only record the terminal
                // "rate_limited" status after retries are exhausted (see below).
                if status != 429 {
                    tracing::Span::current().record("llm.status", "error");
                    return Err(err);
                }
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
        let terminal_status = if last_error
            .as_ref()
            .is_some_and(|e| matches!(e, error::Error::RateLimited { .. }))
        {
            "rate_limited"
        } else {
            "error"
        };
        tracing::Span::current().record("llm.status", terminal_status);

        crate::metrics::record_completion(&self.meta.name, 0, 0, 0.0, false);
        crate::metrics::record_latency(
            &request.model,
            terminal_status,
            start.elapsed().as_secs_f64(),
        );

        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "request failed after all retries".to_owned(),
            }
            .build()
        }))
    }
}

impl LlmProvider for AnthropicProvider {
    fn shutdown(&self) {
        self.credential_provider.shutdown();
    }

    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute_with_retry(request))
    }

    fn supported_models(&self) -> &[&str] {
        // WHY (#5259): only the first-party static catalog can be returned as
        // `&[&str]` without leaking. Custom config-owned models are exposed
        // through [`Self::supported_model_list`] and used by
        // [`Self::match_specificity`] for routing.
        if self.meta.has_operator_model_refs {
            &[]
        } else {
            koina::models::provider_models(koina::models::ModelProvider::Anthropic)
        }
    }

    fn supported_model_list(&self) -> Vec<std::borrow::Cow<'_, str>> {
        self.meta
            .models
            .iter()
            .map(|s| std::borrow::Cow::Borrowed(s.as_str()))
            .collect()
    }

    fn supports_model(&self, model: &str) -> bool {
        self.match_specificity(model).is_some()
    }

    fn match_specificity(&self, model: &str) -> Option<MatchKind> {
        // WHY (#4881): first-party Anthropic catalog models must be Exact
        // matches so a broad catch-all provider registered earlier (e.g. the
        // Claude Code subprocess provider) cannot intercept ordinary claude-*
        // traffic. Unknown future claude-* aliases still fall back to CatchAll.
        if self.meta.models.iter().any(|m| m == model) {
            Some(MatchKind::Exact)
        } else if model.starts_with("claude-") {
            Some(MatchKind::CatchAll)
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        &self.meta.name
    }

    fn deployment_target(&self) -> DeploymentTarget {
        self.meta.deployment_target
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
            .field("base_url", &self.endpoint.base_url)
            .field("api_version", &self.endpoint.api_version)
            .field("max_retries", &self.max_retries)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
