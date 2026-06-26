#![deny(clippy::unwrap_used)]
#![deny(missing_docs)]
//! Pylon (πυλών): "gateway." Routes HTTP and SSE requests to the agent pipeline.

/// Per-turn approval-decision sender registry (#3958, ADR-005).
pub mod approval_registry;
/// Typed HTTP client for first-party gateway consumers.
pub mod client;
/// Runtime state for pylon-managed provider credentials (#4872).
pub mod credential_runtime;
/// Service discovery file writer (Tailscale IP + file-based announcement).
pub mod discovery;
/// API error types with Axum HTTP status code mapping.
pub(crate) mod error;
/// In-process broadcast bus for domain events.
pub mod event_bus;
/// JWT auth extractor for Bearer token validation.
pub mod extract;
/// HTTP request handlers for health, nous, and session endpoints.
pub mod handlers;
/// Idempotency-key cache for deduplicating message sends on retry.
pub mod idempotency;
/// Meta-insights computation: anomaly detection and metric aggregation.
pub mod insights;
/// Prometheus metrics collection and exposure.
pub mod metrics;
/// Custom Axum middleware layers (CSRF protection, request ID, error enrichment, HTTP metrics).
pub mod middleware;
/// OpenAPI specification generation via utoipa.
pub(crate) mod openapi;
/// Cursor-based pagination types for list endpoints.
pub mod pagination;
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
/// In-memory turn event buffer for SSE client recovery after connection drops.
pub mod turn_buffer;
/// Shared API request and response types.
pub mod types;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod assertions {
    const _: fn() = || {
        fn assert<T: Send + Sync>() {}
        assert::<super::state::AppState>();
    };
}
