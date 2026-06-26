//! Anthropic API error mapping to hermeneus error variants.

use reqwest::Response;
use snafu::ResultExt;
use tracing::warn;

use super::wire::WireErrorResponse;
use crate::error::{self, ApiErrorContext, Result};

/// Maximum bytes of a provider error body preserved in logs.
///
/// WHY(#4885): Raw provider error bodies can contain prompt fragments or tool
/// payloads. Truncating to 512 bytes captures enough for diagnostics while
/// bounding the maximum log line size.
const MAX_ERROR_BODY_LOG_BYTES: usize = 512;

/// Map an HTTP response with a non-success status to a hermeneus error.
///
/// Consumes the response body to extract the Anthropic error detail.
/// Logs status, model, credential source class, request-id, and a truncated
/// body at WARN; no credential-derived token material is logged.
#[tracing::instrument(skip_all)]
pub(crate) async fn map_error_response(
    response: Response,
    model: &str,
    credential_source: &str,
) -> error::Error {
    let status = response.status().as_u16();
    let retry_after_ms = extract_retry_after(&response);
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let body = match response.text().await {
        Ok(text) => text,
        Err(e) => {
            warn!(error = %e, "failed to read error response body");
            String::new()
        }
    };

    // WHY(#4885): truncate before logging; raw bodies can contain prompt
    // or tool-payload fragments. No token prefix is logged.
    let body_truncated = crate::secret::truncate_error_body(&body, MAX_ERROR_BODY_LOG_BYTES);

    warn!(
        status,
        model,
        credential_source,
        request_id = request_id.as_deref().unwrap_or(""),
        body = %body_truncated,
        "Anthropic API error response"
    );

    let detail = serde_json::from_str::<WireErrorResponse>(&body)
        .ok()
        .map(|e| e.error.message);

    let message = detail.map_or_else(
        || format!("HTTP {status}"),
        |message| crate::secret::truncate_error_body(&message, MAX_ERROR_BODY_LOG_BYTES),
    );

    match status {
        401 => error::AuthFailedSnafu { message }.build(),
        // TODO(#2183): fallback 1000ms when no retry-after header present.
        // Most 429s include the header; this covers edge cases where it's absent.
        429 => error::RateLimitedSnafu {
            retry_after_ms: retry_after_ms.unwrap_or(1000),
        }
        .build(),
        _ => error::ApiSnafu {
            status,
            message,
            context: Box::new(ApiErrorContext {
                model: model.to_owned(),
                credential_source: credential_source.to_owned(),
            }),
        }
        .build(),
    }
}

/// Map a reqwest transport error to a hermeneus error.
pub(crate) fn map_request_error(err: &reqwest::Error) -> error::Error {
    error::ApiRequestSnafu {
        message: err.to_string(),
    }
    .build()
}

/// Parse a response body as JSON, mapping parse failures to `ParseResponse`.
pub(crate) fn parse_response_body<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    serde_json::from_str(body).context(error::ParseResponseSnafu)
}

/// Extract `retry-after` header value as milliseconds.
fn extract_retry_after(response: &Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(|secs| secs * 1000)
}

/// Default retry delay for SSE stream rate-limit and overload errors.
///
/// Used as the fallback when `providerBehavior.sseDefaultRetryMs` is not
/// configured. Exposed `pub` so `taxis::config::ProviderBehaviorConfig` can use
/// it as the field default when constructing from config without a behavior override.
pub const SSE_DEFAULT_RETRY_MS: u64 = 1000;

/// Map an SSE stream error event to a hermeneus error.
///
/// Unlike HTTP errors, SSE errors arrive inside a 200 response body.
/// The error type string determines retryability:
/// - `overloaded_error` / `rate_limit_error` → `RateLimited` (retryable)
/// - Everything else → `ApiError` (not retried)
///
/// `sse_retry_ms` is the configured fallback delay for rate-limit/overload
/// errors when no Retry-After header is present (#4886).
pub(crate) fn map_sse_error(
    detail: super::wire::WireErrorDetail,
    sse_retry_ms: u64,
) -> crate::error::Error {
    match detail.error_type.as_str() {
        "overloaded_error" | "rate_limit_error" => crate::error::RateLimitedSnafu {
            retry_after_ms: sse_retry_ms,
        }
        .build(),
        _ => crate::error::ApiSnafu {
            status: 0_u16,
            message: detail.message,
            context: ApiErrorContext::empty(),
        }
        .build(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn overloaded_error_maps_to_rate_limited_with_default_delay() {
        let detail = super::super::wire::WireErrorDetail {
            error_type: "overloaded_error".to_owned(),
            message: "Overloaded".to_owned(),
        };
        let err = map_sse_error(detail, SSE_DEFAULT_RETRY_MS);
        assert!(
            matches!(
                err,
                Error::RateLimited {
                    retry_after_ms: 1000,
                    ..
                }
            ),
            "expected RateLimited with 1000ms, got: {err:?}"
        );
    }

    #[test]
    fn overloaded_error_maps_to_rate_limited_with_configured_delay() {
        // WHY(#4886): verify the configured delay is honored, not the hardcoded default.
        let detail = super::super::wire::WireErrorDetail {
            error_type: "overloaded_error".to_owned(),
            message: "Overloaded".to_owned(),
        };
        let err = map_sse_error(detail, 5000);
        assert!(
            matches!(
                err,
                Error::RateLimited {
                    retry_after_ms: 5000,
                    ..
                }
            ),
            "expected RateLimited with 5000ms, got: {err:?}"
        );
    }

    #[test]
    fn rate_limit_error_maps_to_rate_limited() {
        let detail = super::super::wire::WireErrorDetail {
            error_type: "rate_limit_error".to_owned(),
            message: "Rate limited".to_owned(),
        };
        let err = map_sse_error(detail, SSE_DEFAULT_RETRY_MS);
        assert!(matches!(err, Error::RateLimited { .. }));
    }

    #[test]
    fn unknown_error_maps_to_api_error() {
        let detail = super::super::wire::WireErrorDetail {
            error_type: "invalid_request_error".to_owned(),
            message: "bad input".to_owned(),
        };
        let err = map_sse_error(detail, SSE_DEFAULT_RETRY_MS);
        assert!(
            matches!(err, Error::ApiError { status: 0, .. }),
            "expected ApiError, got: {err:?}"
        );
    }
}
