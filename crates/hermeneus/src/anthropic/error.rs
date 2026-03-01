//! Anthropic API error mapping to hermeneus error variants.

use reqwest::blocking::Response;
use snafu::ResultExt;

use super::wire::WireErrorResponse;
use crate::error::{self, Result};

/// Map an HTTP response with a non-success status to a hermeneus error.
///
/// Consumes the response body to extract the Anthropic error detail.
pub(crate) fn map_error_response(response: Response) -> error::Error {
    let status = response.status().as_u16();
    let retry_after_ms = extract_retry_after(&response);

    let detail = response
        .text()
        .ok()
        .and_then(|body| serde_json::from_str::<WireErrorResponse>(&body).ok())
        .map(|e| e.error.message);

    let message = detail.unwrap_or_else(|| format!("HTTP {status}"));

    match status {
        401 => error::AuthFailedSnafu { message }.build(),
        429 => error::RateLimitedSnafu {
            retry_after_ms: retry_after_ms.unwrap_or(1000),
        }
        .build(),
        _ => error::ApiSnafu { status, message }.build(),
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
