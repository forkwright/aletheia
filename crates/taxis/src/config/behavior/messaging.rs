//! Agora messaging transport configuration.

use std::collections::HashMap;

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
/// Default value used for `RawPayloadPolicy::max_bytes`.
pub(crate) const DEFAULT_RAW_PAYLOAD_MAX_BYTES: usize = 4096;

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
    /// Per-agent outbound-recipient allowlist and default-deny posture,
    /// enforced by `agora::ChannelRegistry::send` before any provider send.
    pub outbound: OutboundMessagePolicy,
    /// Opt-in, bounded raw provider-payload retention on
    /// `InboundMessage::raw` (Signal envelopes, Matrix events).
    pub raw_payload: RawPayloadPolicy,
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
            outbound: OutboundMessagePolicy::default(),
            raw_payload: RawPayloadPolicy::default(),
        }
    }
}

/// Opt-in, bounded raw provider-payload retention on
/// `InboundMessage::raw`.
///
/// WHY default-off (#5198): a captured Signal envelope or Matrix event
/// nests phone numbers, Matrix IDs, and attachment URLs several levels
/// deep. Storing it unconditionally means every trace/diagnostic sink
/// downstream of `InboundMessage` inherits that PII surface -- capture is
/// off by default, matching `OutboundMessagePolicy`'s default-deny
/// posture for the same reason (an operator must opt in to a wider
/// surface, not opt out of one enabled by default). When enabled, a
/// captured payload is still redacted via
/// `koina::redact::redact_json_identifiers` and dropped outright if it
/// exceeds `max_bytes` after redaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct RawPayloadPolicy {
    /// Whether providers attach a raw payload to `InboundMessage::raw` at
    /// all. Default: `false`.
    pub capture: bool,
    /// Maximum encoded size in bytes of a retained (already-redacted) raw
    /// payload. Default: 4096.
    pub max_bytes: usize,
}

impl Default for RawPayloadPolicy {
    fn default() -> Self {
        Self {
            capture: false,
            max_bytes: DEFAULT_RAW_PAYLOAD_MAX_BYTES,
        }
    }
}

/// Per-agent outbound-recipient allowlist and default-deny posture for
/// channel sends (the `message` tool, via `MessageService`, into Agora's
/// `ChannelRegistry::send`).
///
/// WHY default-deny (#4788): outbound messaging is an external side effect
/// -- reaching a phone number, Signal group, or Matrix room outside
/// aletheia's own boundary. An agent should not be able to message an
/// arbitrary recipient merely because a channel provider happens to be
/// registered; the operator opts each agent into the specific recipients
/// it may reach.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct OutboundMessagePolicy {
    /// Allowed recipients per sending agent: `nous_id` -> recipient
    /// patterns. A pattern of exactly `"*"` allows any recipient for that
    /// agent; any other pattern must match the recipient exactly.
    pub allowlist: HashMap<String, Vec<String>>,
    /// Deny a send when the sending agent has no `allowlist` entry at all.
    /// Default: `true` (fail closed) -- an operator who never configured
    /// `[messaging.outbound]` blocks every send rather than allowing every
    /// send, matching `RecallSourcesConfig`'s network-source default-off
    /// posture.
    pub default_deny: bool,
}

impl Default for OutboundMessagePolicy {
    fn default() -> Self {
        Self {
            allowlist: HashMap::new(),
            default_deny: true,
        }
    }
}

impl OutboundMessagePolicy {
    /// Whether `sender` may send to `recipient` under this policy.
    ///
    /// A `sender` of `None` (no attributed agent) is always denied,
    /// regardless of `default_deny`: `default_deny` governs what happens
    /// when a *known* sender has no configured allowlist entry, not
    /// whether an unattributed send is ever allowed.
    #[must_use]
    pub fn allows(&self, sender: Option<&str>, recipient: &str) -> bool {
        let Some(sender) = sender else {
            return false;
        };
        match self.allowlist.get(sender) {
            Some(patterns) => patterns.iter().any(|p| p == "*" || p == recipient),
            None => !self.default_deny,
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

// WHY this module is last: clippy::items_after_test_module forbids any item
// (including the const _: () assertions above, which predate this module)
// appearing textually after a #[cfg(test)] mod -- so the test module must
// be the final item in the file.
#[cfg(test)]
mod outbound_policy_tests {
    use super::OutboundMessagePolicy;

    #[test]
    fn default_denies_unconfigured_sender() {
        let policy = OutboundMessagePolicy::default();
        assert!(!policy.allows(Some("syn"), "+15550100"));
    }

    #[test]
    fn default_denies_unattributed_send() {
        let policy = OutboundMessagePolicy::default();
        assert!(!policy.allows(None, "+15550100"));
    }

    #[test]
    fn allowlisted_exact_recipient_is_allowed() {
        let mut policy = OutboundMessagePolicy::default();
        policy
            .allowlist
            .insert("syn".to_owned(), vec!["+15550100".to_owned()]);
        assert!(policy.allows(Some("syn"), "+15550100"));
        assert!(!policy.allows(Some("syn"), "+15559999"));
    }

    #[test]
    fn wildcard_pattern_allows_any_recipient() {
        let mut policy = OutboundMessagePolicy::default();
        policy
            .allowlist
            .insert("syn".to_owned(), vec!["*".to_owned()]);
        assert!(policy.allows(Some("syn"), "+15550100"));
        assert!(policy.allows(Some("syn"), "anything"));
    }

    #[test]
    fn default_deny_false_allows_unconfigured_sender() {
        let policy = OutboundMessagePolicy {
            default_deny: false,
            ..OutboundMessagePolicy::default()
        };
        assert!(policy.allows(Some("syn"), "+15550100"));
        // WHY: default_deny only relaxes "no allowlist entry" -- an
        // unattributed sender is still refused.
        assert!(!policy.allows(None, "+15550100"));
    }

    #[test]
    fn raw_payload_policy_defaults_to_capture_disabled() {
        let policy = super::RawPayloadPolicy::default();
        assert!(!policy.capture);
        assert_eq!(policy.max_bytes, super::DEFAULT_RAW_PAYLOAD_MAX_BYTES);
    }

    #[test]
    fn messaging_config_defaults_to_raw_payload_capture_disabled() {
        assert!(!super::MessagingConfig::default().raw_payload.capture);
    }
}
