//! Shared HTTP constants used across Aletheia crates.

/// MIME type for JSON request and response bodies.
pub const APPLICATION_JSON: &str = "application/json";

/// MIME type for Server-Sent Event streams.
pub const TEXT_EVENT_STREAM: &str = "text/event-stream";

/// MIME type for URL-encoded form data.
pub const APPLICATION_FORM_URLENCODED: &str = "application/x-www-form-urlencoded";

/// Prefix for Bearer token values in the Authorization header.
pub const BEARER_PREFIX: &str = "Bearer ";

/// Header name for the Anthropic API key.
pub const HEADER_X_API_KEY: &str = "x-api-key";

/// Header name for the Anthropic API version.
pub const HEADER_ANTHROPIC_VERSION: &str = "anthropic-version";
