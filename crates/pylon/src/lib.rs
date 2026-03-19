#![deny(missing_docs)]
//! Pylon (πυλών): "gateway." Routes HTTP and SSE requests to the agent pipeline.

/// API error types with Axum HTTP status code mapping.
pub(crate) mod error;
/// JWT auth extractor for Bearer token validation.
pub mod extract;
/// HTTP request handlers for health, nous, and session endpoints.
pub mod handlers;
/// Idempotency-key cache for deduplicating message sends on retry.
pub mod idempotency;
/// Prometheus metrics collection and exposure.
pub(crate) mod metrics;
/// Custom Axum middleware layers (CSRF protection, request ID, error enrichment, HTTP metrics).
pub mod middleware;
/// OpenAPI specification generation via utoipa.
pub(crate) mod openapi;
/// Axum router construction with CORS, auth, and body-limit layers.
pub mod router;
/// Security configuration derived from the gateway config section.
pub mod security;
/// Server entry point with TLS support and graceful shutdown.
pub mod server;
/// Shared application state accessible across all handlers.
pub mod state;
/// SSE event types for streaming agent responses to clients.
pub mod stream;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(super::state::AppState: Send, Sync);
}
