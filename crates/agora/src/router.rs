//! Message routing: resolves inbound messages to nous targets.

use tracing::debug;

use taxis::config::ChannelBinding;

use crate::types::InboundMessage;

/// A resolved routing decision.
///
/// Borrows `nous_id` from the router's binding data. The `session_key` is
/// always freshly expanded, so it remains owned.
#[derive(Debug, Clone, PartialEq, Eq)] // kanon:ignore RUST/no-debug-derive-on-public-types WHY: session_key is a routing template expansion (non-sensitive); MatchReason and nous_id are non-sensitive
pub struct RouteDecision<'a> {
    /// The nous agent that should handle this message.
    pub nous_id: &'a str,
    /// Session key derived from template expansion (e.g., `signal:+1234567890`).
    pub session_key: String, // kanon:ignore RUST/plain-string-secret WHY: session_key is a routing key (channel:sender template expansion), not a credential
    /// How the routing decision was determined.
    pub matched_by: MatchReason,
}

/// How the routing decision was made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchReason {
    /// Matched by exact group ID binding on a specific channel.
    GroupBinding,
    /// Matched by exact sender binding on a specific channel.
    SourceBinding,
    /// Matched by channel-level wildcard (`source = "*"`).
    ChannelDefault,
    /// Fell through to the global default nous.
    GlobalDefault,
}

/// Routes inbound channel messages to the appropriate nous agent.
///
/// Resolution order:
/// 1. Exact group match: channel + `group_id` → `nous_id`
/// 2. Exact source match: channel + source → `nous_id`
/// 3. Default for channel: channel + `"*"` → `nous_id`
/// 4. Global default: the nous with `default: true`
/// 5. No match → `None`
///
/// A binding that sets `account` only matches messages received by that
/// provider account (`InboundMessage::account_id`); a binding without
/// `account` matches any account. This keeps identical senders/groups on
/// different accounts of the same channel distinct.
///
/// A binding with a non-empty `participants` list only matches the listed
/// senders; other senders (including fellow members of a matched group)
/// fall through to lower-priority routes.
pub struct MessageRouter {
    bindings: Vec<ChannelBinding>,
    default_nous: Option<String>,
}

impl MessageRouter {
    /// Build a router from channel bindings and an optional global default nous.
    #[must_use]
    pub fn new(bindings: Vec<ChannelBinding>, default_nous: Option<String>) -> Self {
        Self {
            bindings,
            default_nous,
        }
    }

    /// Resolve which nous should handle this message.
    ///
    /// # Complexity
    ///
    /// O(b) where b is the number of channel bindings.
    pub fn resolve(&self, msg: &InboundMessage) -> Option<RouteDecision<'_>> {
        let decision = self.match_route(msg);
        if let Some(ref d) = decision {
            debug!(nous_id = %d.nous_id, matched_by = ?d.matched_by, "message routed");
        }
        decision
    }

    fn match_route(&self, msg: &InboundMessage) -> Option<RouteDecision<'_>> {
        // NOTE: Priority 1: exact group match (channel + group_id)
        if let Some(group_id) = &msg.group_id {
            for b in &self.bindings {
                if b.channel == msg.channel && b.source == *group_id && binding_matches(b, msg) {
                    return Some(RouteDecision {
                        nous_id: &b.nous_id,
                        session_key: expand_session_key(&b.session_key, msg),
                        matched_by: MatchReason::GroupBinding,
                    });
                }
            }
        }

        // NOTE: Priority 2: exact source match (channel + sender)
        for b in &self.bindings {
            if b.channel == msg.channel && b.source == msg.sender && binding_matches(b, msg) {
                return Some(RouteDecision {
                    nous_id: &b.nous_id,
                    session_key: expand_session_key(&b.session_key, msg),
                    matched_by: MatchReason::SourceBinding,
                });
            }
        }

        // NOTE: Priority 3: channel default (source = "*")
        for b in &self.bindings {
            if b.channel == msg.channel && b.source == "*" && binding_matches(b, msg) {
                return Some(RouteDecision {
                    nous_id: &b.nous_id,
                    session_key: expand_session_key(&b.session_key, msg),
                    matched_by: MatchReason::ChannelDefault,
                });
            }
        }

        // NOTE: Priority 4: global default
        self.default_nous.as_deref().map(|id| RouteDecision {
            nous_id: id,
            session_key: expand_session_key("{source}", msg),
            matched_by: MatchReason::GlobalDefault,
        })
    }
}

