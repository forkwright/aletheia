//! Agora messaging transport configuration.

use serde::{Deserialize, Serialize};

/// Agora messaging transport poll, buffer, circuit-breaker, and RPC settings.
///
/// All defaults match the current hardcoded constants in the `agora` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MessagingConfig {
    /// How often Semeion polls for new channel messages in milliseconds. Default: 2000.
    /// Mirrors `agora::semeion::DEFAULT_POLL_INTERVAL`.
    pub poll_interval_ms: u64,
    /// Inbound message buffer size per channel. Default: 100.
    /// Mirrors `agora::semeion::DEFAULT_BUFFER_CAPACITY`.
    pub buffer_capacity: usize,
    /// Consecutive channel errors before the channel is halted. Default: 5.
    /// Mirrors `agora::semeion::CIRCUIT_BREAKER_THRESHOLD`.
    pub circuit_breaker_threshold: u32,
    /// How often a halted channel is health-checked in seconds. Default: 60.
    /// Mirrors `agora::semeion::HALTED_HEALTH_CHECK_INTERVAL`.
    pub halted_health_check_interval_secs: u64,
    /// Timeout in seconds for Semeion RPC calls. Default: 10.
    /// Mirrors `agora::semeion::client::RPC_TIMEOUT`.
    pub rpc_timeout_secs: u64,
    /// Timeout in seconds for Semeion health-check requests. Default: 2.
    /// Mirrors `agora::semeion::client::HEALTH_TIMEOUT`.
    pub health_timeout_secs: u64,
    /// Timeout in seconds waiting to receive a Semeion response. Default: 15.
    /// Mirrors `agora::semeion::client::RECEIVE_TIMEOUT`.
    pub receive_timeout_secs: u64,
    /// Default timeout in seconds for agent-dispatch tool calls. Default: 300.
    /// Mirrors `organon::builtins::agent::DEFAULT_TIMEOUT_SECS`.
    pub agent_dispatch_timeout_secs: u64,
    /// Maximum concurrent inbound-message handler tasks. Default: 64.
    /// Mirrors `agora::listener::ChannelListener::MAX_CONCURRENT_HANDLERS`.
    pub max_concurrent_handlers: usize,
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 2_000,
            buffer_capacity: 100,
            circuit_breaker_threshold: 5,
            halted_health_check_interval_secs: 60,
            rpc_timeout_secs: 10,
            health_timeout_secs: 2,
            receive_timeout_secs: 15,
            agent_dispatch_timeout_secs: 300,
            max_concurrent_handlers: 64,
        }
    }
}
