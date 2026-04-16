//! Pylon API limits configuration.

use serde::{Deserialize, Serialize};

/// Pylon API request size and idempotency cache limits.
///
/// All defaults match the current hardcoded constants in the `pylon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ApiLimitsConfig {
    /// Maximum characters in a session name. Default: 255.
    /// Mirrors `pylon::handlers::sessions::MAX_SESSION_NAME_LEN`.
    pub max_session_name_len: usize,
    /// Maximum bytes in a session identifier. Default: 256.
    /// Mirrors `pylon::handlers::sessions::MAX_IDENTIFIER_BYTES`.
    pub max_identifier_bytes: usize,
    /// Maximum messages returned by the history endpoint. Default: 1000.
    /// Mirrors `pylon::handlers::sessions::MAX_HISTORY_LIMIT`.
    pub max_history_limit: u32,
    /// Default messages returned by the history endpoint. Default: 50.
    /// Mirrors `pylon::handlers::sessions::DEFAULT_HISTORY_LIMIT`.
    pub default_history_limit: u32,
    /// Maximum bytes per streaming message body. Default: 262144 (256 KiB).
    /// Mirrors `pylon::handlers::sessions::streaming::MAX_MESSAGE_BYTES`.
    pub max_message_bytes: usize,
    /// Maximum facts returned by a single knowledge list request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_FACTS_LIMIT`.
    pub max_facts_limit: usize,
    /// Maximum results for a single knowledge search request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_SEARCH_LIMIT`.
    pub max_search_limit: usize,
    /// Maximum facts in a single bulk-import request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::bulk_import::MAX_IMPORT_BATCH_SIZE`.
    pub max_import_batch_size: usize,
    /// TTL in seconds for idempotency key cache entries. Default: 300.
    /// Mirrors `pylon::idempotency::DEFAULT_TTL`.
    pub idempotency_ttl_secs: u64,
    /// Maximum idempotency cache entries (LRU cap). Default: 10000.
    /// Mirrors `pylon::idempotency::DEFAULT_CAPACITY`.
    pub idempotency_capacity: usize,
    /// Maximum character length of an idempotency key. Default: 64.
    pub idempotency_max_key_length: usize,
    /// Acceptable clock skew in seconds before token expiry check warns. Default: 30.
    /// Mirrors `pylon::handlers::health::CLOCK_SKEW_LEEWAY`.
    pub clock_skew_leeway_secs: u64,
    /// Time in seconds before token expiry that triggers a warning. Default: 3600.
    /// Mirrors `pylon::handlers::health::EXPIRY_WARNING_THRESHOLD`.
    pub expiry_warning_threshold_secs: u64,
}

impl Default for ApiLimitsConfig {
    fn default() -> Self {
        Self {
            max_session_name_len: 255,
            max_identifier_bytes: 256,
            max_history_limit: 1_000,
            default_history_limit: 50,
            max_message_bytes: 262_144,
            max_facts_limit: 1_000,
            max_search_limit: 1_000,
            max_import_batch_size: 1_000,
            idempotency_ttl_secs: 300,
            idempotency_capacity: 10_000,
            idempotency_max_key_length: 64,
            clock_skew_leeway_secs: 30,
            expiry_warning_threshold_secs: 3_600,
        }
    }
}
