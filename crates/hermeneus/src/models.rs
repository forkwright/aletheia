//! API configuration defaults and model-catalog re-exports.

/// Default Anthropic API base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com"; // kanon:ignore RUST/pub-visibility

/// Default Anthropic API version header value.
pub const DEFAULT_API_VERSION: &str = "2023-06-01"; // kanon:ignore RUST/pub-visibility

/// Default maximum retry attempts for transient failures.
pub use koina::defaults::DEFAULT_MAX_RETRIES;

/// Retry backoff base delay in milliseconds.
pub use koina::defaults::BACKOFF_BASE_MS;

/// Retry backoff multiplier per attempt.
pub use koina::defaults::BACKOFF_FACTOR;

/// Maximum retry backoff delay in milliseconds.
pub use koina::defaults::BACKOFF_MAX_MS;

pub use koina::models::names;
