//! OpenAI-compatible error mapping to hermeneus error variants.
//!
//! Translates HTTP and transport errors from OpenAI-shaped endpoints
//! (OpenAI proper, llama.cpp `--server`, ollama, vllm) into
//! [`crate::error::Error`]. The OpenAI error envelope is simpler than
//! Anthropic's: `{"error": {"message": ..., "type": ..., "code": ...}}`.

use reqwest::Response;
use serde::Deserialize;

use crate::error::{self, ApiErrorContext};

/// Wire shape of an OpenAI error response body.
///
/// Field set matches the documented OpenAI error envelope. Also accepted
/// verbatim by llama.cpp / ollama / vllm since they ape the OpenAI API.
#[derive(Debug, Deserialize)]
pub(crate) struct WireErrorResponse {
    pub error: WireErrorDetail,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireErrorDetail {
    pub message: String,
    #[serde(default, rename = "type")]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "captured for debugging; not dispatched on yet")
    )]
    pub error_type: Option<String>,
    #[serde(default)]
    #[expect(dead_code, reason = "captured for debugging; not dispatched on yet")]
    pub code: Option<String>,
}

/// Map an HTTP response with a non-success status to a hermeneus error.
///
/// Consumes the response body to extract the OpenAI-style error detail.
/// Falls back to a synthetic `HTTP {status}` message if the body is absent
/// or not parseable — llama.cpp in particular returns plain-text errors for
/// some failure modes.
#[tracing::instrument(skip_all)]
pub(crate) async fn map_error_response(
    response: Response,
    model: &str,
    credential_source: &str,
) -> error::Error {
    let status = response.status().as_u16();
    let retry_after_ms = extract_retry_after(&response);

    let body = match response.text().await {
        Ok(body) => body,
        Err(err) => {
            tracing::debug!(error = %err, "failed to read OpenAI error response body");
            String::new()
        }
    };

    let detail = serde_json::from_str::<WireErrorResponse>(&body)
        .ok()
        .map(|e| e.error.message);

    let message = detail.unwrap_or_else(|| {
        if body.is_empty() {
            format!("HTTP {status}")
        } else {
            // WHY: bounded slice so we do not paste a multi-megabyte HTML
            // error page into the error chain.
            let trimmed: String = body.chars().take(512).collect();
            format!("HTTP {status}: {trimmed}")
        }
    });

    tracing::warn!(
        status,
        model,
        credential_source,
        "OpenAI-compatible API error response"
    );

    match status {
        401 | 403 => error::AuthFailedSnafu { message }.build(),
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

/// Extract `retry-after` header value as milliseconds.
fn extract_retry_after(response: &Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(|secs| secs * 1000)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_error_envelope() {
        let body = r#"{"error":{"message":"Invalid API key","type":"invalid_request_error","code":"invalid_api_key"}}"#;
        let parsed: WireErrorResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.error.message, "Invalid API key");
        assert_eq!(
            parsed.error.error_type.as_deref(),
            Some("invalid_request_error")
        );
    }

    #[test]
    fn parses_llama_cpp_error_envelope() {
        // llama.cpp omits `code` and `type` on some errors.
        let body = r#"{"error":{"message":"context length exceeded"}}"#;
        let parsed: WireErrorResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.error.message, "context length exceeded");
        assert!(parsed.error.error_type.is_none());
    }
}
