//! HTTP request handlers.

/// Runtime configuration read/write.
pub mod config;
/// System health and readiness check.
pub mod health;
/// Prometheus metrics exposition endpoint.
pub mod metrics;
/// Nous agent listing and status inspection.
pub mod nous;
/// Session lifecycle, history retrieval, and SSE message streaming.
pub mod sessions;