/// A binding that names an `account` matches only messages received by that
/// account; a binding without one matches any account (including messages
/// whose provider did not attribute an account).
fn account_matches(binding: &ChannelBinding, msg: &InboundMessage) -> bool {
    binding
        .account
        .as_deref()
        .is_none_or(|account| Some(account) == msg.account_id.as_deref())
}

/// A binding with a non-empty `participants` allowlist matches only the
/// listed senders; anyone else falls through to lower-priority routes, so
/// mere membership in a configured group grants no route on its own.
fn participant_allowed(binding: &ChannelBinding, msg: &InboundMessage) -> bool {
    binding.participants.is_empty() || binding.participants.iter().any(|p| *p == msg.sender)
}

/// Both binding-level identity legs (account, participant) must hold for a
/// binding to match.
fn binding_matches(binding: &ChannelBinding, msg: &InboundMessage) -> bool {
    account_matches(binding, msg) && participant_allowed(binding, msg)
}

/// Expand session key template placeholders.
fn expand_session_key(template: &str, msg: &InboundMessage) -> String {
    template
        .replace("{source}", &msg.sender)
        .replace("{group}", msg.group_id.as_deref().unwrap_or("dm"))
        .replace("{account}", msg.account_id.as_deref().unwrap_or("default"))
}

