//! API configuration defaults and model-catalog re-exports.

/// Default Anthropic API base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com"; // kanon:ignore RUST/pub-visibility

/// Default Anthropic API version header value.
pub const DEFAULT_API_VERSION: &str = "2023-06-01"; // kanon:ignore RUST/pub-visibility

/// Default maximum retry attempts for transient failures.
pub const DEFAULT_MAX_RETRIES: u32 = 3; // kanon:ignore RUST/pub-visibility

/// Retry backoff base delay in milliseconds.
pub const BACKOFF_BASE_MS: u64 = 1000; // kanon:ignore RUST/pub-visibility

/// Retry backoff multiplier per attempt.
pub const BACKOFF_FACTOR: u64 = 2; // kanon:ignore RUST/pub-visibility

/// Maximum retry backoff delay in milliseconds.
pub const BACKOFF_MAX_MS: u64 = 30_000; // kanon:ignore RUST/pub-visibility

pub use koina::models::names;
