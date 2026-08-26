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
    /// Retain the raw provider payload (Signal envelope, Matrix event) on
    /// inbound messages for diagnostics. Default: `false` — raw payloads
    /// contain personal identifiers and message metadata, so they are
    /// captured only when an operator explicitly opts in.
    pub retain_raw_payloads: bool,
    /// Per-agent outbound-recipient allowlist and default-deny posture,
    /// enforced by `agora::ChannelRegistry::send` before any provider send.
    pub outbound: OutboundMessagePolicy,
    /// Inbound `!`-command authorization: who may invoke operational
    /// commands from a channel, and which commands stay public.
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
            retain_raw_payloads: false,
            outbound: OutboundMessagePolicy::default(),
            commands: InboundCommandPolicy::default(),
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

/// Inbound `!`-command authorization: who may invoke operational
/// commands from a channel, and which commands stay public.
///
/// WHY default-deny (#5193): any inbound message that parses as a `!`
/// command is intercepted after routing and can enumerate agents, channel
/// health, models, skills, blackboard entries, and sessions. Wildcard and
/// default routes make that surface available to broad sender sets, so the
/// safe default is that only `public_commands` are reachable and the
/// operational surface requires an explicit `operators` entry naming the
/// sender. Non-operator senders see only the public subset in `!help`
/// output; denied commands receive a refusal reply and are counted in the
/// `aletheia_command_denied_total` metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct InboundCommandPolicy {
    /// Sender patterns granted the full operator command surface. Each
    /// entry is `"channel:source"`; either segment may be `"*"`, and the
    /// source segment may itself contain `:` (e.g. Matrix user IDs) —
    /// only the first `:` separates the channel.
    pub operators: Vec<String>,
    /// Command names any sender may invoke (without the leading `!`).
    /// Default: `["help", "ping"]` — liveness and discovery reveal no
    /// fleet state.
    pub public_commands: Vec<String>,
    /// Allow every command from any sender. Default: `false` (fail
    /// closed). Setting this to `true` restores the pre-policy behavior
    /// and is an explicit operator opt-out.
    pub default_allow: bool,
}

impl Default for InboundCommandPolicy {
    fn default() -> Self {
        Self {
            operators: Vec::new(),
            public_commands: vec!["help".to_owned(), "ping".to_owned()],
            default_allow: false,
        }
    }
}

impl InboundCommandPolicy {
    /// Whether `command` (name without the leading `!`) may be invoked by
    /// `sender` on `channel` under this policy.
    #[must_use]
    pub fn allows(&self, command: &str, channel: &str, sender: &str) -> bool {
        self.is_operator(channel, sender) || self.public_commands.iter().any(|c| c == command)
    }

    /// Whether `sender` on `channel` holds the operator command tier.
    #[must_use]
    pub fn is_operator(&self, channel: &str, sender: &str) -> bool {
        self.default_allow
            || self
                .operators
                .iter()
                .any(|pattern| sender_pattern_matches(pattern, channel, sender))
    }
}

fn sender_pattern_matches(pattern: &str, channel: &str, sender: &str) -> bool {
    let Some((channel_pat, source_pat)) = pattern.split_once(':') else {
        // WHY: a pattern without a channel separator can never match;
        // treating it as malformed-deny keeps a typo from widening access.
        return false;
    };
    (channel_pat == "*" || channel_pat == channel) && (source_pat == "*" || source_pat == sender)
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
    use super::{InboundCommandPolicy, OutboundMessagePolicy};

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
        assert!(!policy.allows("agents", "signal", "+15550100"));
        assert!(!policy.allows("blackboard", "matrix", "@mallory:example.org"));
        assert!(!policy.is_operator("signal", "+15550100"));
    }

    #[test]
    fn inbound_default_allows_public_subset() {
        let policy = InboundCommandPolicy::default();
        assert!(policy.allows("help", "signal", "+15550100"));
        assert!(policy.allows("ping", "matrix", "@anyone:example.org"));
    }

    #[test]
    fn inbound_operator_entry_grants_full_surface() {
        let policy = InboundCommandPolicy {
            operators: vec!["signal:+15550100".to_owned()],
            ..InboundCommandPolicy::default()
        };
        assert!(policy.allows("agents", "signal", "+15550100"));
        assert!(policy.is_operator("signal", "+15550100"));
        // WHY: the grant is scoped to the exact channel + sender pair.
        assert!(!policy.allows("agents", "matrix", "+15550100"));
        assert!(!policy.allows("agents", "signal", "+15550999"));
    }

    #[test]
    fn inbound_operator_wildcards() {
        let policy = InboundCommandPolicy {
            operators: vec!["matrix:*".to_owned()],
            ..InboundCommandPolicy::default()
        };
        assert!(policy.allows("channels", "matrix", "@alice:example.org"));
        assert!(!policy.allows("channels", "signal", "@alice:example.org"));
    }

    #[test]
    fn inbound_operator_source_may_contain_colons() {
        let policy = InboundCommandPolicy {
            operators: vec!["matrix:@alice:example.org".to_owned()],
            ..InboundCommandPolicy::default()
        };
        assert!(policy.allows("uptime", "matrix", "@alice:example.org"));
        assert!(!policy.allows("uptime", "matrix", "@bob:example.org"));
    }

    #[test]
    fn inbound_malformed_pattern_never_matches() {
        let policy = InboundCommandPolicy {
            operators: vec!["no-separator".to_owned()],
            ..InboundCommandPolicy::default()
        };
        assert!(!policy.is_operator("signal", "no-separator"));
    }

    #[test]
    fn inbound_default_allow_restores_open_behavior() {
        let policy = InboundCommandPolicy {
            default_allow: true,
            ..InboundCommandPolicy::default()
        };
        assert!(policy.allows("blackboard", "signal", "+15550999"));
    }

    #[test]
    fn inbound_public_commands_are_configurable() {
        let policy = InboundCommandPolicy {
            public_commands: vec!["status".to_owned()],
            ..InboundCommandPolicy::default()
        };
        assert!(policy.allows("status", "signal", "+15550999"));
        assert!(!policy.allows("help", "signal", "+15550999"));
    }
}
