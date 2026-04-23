//! `ETag` middleware: computes strong `ETags` from GET response bodies and
//! supports conditional requests via `If-None-Match`.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::response::Response;
use http_body_util::BodyStream;
use sha2::{Digest, Sha256};
use tokio_stream::Stream;
use tower::{Layer, Service};
use tracing::trace;

/// Tower layer that adds strong `ETag` headers to GET responses and supports
/// `If-None-Match` conditional requests.
///
/// # Escape hatch
///
/// The middleware automatically skips:
/// - Non-GET requests.
/// - Routes whose path matches known SSE endpoints (`/api/v1/events`,
///   `/api/v1/sessions/{id}/turns/{turn_id}/events`).
/// - Responses with `Content-Type: text/event-stream`.
#[derive(Debug, Clone, Default)]
pub struct ETagLayer;

impl ETagLayer {
    /// Create a new `ETag` layer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for ETagLayer {
    type Service = ETagService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ETagService { inner }
    }
}

/// Tower service that computes `ETags` for GET responses.
#[derive(Debug, Clone)]
pub struct ETagService<S> {
    inner: S,
}

impl<S> Service<Request> for ETagService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    #[tracing::instrument(skip(self, request), fields(http.method = %request.method(), http.path = %request.uri().path()))]
    fn call(&mut self, request: Request) -> Self::Future {
        let is_get = request.method() == axum::http::Method::GET;
        let path = request.uri().path().to_owned();
        let if_none_match = request
            .headers()
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let mut inner = self.inner.clone();

        Box::pin(async move {
            let response = inner.call(request).await?;

            // Only apply to GET 200 OK responses.
            if !is_get || response.status() != StatusCode::OK {
                return Ok(response);
            }

            // Skip SSE responses (defense in depth: path + content-type).
            if is_sse_path(&path) || is_sse_response(&response) {
                return Ok(response);
            }

            // Collect body and compute hash in a single pass.
            let (parts, body) = response.into_parts();
            let (bytes, etag) = match hash_body(body).await {
                Ok(v) => v,
                Err(e) => {
                    trace!(error = %e, "failed to read response body for ETag");
                    return Ok(Response::from_parts(parts, Body::empty()));
                }
            };

            // Check conditional request.
            if let Some(ref client_etag) = if_none_match
                && strong_etag_matches(client_etag, &etag)
            {
                trace!(etag = %etag, "ETag match, returning 304 Not Modified");
                let mut not_modified = Response::new(Body::empty());
                *not_modified.status_mut() = StatusCode::NOT_MODIFIED;
                if let Ok(hv) = etag.parse() {
                    not_modified.headers_mut().insert(header::ETAG, hv);
                }
                return Ok(not_modified);
            }

            let mut response = Response::from_parts(parts, Body::from(bytes));
            if let Ok(hv) = etag.parse() {
                response.headers_mut().insert(header::ETAG, hv);
            }
            Ok(response)
        })
    }
}

/// Return `true` if the request path is a known SSE endpoint.
///
/// These paths are excluded because buffering a streaming response would
/// break the SSE contract.
fn is_sse_path(path: &str) -> bool {
    path == "/api/v1/events"
        || (path.starts_with("/api/v1/sessions/")
            && path.contains("/turns/")
            && path.ends_with("/events"))
}

/// Return `true` if the response already has an SSE content type.
fn is_sse_response(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/event-stream"))
}

/// Consume a body, hashing each chunk as it arrives, and return the collected
/// bytes plus the hexadecimal strong `ETag`.
///
/// WHY: This performs hashing during the single read so the body is never
/// buffered and then hashed in a second pass.
async fn hash_body(body: Body) -> Result<(Bytes, String), axum::Error> {
    let mut hasher = Sha256::new();
    let mut buf = Vec::new();

    let mut stream = BodyStream::new(body);
    loop {
        let next = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await;
        match next {
            None => break,
            Some(Ok(frame)) => {
                if let Some(data) = frame.data_ref() {
                    hasher.update(data);
                    buf.extend_from_slice(data);
                }
            }
            Some(Err(e)) => return Err(e),
        }
    }

    let digest = hasher.finalize();
    let hex = digest.iter().fold(String::new(), |mut acc, b| {
        let _ = std::fmt::Write::write_fmt(&mut acc, format_args!("{b:02x}"));
        acc
    });
    let etag = format!("\"{hex}\"");
    Ok((Bytes::from(buf), etag))
}

/// Strong comparison for `ETags`.
///
/// Returns `true` only when both `ETags` are strong (no `W/` prefix) and
/// byte-identical.
fn strong_etag_matches(client_etag: &str, server_etag: &str) -> bool {
    let client = client_etag.trim();
    let server = server_etag.trim();

    // Weak ETags never match strongly.
    if client.starts_with("W/") || server.starts_with("W/") {
        return false;
    }

    client == server
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn sse_path_matches_events() {
        assert!(is_sse_path("/api/v1/events"));
    }

    #[test]
    fn sse_path_matches_turn_events() {
        assert!(is_sse_path("/api/v1/sessions/abc/turns/123/events"));
    }

    #[test]
    fn sse_path_does_not_match_regular_get() {
        assert!(!is_sse_path("/api/v1/nous"));
        assert!(!is_sse_path("/api/v1/sessions"));
        assert!(!is_sse_path("/api/health"));
    }

    #[test]
    fn strong_match_requires_exact_equality() {
        assert!(strong_etag_matches("\"abc\"", "\"abc\""));
        assert!(!strong_etag_matches("\"abc\"", "\"def\""));
    }

    #[test]
    fn weak_etag_never_matches_strongly() {
        assert!(!strong_etag_matches("W/\"abc\"", "\"abc\""));
        assert!(!strong_etag_matches("\"abc\"", "W/\"abc\""));
    }

    #[tokio::test]
    async fn hash_body_produces_stable_etag() {
        let body = Body::from("hello world");
        let (bytes, etag) = hash_body(body).await.unwrap();
        assert_eq!(bytes, "hello world");
        assert!(etag.starts_with('\"') && etag.ends_with('\"'));
    }
}
