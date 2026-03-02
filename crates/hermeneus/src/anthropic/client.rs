//! Anthropic Messages API provider.

use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use snafu::ResultExt;
use tracing::instrument;

use crate::error::{self, Result};
use crate::provider::{LlmProvider, ProviderConfig};
use crate::types::{CompletionRequest, CompletionResponse};

use super::stream::{parse_sse_stream, StreamAccumulator, StreamEvent};
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
    api_key: SecretString,
    base_url: String,
    api_version: String,
    max_retries: u32,
}

impl AnthropicProvider {
    /// Create a provider from configuration.
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

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| {
                error::ProviderInitSnafu {
                    message: format!("failed to build HTTP client: {e}"),
                }
                .build()
            })?;

        Ok(Self {
            client,
            api_key: SecretString::from(api_key.clone()),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            api_version: DEFAULT_API_VERSION.to_owned(),
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
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
    #[instrument(skip(self, request, on_event), fields(model = %request.model))]
    pub fn complete_streaming(
        &self,
        request: &CompletionRequest,
        mut on_event: impl FnMut(StreamEvent),
    ) -> Result<CompletionResponse> {
        let wire = WireRequest::from_request(request, Some(true));
        let body = serde_json::to_string(&wire).context(error::ParseResponseSnafu)?;
        let headers = self.build_headers();

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

            // HTTP-level errors (connection, non-200 status)
            let response = match self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers.clone())
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
                Ok(()) => return Ok(accumulator.finish()),
                Err(e) => {
                    // If content was already streamed, we can't retry — it would
                    // produce duplicates. Propagate immediately.
                    if content_started {
                        tracing::error!(
                            "SSE error after content started streaming — cannot retry"
                        );
                        return Err(e);
                    }
                    // Only retry RateLimited (overloaded/429); other errors are terminal.
                    if matches!(e, error::Error::RateLimited { .. }) {
                        tracing::warn!("SSE stream returned retryable error before content");
                        last_error = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "streaming request failed after all retries".to_owned(),
            }
            .build()
        }))
    }

    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(self.api_key.expose_secret())
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_str(&self.api_version)
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_API_VERSION)),
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers
    }

    fn execute_with_retry(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let wire = WireRequest::from_request(request, None);
        let body = serde_json::to_string(&wire).context(error::ParseResponseSnafu)?;

        let mut last_error = None;
        let headers = self.build_headers();

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                std::thread::sleep(backoff_delay(attempt, last_error.as_ref()));
            }

            let response = match self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers.clone())
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
                return super::error::parse_response_body::<super::wire::WireResponse>(&text)
                    .and_then(|r| {
                        r.into_response()
                            .map_err(|msg| error::ApiRequestSnafu { message: msg }.build())
                    });
            }

            let err = super::error::map_error_response(response);

            // Non-retryable: 401, 400-level (except 429).
            if status == 401 || ((400..500).contains(&status) && status != 429) {
                return Err(err);
            }

            // Retryable: 429, 5xx, network errors.
            last_error = Some(err);
        }

        Err(last_error.unwrap_or_else(|| {
            error::ApiRequestSnafu {
                message: "request failed after all retries".to_owned(),
            }
            .build()
        }))
    }
}

impl LlmProvider for AnthropicProvider {
    #[instrument(skip(self, request), fields(model = %request.model))]
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

fn backoff_delay(attempt: u32, last_error: Option<&error::Error>) -> Duration {
    if let Some(error::Error::RateLimited {
        retry_after_ms, ..
    }) = last_error
    {
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
            .field("base_url", &self.base_url)
            .field("api_version", &self.api_version)
            .field("max_retries", &self.max_retries)
            .finish_non_exhaustive()
    }
}
