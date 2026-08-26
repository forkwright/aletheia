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
    /// Enforced on the live dispatch path; saturation is observable via the
    /// `aletheia_inbound_handler_saturation_total` counter and the
    /// `aletheia_inbound_handlers_in_flight` gauge.
    pub max_concurrent_handlers: usize,
    /// Per-agent outbound-recipient allowlist and default-deny posture,
    /// enforced by `agora::ChannelRegistry::send` before any provider send.
    /// Channel services clone this policy at startup, so changes require a
    /// process restart before they alter send authority.
    pub outbound: OutboundMessagePolicy,
    /// Inbound `!`-command authorization: which commands stay public.
    /// Operational authority is carried by the selected channel binding. The
    /// dispatcher clones both at startup, so changes require a process restart
    /// before they alter command authority.
    pub commands: InboundCommandPolicy,
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
            commands: InboundCommandPolicy::default(),
        }
    }
}

/// Per-agent outbound-recipient allowlist and default-deny posture for
/// channel sends (the `message` tool, via `MessageService`, into Agora's
/// `ChannelRegistry::send`). Runtime channel services capture a clone at
/// startup; policy changes are cold until those services are rebuilt.
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

/// Authority tier attached to a selected inbound route.
///
/// The router, rather than the command dispatcher, derives this value from
/// the complete channel/account/group/participant match. This keeps command
/// authority bound to the identity evidence that selected the destination.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CommandTier {
    /// Only the fixed, non-operational public command surface is available.
    #[default]
    Public,
    /// The complete operational command surface is available.
    Operator,
}

/// Inbound `!`-command authorization for the public command subset. The
/// dispatcher captures a clone at startup; policy changes are cold until it
/// is rebuilt.
///
/// WHY default-deny (#5193): any inbound message that parses as a `!`
/// command is intercepted after routing and can enumerate agents, channel
/// health, models, skills, blackboard entries, and sessions. Wildcard and
/// default routes make that surface available to broad sender sets, so the
/// safe default is that only `public_commands` are reachable and the
/// operational surface requires a route whose [`CommandTier`] resolves to
/// `Operator`. Non-operator senders see only the public subset in `!help`
/// output; denied commands receive a refusal reply and are counted in the
/// `aletheia_command_denied_total` metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct InboundCommandPolicy {
    /// Command names any sender may invoke (without the leading `!`).
    /// Default: `["help", "ping"]` — liveness and discovery reveal no
    /// fleet state. Configuration may narrow this set, but cannot broaden it
    /// beyond the built-in safe commands.
    pub public_commands: Vec<String>,
}

impl Default for InboundCommandPolicy {
    fn default() -> Self {
        Self {
            public_commands: vec!["help".to_owned(), "ping".to_owned()],
        }
    }
}

impl InboundCommandPolicy {
    /// Whether `command` may be invoked at the authority of the selected route.
    ///
    /// `public_commands` is intersected with the built-in safe set, so a
    /// directly constructed config cannot make an operational command public.
    #[must_use]
    pub fn allows(&self, command: &str, tier: CommandTier) -> bool {
        tier == CommandTier::Operator || self.public_command_names().any(|name| name == command)
    }

    /// Configured public commands after fail-closed intersection with the safe set.
    pub fn public_command_names(&self) -> impl Iterator<Item = &str> {
        self.public_commands
            .iter()
            .map(String::as_str)
            .filter(|name| matches!(*name, "help" | "ping"))
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
mod policy_tests {
    use super::{CommandTier, InboundCommandPolicy, MessagingConfig, OutboundMessagePolicy};

    #[test]
    fn raw_payload_retention_is_not_a_public_config_switch() {
        let result = serde_json::from_value::<MessagingConfig>(serde_json::json!({
            "retainRawPayloads": true
        }));
        assert!(
            result.is_err(),
            "raw capture needs a governed operator-only design, not a transport config flag"
        );
    }

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
    fn inbound_default_denies_operator_commands() {
        let policy = InboundCommandPolicy::default();
        assert!(!policy.allows("agents", CommandTier::Public));
        assert!(!policy.allows("blackboard", CommandTier::Public));
    }

    #[test]
    fn inbound_default_allows_public_subset() {
        let policy = InboundCommandPolicy::default();
        assert!(policy.allows("help", CommandTier::Public));
        assert!(policy.allows("ping", CommandTier::Public));
    }

    #[test]
    fn inbound_operator_tier_grants_full_surface() {
        let policy = InboundCommandPolicy::default();
        assert!(policy.allows("agents", CommandTier::Operator));
        assert!(policy.allows("blackboard", CommandTier::Operator));
    }

    #[test]
    fn inbound_public_commands_are_configurable() {
        let policy = InboundCommandPolicy {
            public_commands: vec!["ping".to_owned()],
        };
        assert!(policy.allows("ping", CommandTier::Public));
        assert!(!policy.allows("help", CommandTier::Public));
    }

    #[test]
    fn inbound_public_configuration_cannot_broaden_safe_set() {
        let policy = InboundCommandPolicy {
            public_commands: vec!["help".to_owned(), "agents".to_owned()],
        };
        assert!(policy.allows("help", CommandTier::Public));
        assert!(!policy.allows("agents", CommandTier::Public));
    }

    #[test]
    fn inbound_retired_sender_grants_are_rejected() {
        for json in [
            r#"{"operators":["signal:+15550100"]}"#,
            r#"{"defaultAllow":true}"#,
        ] {
            assert!(serde_json::from_str::<InboundCommandPolicy>(json).is_err());
        }
    }
}
