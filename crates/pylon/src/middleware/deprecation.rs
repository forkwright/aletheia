//! Deprecation middleware: injects `Deprecation` and `Sunset` headers per RFC 8594.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::Response;
use jiff::Timestamp;
use tower::{Layer, Service};
use tracing::warn;

/// Metadata for a deprecated endpoint.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DeprecationInfo {
    /// Unix timestamp when the endpoint was deprecated.
    pub deprecated_at: Timestamp,
    /// Unix timestamp when the endpoint will be removed.
    pub sunset_at: Timestamp,
    /// Optional URL to a migration guide.
    pub link: Option<String>,
}

impl DeprecationInfo {
    /// Create a new deprecation entry.
    #[must_use]
    pub fn new(deprecated_at: Timestamp, sunset_at: Timestamp, link: Option<String>) -> Self {
        Self {
            deprecated_at,
            sunset_at,
            link,
        }
    }
}

/// Map from route pattern (`METHOD /path`) to deprecation metadata.
#[derive(Debug, Clone, Default)]
pub struct DeprecationMap {
    inner: HashMap<String, DeprecationInfo>,
}

/// Register a deprecation for a route pattern.
///
/// Returns a `(pattern, info)` tuple suitable for passing to
/// [`DeprecationLayer::new`].
#[must_use]
pub fn deprecate(
    pattern: impl Into<String>,
    deprecated_at: Timestamp,
    sunset_at: Timestamp,
    link: Option<String>,
) -> (String, DeprecationInfo) {
    (
        pattern.into(),
        DeprecationInfo::new(deprecated_at, sunset_at, link),
    )
}

/// Tower layer that injects `Deprecation` and `Sunset` headers on matching
/// responses, per RFC 8594.
#[derive(Debug, Clone)]
pub struct DeprecationLayer {
    deprecations: Arc<DeprecationMap>,
}

impl DeprecationLayer {
    /// Create a new deprecation layer from a list of registered deprecations.
    #[must_use]
    pub fn new(deprecations: impl IntoIterator<Item = (String, DeprecationInfo)>) -> Self {
        let mut map = DeprecationMap::default();
        for (pattern, info) in deprecations {
            map.inner.insert(pattern, info);
        }
        Self {
            deprecations: Arc::new(map),
        }
    }
}

impl<S> Layer<S> for DeprecationLayer {
    type Service = DeprecationService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DeprecationService {
            inner,
            deprecations: Arc::clone(&self.deprecations),
        }
    }
}

/// Tower service that adds deprecation headers to responses.
#[derive(Debug, Clone)]
pub struct DeprecationService<S> {
    inner: S,
    deprecations: Arc<DeprecationMap>,
}

impl<S> Service<Request> for DeprecationService<S>
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

    fn call(&mut self, request: Request) -> Self::Future {
        let method = request.method().to_string();
        let key = format!("{method} {}", request.uri().path());
        let info = self.deprecations.inner.get(&key).cloned();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let mut response = inner.call(request).await?;

            if let Some(info) = info {
                let span = tracing::info_span!(
                    "deprecation_middleware",
                    http.path = %key,
                    http.method = %method,
                );
                let _guard = span.enter();

                // RFC 8594: Deprecation: @<unix-timestamp>
                let deprecation_value = format!("@{}", info.deprecated_at.as_second());
                if let Ok(header) = axum::http::HeaderValue::from_str(&deprecation_value) {
                    response
                        .headers_mut()
                        .insert(axum::http::HeaderName::from_static("deprecation"), header);
                }

                // RFC 8594: Sunset: <HTTP-date> (RFC 7231)
                let sunset = info
                    .sunset_at
                    .strftime("%a, %d %b %Y %H:%M:%S GMT")
                    .to_string();
                if let Ok(header) = axum::http::HeaderValue::from_str(&sunset) {
                    response
                        .headers_mut()
                        .insert(axum::http::HeaderName::from_static("sunset"), header);
                }

                // RFC 8594: Link: <url>; rel="deprecation"
                if let Some(link) = info.link {
                    let link_value = format!("<{link}>; rel=\"deprecation\"");
                    if let Ok(header) = axum::http::HeaderValue::from_str(&link_value) {
                        response
                            .headers_mut()
                            .insert(axum::http::HeaderName::from_static("link"), header);
                    }
                }

                warn!(path = %key, "request to deprecated endpoint");
            }

            Ok(response)
        })
    }
}
