//! Shared HTTP constants used across Aletheia crates.

/// `application/json` content type.
pub const CONTENT_TYPE_JSON: &str = "application/json"; // kanon:ignore RUST/pub-visibility

/// `text/event-stream` content type for SSE responses.
pub const CONTENT_TYPE_EVENT_STREAM: &str = "text/event-stream"; // kanon:ignore RUST/pub-visibility

/// Bearer token prefix including the trailing space.
pub const BEARER_PREFIX: &str = "Bearer "; // kanon:ignore RUST/pub-visibility

/// API v1 route prefix.
pub const API_V1: &str = "/api/v1"; // kanon:ignore RUST/pub-visibility

/// Health check endpoint path.
pub const API_HEALTH: &str = "/api/health"; // kanon:ignore RUST/pub-visibility
