//! Shared HTTP constants used across Aletheia crates.

/// `application/json` content type.
pub const CONTENT_TYPE_JSON: &str = "application/json";

/// `text/event-stream` content type for SSE responses.
pub const CONTENT_TYPE_EVENT_STREAM: &str = "text/event-stream";

/// Bearer token prefix including the trailing space.
pub const BEARER_PREFIX: &str = "Bearer ";

/// API v1 route prefix.
pub const API_V1: &str = "/api/v1";

/// Health check endpoint path.
pub const API_HEALTH: &str = "/api/health";
