//! Model constants and API configuration defaults.

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

/// All supported Anthropic model identifiers.
///
/// Includes both short names (e.g., `claude-opus-4-6`) and dated snapshots
/// (e.g., `claude-opus-4-20250514`).
pub static SUPPORTED_MODELS: &[&str] = &[
    // kanon:ignore RUST/pub-visibility
    "claude-opus-4-6",
    "claude-opus-4-20250514",
    "claude-sonnet-4-6",
    "claude-sonnet-4-20250514",
    "claude-haiku-4-5",
    "claude-haiku-4-5-20251001",
];

/// Short model name constants for use in config and code.
pub mod names {
    // kanon:ignore RUST/pub-visibility
    /// Default Opus model alias.
    pub const OPUS: &str = "claude-opus-4-6"; // kanon:ignore RUST/pub-visibility
    /// Default Sonnet model alias.
    pub const SONNET: &str = "claude-sonnet-4-6"; // kanon:ignore RUST/pub-visibility
    /// Default Haiku model alias.
    pub const HAIKU: &str = "claude-haiku-4-5-20251001"; // kanon:ignore RUST/pub-visibility
}
