//! Agora messaging transport configuration.

use serde::{Deserialize, Serialize};
/// Default value used for `MessagingConfig::poll_interval_ms`.
pub(crate) const DEFAULT_POLL_INTERVAL_MS: u64 = 2_000;
/// Default value used for `MessagingConfig::buffer_capacity`.
pub(crate) const DEFAULT_BUFFER_CAPACITY: usize = 100;
/// Default value used for `MessagingConfig::circuit_breaker_threshold`.
pub(crate) const DEFAULT_CIRCUIT_BREAKER_THRESHOLD: u32 = 5;
/// Default value used for `MessagingConfig::halted_health_check_interval_secs`.
pub(crate) const DEFAULT_HALTED_HEALTH_CHECK_INTERVAL_SECS: u64 = 60;
/// Default value used for `MessagingConfig::rpc_timeout_secs`.
pub(crate) const DEFAULT_RPC_TIMEOUT_SECS: u64 = 10;
/// Default value used for `MessagingConfig::health_timeout_secs`.
pub(crate) const DEFAULT_HEALTH_TIMEOUT_SECS: u64 = 2;
/// Default value used for `MessagingConfig::receive_timeout_secs`.
pub(crate) const DEFAULT_RECEIVE_TIMEOUT_SECS: u64 = 15;
/// Default value used for `MessagingConfig::agent_dispatch_timeout_secs`.
pub(crate) const DEFAULT_AGENT_DISPATCH_TIMEOUT_SECS: u64 = 300;

/// Agora messaging transport poll, buffer, circuit-breaker, and RPC settings.
///
/// Defaults for the fields that mirror `agora`/`organon` constants are
/// enforced at test-build time by `const _: () = assert!` guards below.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct MessagingConfig {
    /// How often Semeion polls for new channel messages in milliseconds.
    pub poll_interval_ms: u64,
    /// Inbound message buffer size per channel.
    pub buffer_capacity: usize,
    /// Consecutive channel errors before the channel is halted.
    pub circuit_breaker_threshold: u32,
    /// How often a halted channel is health-checked in seconds.
    pub halted_health_check_interval_secs: u64,
    /// Timeout in seconds for Semeion RPC calls.
    pub rpc_timeout_secs: u64,
    /// Timeout in seconds for Semeion health-check requests.
    pub health_timeout_secs: u64,
    /// Timeout in seconds waiting to receive a Semeion response.
    pub receive_timeout_secs: u64,
    /// Default timeout in seconds for agent-dispatch tool calls.
    pub agent_dispatch_timeout_secs: u64,
    /// Maximum concurrent inbound-message handler tasks. Default: 64.
    pub max_concurrent_handlers: usize,
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            buffer_capacity: DEFAULT_BUFFER_CAPACITY,
            circuit_breaker_threshold: DEFAULT_CIRCUIT_BREAKER_THRESHOLD,
            halted_health_check_interval_secs: DEFAULT_HALTED_HEALTH_CHECK_INTERVAL_SECS,
            rpc_timeout_secs: DEFAULT_RPC_TIMEOUT_SECS,
            health_timeout_secs: DEFAULT_HEALTH_TIMEOUT_SECS,
            receive_timeout_secs: DEFAULT_RECEIVE_TIMEOUT_SECS,
            agent_dispatch_timeout_secs: DEFAULT_AGENT_DISPATCH_TIMEOUT_SECS,
            max_concurrent_handlers: 64,
        }
    }
}

#[cfg(test)]
const _: () =
    assert!(DEFAULT_POLL_INTERVAL_MS == agora::semeion::DEFAULT_POLL_INTERVAL.as_secs() * 1_000);
#[cfg(test)]
const _: () = assert!(DEFAULT_BUFFER_CAPACITY == agora::semeion::DEFAULT_BUFFER_CAPACITY);
#[cfg(test)]
const _: () =
    assert!(DEFAULT_CIRCUIT_BREAKER_THRESHOLD == agora::semeion::CIRCUIT_BREAKER_THRESHOLD);
#[cfg(test)]
const _: () = assert!(
    DEFAULT_HALTED_HEALTH_CHECK_INTERVAL_SECS
        == agora::semeion::HALTED_HEALTH_CHECK_INTERVAL.as_secs()
);
#[cfg(test)]
const _: () = assert!(DEFAULT_RPC_TIMEOUT_SECS == agora::semeion::client::RPC_TIMEOUT.as_secs());
#[cfg(test)]
const _: () =
    assert!(DEFAULT_HEALTH_TIMEOUT_SECS == agora::semeion::client::HEALTH_TIMEOUT.as_secs());
#[cfg(test)]
const _: () =
    assert!(DEFAULT_RECEIVE_TIMEOUT_SECS == agora::semeion::client::RECEIVE_TIMEOUT.as_secs());
#[cfg(test)]
const _: () =
    assert!(DEFAULT_AGENT_DISPATCH_TIMEOUT_SECS == organon::builtins::agent::DEFAULT_TIMEOUT_SECS);
