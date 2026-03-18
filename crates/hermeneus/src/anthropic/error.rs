//! Anthropic API error mapping to hermeneus error variants.

use reqwest::Response;
use snafu::ResultExt;
use tracing::warn;

use super::wire::WireErrorResponse;
use crate::error::{self, ApiErrorContext, Result};

/// Map an HTTP response with a non-success status to a hermeneus error.
///
/// Consumes the response body to extract the Anthropic error detail.
/// Logs the full raw body, model, token prefix, credential source, and
/// `x-request-id` at WARN level before parsing so operators can diagnose
/// opaque errors (e.g. OAuth token with unknown model alias returning "Error").
pub(crate) async fn map_error_response(
    response: Response,
    model: &str,
    token_prefix: &str,
    credential_source: &str,
) -> error::Error {
    let status = response.status().as_u16();
    let retry_after_ms = extract_retry_after(&response);
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let body = response.text().await.unwrap_or_default();

    warn!(
        status,
        model,
        token_prefix,
        credential_source,
        request_id = request_id.as_deref().unwrap_or(""),
        body = %body,
        "Anthropic API error response"
    );

    let detail = serde_json::from_str::<WireErrorResponse>(&body)
        .ok()
        .map(|e| e.error.message);

    let message = detail.unwrap_or_else(|| format!("HTTP {status}"));

    match status {
        401 => error::AuthFailedSnafu { message }.build(),
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

/// Default backoff for SSE overload/rate-limit errors (no retry-after header available).
const SSE_DEFAULT_RETRY_MS: u64 = 1000;

/// Map an SSE stream error event to a hermeneus error.
///
/// Unlike HTTP errors, SSE errors arrive inside a 200 response body.
/// The error type string determines retryability:
/// - `overloaded_error` / `rate_limit_error` → `RateLimited` (retryable)
/// - Everything else → `ApiError` (not retried)
pub(crate) fn map_sse_error(detail: super::wire::WireErrorDetail) -> crate::error::Error {
    match detail.error_type.as_str() {
        "overloaded_error" | "rate_limit_error" => crate::error::RateLimitedSnafu {
            retry_after_ms: SSE_DEFAULT_RETRY_MS,
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
    fn overloaded_error_maps_to_rate_limited() {
        let detail = super::super::wire::WireErrorDetail {
            error_type: "overloaded_error".to_owned(),
            message: "Overloaded".to_owned(),
        };
        let err = map_sse_error(detail);
        assert!(
            matches!(
                err,
                Error::RateLimited {
                    retry_after_ms: 1000,
                    ..
                }
            ),
            "expected RateLimited, got: {err:?}"
        );
    }

    #[test]
    fn rate_limit_error_maps_to_rate_limited() {
        let detail = super::super::wire::WireErrorDetail {
            error_type: "rate_limit_error".to_owned(),
            message: "Rate limited".to_owned(),
        };
        let err = map_sse_error(detail);
        assert!(matches!(err, Error::RateLimited { .. }));
    }

    #[test]
    fn unknown_error_maps_to_api_error() {
        let detail = super::super::wire::WireErrorDetail {
            error_type: "invalid_request_error".to_owned(),
            message: "bad input".to_owned(),
        };
        let err = map_sse_error(detail);
        assert!(
            matches!(err, Error::ApiError { status: 0, .. }),
            "expected ApiError, got: {err:?}"
        );
    }
}