/// Determine reply target for outbound response.
///
/// Group messages reply to the group. Signal keeps its `group:` send-target
/// prefix; Matrix replies directly to the room ID.
#[must_use]
pub fn reply_target(msg: &InboundMessage) -> String {
    match (msg.channel.as_str(), &msg.group_id) {
        ("signal", Some(group)) => format!("group:{group}"),
        (_, Some(group)) => group.clone(),
        (_, None) => msg.sender.clone(),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn dm_message(sender: &str) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: sender.to_owned(),
            sender_name: None,
            group_id: None,
            account_id: None,
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            raw: None,
        }
    }

    fn group_message(sender: &str, group_id: &str) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: sender.to_owned(),
            sender_name: None,
            group_id: Some(group_id.to_owned()),
            account_id: None,
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            raw: None,
        }
    }

    fn binding(channel: &str, source: &str, nous_id: &str) -> ChannelBinding {
        ChannelBinding {
            channel: channel.to_owned(),
            source: source.to_owned(),
            nous_id: nous_id.to_owned(),
            session_key: "{source}".to_owned(),
            account: None,
            participants: vec![],
        }
    }

    #[test]
    fn exact_group_binding_matches() {
        let router = MessageRouter::new(vec![binding("signal", "group-abc", "syn")], None);
        let msg = group_message("+1234567890", "group-abc");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.nous_id, "syn");
        assert_eq!(decision.matched_by, MatchReason::GroupBinding);
    }

    #[test]
    fn exact_source_binding_matches() {
        let router = MessageRouter::new(vec![binding("signal", "+1234567890", "alice")], None);
        let msg = dm_message("+1234567890");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.nous_id, "alice");
        assert_eq!(decision.matched_by, MatchReason::SourceBinding);
    }

    #[test]
    fn channel_default_matches() {
        let router = MessageRouter::new(vec![binding("signal", "*", "default-nous")], None);
        let msg = dm_message("+9999999999");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.nous_id, "default-nous");
        assert_eq!(decision.matched_by, MatchReason::ChannelDefault);
    }

    #[test]
    fn global_default_fallback() {
        let router = MessageRouter::new(vec![], Some("global-nous".to_owned()));
        let msg = dm_message("+1234567890");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.nous_id, "global-nous");
        assert_eq!(decision.matched_by, MatchReason::GlobalDefault);
    }

    #[test]
    fn no_match_returns_none() {
        let router = MessageRouter::new(vec![], None);
        let msg = dm_message("+1234567890");
        assert!(router.resolve(&msg).is_none());
    }

    #[test]
    fn group_binding_takes_priority_over_source() {
        let router = MessageRouter::new(
            vec![
                binding("signal", "+1234567890", "source-nous"),
                binding("signal", "group-abc", "group-nous"),
            ],
            None,
        );
        let msg = group_message("+1234567890", "group-abc");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.nous_id, "group-nous");
        assert_eq!(decision.matched_by, MatchReason::GroupBinding);
    }

    #[test]
    fn session_key_source_interpolation() {
        let mut b = binding("signal", "*", "syn");
        b.session_key = "signal:{source}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let msg = dm_message("+1234567890");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "signal:+1234567890");
    }

    #[test]
    fn session_key_group_interpolation() {
        let mut b = binding("signal", "group-abc", "syn");
        b.session_key = "signal:group:{group}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let msg = group_message("+1234567890", "group-abc");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "signal:group:group-abc");
    }

    #[test]
    fn dm_session_key_format() {
        let mut b = binding("signal", "+1234567890", "syn");
        b.session_key = "signal:{source}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let msg = dm_message("+1234567890");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "signal:+1234567890");
    }

    #[test]
    fn group_session_key_format() {
        let mut b = binding("signal", "group-xyz", "syn");
        b.session_key = "signal:group:{group}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let msg = group_message("+9999999999", "group-xyz");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "signal:group:group-xyz");
    }

    #[test]
    fn group_placeholder_defaults_to_dm() {
        let mut b = binding("signal", "+1234567890", "syn");
        b.session_key = "{source}:{group}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let msg = dm_message("+1234567890");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "+1234567890:dm");
    }

    #[test]
    fn wrong_channel_does_not_match() {
        let router = MessageRouter::new(vec![binding("slack", "+1234567890", "syn")], None);
        let msg = dm_message("+1234567890");
        assert!(router.resolve(&msg).is_none());
    }

    #[test]
    fn reply_target_dm() {
        let msg = dm_message("+1234567890");
        assert_eq!(reply_target(&msg), "+1234567890");
    }

    #[test]
    fn reply_target_group() {
        let msg = group_message("+1234567890", "group-abc");
        assert_eq!(reply_target(&msg), "group:group-abc");
    }

    #[test]
    fn reply_target_matrix_room() {
        let mut msg = group_message("@alice:example.org", "!room:example.org");
        msg.channel = "matrix".to_owned();
        assert_eq!(reply_target(&msg), "!room:example.org");
    }

    #[test]
    fn source_binding_takes_priority_over_channel_default() {
        let router = MessageRouter::new(
            vec![
                binding("signal", "*", "default-nous"),
                binding("signal", "+15550100", "alice-nous"),
            ],
            None,
        );
        let msg = dm_message("+15550100");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.nous_id, "alice-nous");
        assert_eq!(decision.matched_by, MatchReason::SourceBinding);
    }

    #[test]
    fn bindings_on_different_channel_do_not_cross_match() {
        let router = MessageRouter::new(
            vec![
                binding("slack", "+15550100", "slack-nous"),
                binding("discord", "group-abc", "discord-nous"),
            ],
            None,
        );
        let msg = dm_message("+15550100");
        assert!(
            router.resolve(&msg).is_none(),
            "signal message must not match slack or discord bindings"
        );
    }

    #[test]
    fn global_default_session_key_uses_sender() {
        let router = MessageRouter::new(vec![], Some("fallback-nous".to_owned()));
        let msg = dm_message("+15550101");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "+15550101");
        assert_eq!(decision.matched_by, MatchReason::GlobalDefault);
    }

    #[test]
    fn group_binding_matches_regardless_of_sender() {
        // NOTE: pins the empty-`participants` compatibility behavior: an
        // unlisted binding stays open to every group participant.
        let router = MessageRouter::new(vec![binding("signal", "group-xyz", "group-nous")], None);
        let msg_a = group_message("+15550100", "group-xyz");
        let msg_b = group_message("+15550199", "group-xyz");
        let decision_a = router.resolve(&msg_a).expect("should match");
        let decision_b = router.resolve(&msg_b).expect("should match");
        assert_eq!(decision_a.nous_id, "group-nous");
        assert_eq!(decision_b.nous_id, "group-nous");
    }

    #[test]
    fn group_binding_participants_allowlist_is_enforced() {
        let mut b = binding("signal", "group-xyz", "ops-nous");
        b.participants = vec!["+15550100".to_owned()];
        let router = MessageRouter::new(vec![b], None);

        let allowed = group_message("+15550100", "group-xyz");
        let decision = router.resolve(&allowed).expect("listed participant should match");
        assert_eq!(decision.nous_id, "ops-nous");
        assert_eq!(decision.matched_by, MatchReason::GroupBinding);

        let stranger = group_message("+15550999", "group-xyz");
        assert!(
            router.resolve(&stranger).is_none(),
            "unlisted group participant must not activate the binding"
        );
    }

    #[test]
    fn unlisted_participant_falls_through_to_channel_default() {
        let mut restricted = binding("signal", "group-xyz", "ops-nous");
        restricted.participants = vec!["+15550100".to_owned()];
        let router = MessageRouter::new(
            vec![restricted, binding("signal", "*", "catchall-nous")],
            None,
        );

        let stranger = group_message("+15550999", "group-xyz");
        let decision = router
            .resolve(&stranger)
            .expect("channel default should catch unlisted participant");
        assert_eq!(decision.nous_id, "catchall-nous");
        assert_eq!(decision.matched_by, MatchReason::ChannelDefault);

        let allowed = group_message("+15550100", "group-xyz");
        assert_eq!(
            router.resolve(&allowed).expect("should match").nous_id,
            "ops-nous"
        );
    }

    #[test]
    fn mixed_privilege_group_members_route_to_different_nous() {
        let mut operators = binding("signal", "group-xyz", "ops-nous");
        operators.participants = vec!["+15550100".to_owned()];
        operators.session_key = "signal:ops:{group}".to_owned();
        let router = MessageRouter::new(vec![operators, binding("signal", "group-xyz", "guest-nous")], None);

        let operator = group_message("+15550100", "group-xyz");
        let decision = router.resolve(&operator).expect("operator should match");
        assert_eq!(decision.nous_id, "ops-nous");
        assert_eq!(decision.session_key, "signal:ops:group-xyz");

        let guest = group_message("+15550999", "group-xyz");
        let decision = router.resolve(&guest).expect("guest should match");
        assert_eq!(decision.nous_id, "guest-nous");
        assert_eq!(decision.matched_by, MatchReason::GroupBinding);
    }

    #[test]
    fn participants_restrict_channel_wildcard_binding() {
        let mut b = binding("signal", "*", "known-senders-nous");
        b.participants = vec!["+15550100".to_owned(), "+15550101".to_owned()];
        let router = MessageRouter::new(vec![b], None);

        let known = dm_message("+15550101");
        assert_eq!(
            router.resolve(&known).expect("should match").nous_id,
            "known-senders-nous"
        );

        let unknown = dm_message("+15550999");
        assert!(
            router.resolve(&unknown).is_none(),
            "wildcard with participants must not catch unlisted senders"
        );
    }

    #[test]
    fn session_key_without_placeholders_is_literal() {
        let mut b = binding("signal", "+15550100", "syn");
        b.session_key = "fixed-key".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let msg = dm_message("+15550100");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "fixed-key");
    }

    #[test]
    fn account_scoped_binding_matches_only_that_account() {
        let mut scoped = binding("signal", "+15550100", "work-nous");
        scoped.account = Some("work".to_owned());
        let router = MessageRouter::new(vec![scoped], None);

        let mut on_work = dm_message("+15550100");
        on_work.account_id = Some("work".to_owned());
        let decision = router.resolve(&on_work).expect("should match");
        assert_eq!(decision.nous_id, "work-nous");

        let mut on_personal = dm_message("+15550100");
        on_personal.account_id = Some("personal".to_owned());
        assert!(
            router.resolve(&on_personal).is_none(),
            "same sender on a different account must not match an account-scoped binding"
        );

        let unattributed = dm_message("+15550100");
        assert!(
            router.resolve(&unattributed).is_none(),
            "message without an account must not match an account-scoped binding"
        );
    }

    #[test]
    fn account_scoped_binding_falls_through_to_unscoped() {
        let mut scoped = binding("signal", "*", "work-nous");
        scoped.account = Some("work".to_owned());
        let router = MessageRouter::new(vec![scoped, binding("signal", "*", "any-nous")], None);

        let mut on_work = dm_message("+15550100");
        on_work.account_id = Some("work".to_owned());
        assert_eq!(
            router.resolve(&on_work).expect("should match").nous_id,
            "work-nous"
        );

        let mut on_personal = dm_message("+15550100");
        on_personal.account_id = Some("personal".to_owned());
        let decision = router.resolve(&on_personal).expect("should match");
        assert_eq!(decision.nous_id, "any-nous");
        assert_eq!(decision.matched_by, MatchReason::ChannelDefault);
    }

    #[test]
    fn session_key_account_placeholder() {
        let mut b = binding("signal", "*", "syn");
        b.session_key = "signal:{account}:{source}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let mut msg = dm_message("+15550100");
        msg.account_id = Some("work".to_owned());
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "signal:work:+15550100");

        let unattributed = dm_message("+15550100");
        let decision = router.resolve(&unattributed).expect("should match");
        assert_eq!(decision.session_key, "signal:default:+15550100");
    }
}
