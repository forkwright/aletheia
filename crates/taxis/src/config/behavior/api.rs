//! Pylon API limits configuration.

use serde::{Deserialize, Serialize};

/// Pylon API request size and idempotency cache limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ApiLimitsConfig {
    /// Maximum characters in a session name. Default: 255.
    pub max_session_name_len: usize,
    /// Maximum bytes in a session identifier. Default: 256.
    pub max_identifier_bytes: usize,
    /// Maximum messages returned by the history endpoint. Default: 1000.
    pub max_history_limit: u32,
    /// Default messages returned by the history endpoint. Default: 50.
    pub default_history_limit: u32,
    /// Maximum bytes per streaming message body. Default: 262144 (256 KiB).
    pub max_message_bytes: usize,
    /// Maximum facts returned by a single knowledge list request. Default: 1000.
    pub max_facts_limit: usize,
    /// Maximum results for a single knowledge search request. Default: 1000.
    pub max_search_limit: usize,
    /// Maximum facts in a single bulk-import request. Default: 1000.
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
    pub clock_skew_leeway_secs: u64,
    /// Time in seconds before token expiry that triggers a warning. Default: 3600.
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
