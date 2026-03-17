//! Model constants and API configuration defaults.

/// Default Anthropic API base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Default Anthropic API version header value.
pub const DEFAULT_API_VERSION: &str = "2023-06-01";

/// Default maximum retry attempts for transient failures.
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Retry backoff base delay in milliseconds.
pub const BACKOFF_BASE_MS: u64 = 1000;

/// Retry backoff multiplier per attempt.
pub const BACKOFF_FACTOR: u64 = 2;

/// Maximum retry backoff delay in milliseconds.
pub const BACKOFF_MAX_MS: u64 = 30_000;

/// All supported Anthropic model identifiers.
///
/// Includes both short names (e.g., `claude-opus-4-6`) and dated snapshots
/// (e.g., `claude-opus-4-20250514`).
pub static SUPPORTED_MODELS: &[&str] = &[
    "claude-opus-4-6",
    "claude-opus-4-20250514",
    "claude-sonnet-4-6",
    "claude-sonnet-4-20250514",
    "claude-haiku-4-5",
    "claude-haiku-4-5-20251001",
];

/// Short model name constants for use in config and code.
pub mod names {
    pub const OPUS: &str = "claude-opus-4-6";
    pub const SONNET: &str = "claude-sonnet-4-6";
    pub const HAIKU: &str = "claude-haiku-4-5-20251001";
}
