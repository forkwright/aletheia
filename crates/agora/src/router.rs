//! Message routing: resolves inbound messages to nous targets.

use tracing::debug;

use taxis::config::{
    ChannelBinding, ChannelSourceKind, CommandTier, DEFAULT_CHANNEL_SESSION_KEY_PATTERN,
};

use crate::types::InboundMessage;

/// A resolved routing decision.
///
/// Borrows `nous_id` from the router's binding data. The `session_key` is
/// always freshly expanded, so it remains owned.
#[derive(Clone, PartialEq, Eq)]
pub struct RouteDecision<'a> {
    /// The nous agent that should handle this message.
    pub nous_id: &'a str,
    /// Session key derived from template expansion (e.g., `signal:+1234567890`).
    pub session_key: String, // kanon:ignore RUST/plain-string-secret WHY: session_key is a routing key (channel:sender template expansion), not a credential
    /// How the routing decision was determined.
    pub matched_by: MatchReason,
    /// Command authority proven by the selected route's identity constraints.
    pub command_tier: CommandTier,
}

impl std::fmt::Debug for RouteDecision<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteDecision")
            .field("nous_id", &self.nous_id)
            .field("session_key", &"[REDACTED]")
            .field("matched_by", &self.matched_by)
            .field("command_tier", &self.command_tier)
            .finish()
    }
}

/// How the routing decision was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// An exact group binding casts a deny shadow: once the group and account
/// identify one or more bindings, an unlisted participant cannot fall through
/// to a DM, wildcard, or global route. A separate open exact-group binding is
/// the explicit guest route.
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
        // NOTE: Priority 1: exact group match (channel + account + group).
        // The existence check intentionally precedes participant filtering:
        // restricted exact groups deny-shadow broader route classes.
        if let Some(group_id) = &msg.group_id {
            let has_exact_group = self.bindings.iter().any(|binding| {
                binding.channel == msg.channel
                    && binding.source == *group_id
                    && source_kind_matches(binding, msg)
                    && account_matches(binding, msg)
            });
            if has_exact_group {
                return choose_binding(
                    self.bindings.iter().filter(|binding| {
                        binding.channel == msg.channel
                            && binding.source == *group_id
                            && source_kind_matches(binding, msg)
                            && account_matches(binding, msg)
                            && participant_allowed(binding, msg)
                    }),
                    msg,
                    MatchReason::GroupBinding,
                );
            }
        }

        // NOTE: Priority 2: exact source matches are DM-only. A sender's DM
        // route must never grant a route or command authority inside a group.
        if msg.group_id.is_none() {
            let has_exact_source = self.bindings.iter().any(|binding| {
                binding.channel == msg.channel
                    && binding.source == msg.sender
                    && source_kind_matches(binding, msg)
                    && binding_matches(binding, msg)
            });
            if has_exact_source {
                // WHY: `None` here means an equal-specificity conflict, not
                // absence. Returning it prevents an ambiguous exact identity
                // from falling through to a broader grant.
                return choose_binding(
                    self.bindings.iter().filter(|binding| {
                        binding.channel == msg.channel
                            && binding.source == msg.sender
                            && source_kind_matches(binding, msg)
                            && binding_matches(binding, msg)
                    }),
                    msg,
                    MatchReason::SourceBinding,
                );
            }
        }

        // NOTE: Priority 3: channel default (source = "*")
        let has_channel_default = self.bindings.iter().any(|binding| {
            binding.channel == msg.channel
                && binding.source == "*"
                && source_kind_matches(binding, msg)
                && binding_matches(binding, msg)
        });
        if has_channel_default {
            // WHY: as above, an ambiguous wildcard route must fail closed
            // instead of descending to the global default.
            return choose_binding(
                self.bindings.iter().filter(|binding| {
                    binding.channel == msg.channel
                        && binding.source == "*"
                        && source_kind_matches(binding, msg)
                        && binding_matches(binding, msg)
                }),
                msg,
                MatchReason::ChannelDefault,
            );
        }

        // NOTE: Priority 4: global default
        self.default_nous.as_deref().map(|id| RouteDecision {
            nous_id: id,
            session_key: expand_session_key(DEFAULT_CHANNEL_SESSION_KEY_PATTERN, msg),
            matched_by: MatchReason::GlobalDefault,
            command_tier: CommandTier::Public,
        })
    }
}

/// Select the most identity-specific binding without depending on TOML order.
/// Equal-specificity bindings may coalesce only when their complete routing
/// outcome is identical; otherwise ambiguity fails closed.
fn choose_binding<'a>(
    candidates: impl Iterator<Item = &'a ChannelBinding>,
    msg: &InboundMessage,
    matched_by: MatchReason,
) -> Option<RouteDecision<'a>> {
    let mut best: Option<(usize, RouteDecision<'a>)> = None;
    let mut is_ambiguous = false;

    for binding in candidates {
        let specificity = binding_specificity(binding);
        let decision = RouteDecision {
            nous_id: &binding.nous_id,
            session_key: expand_session_key(&binding.session_key, msg),
            command_tier: effective_command_tier(binding, msg, matched_by),
            matched_by,
        };
        match &best {
            None => best = Some((specificity, decision)),
            Some((rank, _)) if specificity > *rank => {
                best = Some((specificity, decision));
                is_ambiguous = false;
            }
            Some((rank, current)) if specificity == *rank && current != &decision => {
                is_ambiguous = true;
            }
            Some(_) => {}
        }
    }

    if is_ambiguous {
        None
    } else {
        best.map(|(_, decision)| decision)
    }
}

fn binding_specificity(binding: &ChannelBinding) -> usize {
    usize::from(binding.source_kind.is_some())
        + usize::from(
            binding
                .account
                .as_deref()
                .is_some_and(|account| !account.trim().is_empty()),
        )
        + usize::from(
            binding
                .participants
                .iter()
                .any(|participant| !participant.trim().is_empty()),
        )
}

/// Clamp directly constructed or malformed operator grants to public unless
/// the selected route proves an exact account plus an exact DM principal.
fn effective_command_tier(
    binding: &ChannelBinding,
    msg: &InboundMessage,
    matched_by: MatchReason,
) -> CommandTier {
    if binding.command_tier != CommandTier::Operator
        || binding.source_kind != Some(ChannelSourceKind::Direct)
        || binding.source == "*"
        || !binding.participants.is_empty()
    {
        return CommandTier::Public;
    }

    let Some(account) = binding.account.as_deref() else {
        return CommandTier::Public;
    };
    if account.trim().is_empty() || msg.account_id.as_deref() != Some(account) {
        return CommandTier::Public;
    }

    if matches!(matched_by, MatchReason::SourceBinding)
        && msg.group_id.is_none()
        && !binding.source.trim().is_empty()
        && !msg.sender.trim().is_empty()
        && binding.source == msg.sender
    {
        CommandTier::Operator
    } else {
        CommandTier::Public
    }
}

/// An explicit source kind narrows the route to the corresponding inbound
/// shape. Absence preserves legacy public routing across direct and group
/// messages, but is never sufficient for operator authority.
fn source_kind_matches(binding: &ChannelBinding, msg: &InboundMessage) -> bool {
    match binding.source_kind {
        None => true,
        Some(ChannelSourceKind::Direct) => msg.group_id.is_none(),
        Some(ChannelSourceKind::Group) => msg.group_id.is_some(),
        Some(_) => false,
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
/// listed senders. Exact groups additionally cast a deny shadow, so an
/// unlisted participant cannot activate a broader route accidentally.
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
        .replace("{channel}", &msg.channel)
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
            message_id: None,
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
            message_id: None,
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
            source_kind: None,
            participants: vec![],
            command_tier: CommandTier::Public,
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
        assert!(
            !format!("{decision:?}").contains("+1234567890"),
            "Debug must not expose the route key"
        );
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
    fn global_default_session_key_uses_complete_channel_identity() {
        let router = MessageRouter::new(vec![], Some("fallback-nous".to_owned()));
        let msg = dm_message("+15550101");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.session_key, "signal:default:dm:+15550101");
        assert_eq!(decision.matched_by, MatchReason::GlobalDefault);
    }

    #[test]
    fn default_binding_pattern_isolates_accounts() {
        let mut default = binding("signal", "*", "syn");
        default.session_key = DEFAULT_CHANNEL_SESSION_KEY_PATTERN.to_owned();
        let router = MessageRouter::new(vec![default], None);

        let mut work = dm_message("+15550100");
        work.account_id = Some("work".to_owned());
        let mut personal = work.clone();
        personal.account_id = Some("personal".to_owned());

        assert_eq!(
            router.resolve(&work).expect("work route").session_key,
            "signal:work:dm:+15550100"
        );
        assert_eq!(
            router
                .resolve(&personal)
                .expect("personal route")
                .session_key,
            "signal:personal:dm:+15550100"
        );
    }

    #[test]
    fn default_binding_pattern_isolates_channels_and_message_shapes() {
        let mut signal = binding("signal", "*", "syn");
        signal.session_key = DEFAULT_CHANNEL_SESSION_KEY_PATTERN.to_owned();
        let mut matrix = binding("matrix", "*", "syn");
        matrix.session_key = DEFAULT_CHANNEL_SESSION_KEY_PATTERN.to_owned();
        let router = MessageRouter::new(vec![signal, matrix], None);

        let mut direct = dm_message("alice");
        direct.account_id = Some("primary".to_owned());
        let mut matrix_direct = direct.clone();
        matrix_direct.channel = "matrix".to_owned();
        let mut group = direct.clone();
        group.group_id = Some("room-a".to_owned());

        assert_eq!(
            router.resolve(&direct).expect("signal DM").session_key,
            "signal:primary:dm:alice"
        );
        assert_eq!(
            router
                .resolve(&matrix_direct)
                .expect("matrix DM")
                .session_key,
            "matrix:primary:dm:alice"
        );
        assert_eq!(
            router.resolve(&group).expect("signal group").session_key,
            "signal:primary:room-a:alice"
        );
    }

    #[test]
    fn global_default_isolates_account_channel_and_group_legs() {
        let router = MessageRouter::new(vec![], Some("fallback-nous".to_owned()));
        let mut base = dm_message("alice");
        base.account_id = Some("primary".to_owned());
        let mut other_account = base.clone();
        other_account.account_id = Some("secondary".to_owned());
        let mut other_channel = base.clone();
        other_channel.channel = "matrix".to_owned();
        let mut group = base.clone();
        group.group_id = Some("room-a".to_owned());

        let keys = [base, other_account, other_channel, group]
            .map(|message| router.resolve(&message).expect("global route").session_key);
        assert_eq!(
            keys,
            [
                "signal:primary:dm:alice",
                "signal:secondary:dm:alice",
                "matrix:primary:dm:alice",
                "signal:primary:room-a:alice",
            ]
        );
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
        let decision = router
            .resolve(&allowed)
            .expect("listed participant should match");
        assert_eq!(decision.nous_id, "ops-nous");
        assert_eq!(decision.matched_by, MatchReason::GroupBinding);

        let stranger = group_message("+15550999", "group-xyz");
        assert!(
            router.resolve(&stranger).is_none(),
            "unlisted group participant must not activate the binding"
        );
    }

    #[test]
    fn unlisted_exact_group_participant_cannot_fall_through() {
        let mut restricted = binding("signal", "group-xyz", "ops-nous");
        restricted.participants = vec!["+15550100".to_owned()];
        let router = MessageRouter::new(
            vec![restricted, binding("signal", "*", "catchall-nous")],
            None,
        );

        let stranger = group_message("+15550999", "group-xyz");
        assert!(
            router.resolve(&stranger).is_none(),
            "exact restricted group must deny-shadow the channel default"
        );

        let allowed = group_message("+15550100", "group-xyz");
        assert_eq!(
            router.resolve(&allowed).expect("should match").nous_id,
            "ops-nous"
        );
    }

    #[test]
    fn layered_group_routes_are_order_independent_and_public() {
        let mut operators = binding("signal", "group-xyz", "ops-nous");
        operators.account = Some("primary".to_owned());
        operators.participants = vec!["+15550100".to_owned()];
        operators.session_key = "signal:ops:{group}".to_owned();
        let guest_binding = binding("signal", "group-xyz", "guest-nous");

        for bindings in [
            vec![operators.clone(), guest_binding.clone()],
            vec![guest_binding.clone(), operators.clone()],
        ] {
            let router = MessageRouter::new(bindings, None);
            let mut listed = group_message("+15550100", "group-xyz");
            listed.account_id = Some("primary".to_owned());
            let decision = router.resolve(&listed).expect("listed route");
            assert_eq!(decision.nous_id, "ops-nous");
            assert_eq!(decision.session_key, "signal:ops:group-xyz");
            assert_eq!(decision.command_tier, CommandTier::Public);

            let mut guest = group_message("+15550999", "group-xyz");
            guest.account_id = Some("primary".to_owned());
            let decision = router.resolve(&guest).expect("guest route");
            assert_eq!(decision.nous_id, "guest-nous");
            assert_eq!(decision.matched_by, MatchReason::GroupBinding);
            assert_eq!(decision.command_tier, CommandTier::Public);
        }
    }

    #[test]
    fn exact_account_scoped_dm_may_grant_operator_tier() {
        let mut operator = binding("signal", "+15550100", "ops-nous");
        operator.account = Some("primary".to_owned());
        operator.source_kind = Some(ChannelSourceKind::Direct);
        operator.command_tier = CommandTier::Operator;
        let router = MessageRouter::new(vec![operator], None);
        let mut message = dm_message("+15550100");
        message.account_id = Some("primary".to_owned());

        let decision = router.resolve(&message).expect("exact operator route");
        assert_eq!(decision.matched_by, MatchReason::SourceBinding);
        assert_eq!(decision.command_tier, CommandTier::Operator);
    }

    #[test]
    fn legacy_unspecified_source_kind_remains_public_for_dm_and_group() {
        let route = binding("signal", "shared-id", "legacy-nous");
        let router = MessageRouter::new(vec![route], None);

        let direct = router.resolve(&dm_message("shared-id")).expect("legacy DM");
        assert_eq!(direct.matched_by, MatchReason::SourceBinding);
        assert_eq!(direct.command_tier, CommandTier::Public);

        let group = router
            .resolve(&group_message("alice", "shared-id"))
            .expect("legacy group");
        assert_eq!(group.matched_by, MatchReason::GroupBinding);
        assert_eq!(group.command_tier, CommandTier::Public);
    }

    #[test]
    fn direct_source_kind_does_not_match_or_deny_shadow_same_named_group() {
        let mut direct = binding("signal", "shared-id", "operator-nous");
        direct.account = Some("primary".to_owned());
        direct.source_kind = Some(ChannelSourceKind::Direct);
        direct.command_tier = CommandTier::Operator;
        let mut group_default = binding("signal", "*", "group-nous");
        group_default.source_kind = Some(ChannelSourceKind::Group);
        let router = MessageRouter::new(vec![direct, group_default], None);

        let mut direct_message = dm_message("shared-id");
        direct_message.account_id = Some("primary".to_owned());
        let decision = router.resolve(&direct_message).expect("direct route");
        assert_eq!(decision.nous_id, "operator-nous");
        assert_eq!(decision.command_tier, CommandTier::Operator);

        let mut group_inbound = group_message("alice", "shared-id");
        group_inbound.account_id = Some("primary".to_owned());
        let decision = router.resolve(&group_inbound).expect("group fallback");
        assert_eq!(decision.nous_id, "group-nous");
        assert_eq!(decision.matched_by, MatchReason::ChannelDefault);
        assert_eq!(decision.command_tier, CommandTier::Public);
    }

    #[test]
    fn missing_direct_source_kind_clamps_manually_constructed_operator_public() {
        let mut operator = binding("signal", "+15550100", "ops-nous");
        operator.account = Some("primary".to_owned());
        operator.command_tier = CommandTier::Operator;
        let router = MessageRouter::new(vec![operator], None);
        let mut message = dm_message("+15550100");
        message.account_id = Some("primary".to_owned());

        assert_eq!(
            router
                .resolve(&message)
                .expect("legacy route still resolves")
                .command_tier,
            CommandTier::Public
        );
    }

    #[test]
    fn explicit_source_kind_beats_overlapping_legacy_route_in_either_order() {
        let legacy = binding("signal", "shared-id", "legacy-nous");
        let mut direct = binding("signal", "shared-id", "direct-nous");
        direct.source_kind = Some(ChannelSourceKind::Direct);

        for bindings in [
            vec![legacy.clone(), direct.clone()],
            vec![direct.clone(), legacy.clone()],
        ] {
            let router = MessageRouter::new(bindings, None);
            assert_eq!(
                router
                    .resolve(&dm_message("shared-id"))
                    .expect("specific direct route")
                    .nous_id,
                "direct-nous"
            );
            assert_eq!(
                router
                    .resolve(&group_message("alice", "shared-id"))
                    .expect("legacy group route")
                    .nous_id,
                "legacy-nous"
            );
        }
    }

    #[test]
    fn cross_leg_specificity_tie_fails_closed_in_either_order() {
        let mut direct = binding("signal", "shared-id", "direct-nous");
        direct.source_kind = Some(ChannelSourceKind::Direct);
        let mut account_legacy = binding("signal", "shared-id", "account-nous");
        account_legacy.account = Some("primary".to_owned());
        let mut message = dm_message("shared-id");
        message.account_id = Some("primary".to_owned());

        for bindings in [
            vec![direct.clone(), account_legacy.clone()],
            vec![account_legacy.clone(), direct.clone()],
        ] {
            assert!(
                MessageRouter::new(bindings, None)
                    .resolve(&message)
                    .is_none()
            );
        }
    }

    #[test]
    fn broad_or_group_operator_grants_are_clamped_public() {
        let mut unscoped_dm = binding("signal", "+15550100", "dm-nous");
        unscoped_dm.source_kind = Some(ChannelSourceKind::Direct);
        unscoped_dm.command_tier = CommandTier::Operator;
        assert_eq!(
            MessageRouter::new(vec![unscoped_dm], None)
                .resolve(&dm_message("+15550100"))
                .expect("unscoped DM route")
                .command_tier,
            CommandTier::Public
        );

        let mut wildcard = binding("signal", "*", "wildcard-nous");
        wildcard.account = Some("primary".to_owned());
        wildcard.source_kind = Some(ChannelSourceKind::Direct);
        wildcard.command_tier = CommandTier::Operator;
        let mut wildcard_message = dm_message("+15550100");
        wildcard_message.account_id = Some("primary".to_owned());
        assert_eq!(
            MessageRouter::new(vec![wildcard], None)
                .resolve(&wildcard_message)
                .expect("wildcard route")
                .command_tier,
            CommandTier::Public
        );

        let mut group_binding = binding("signal", "group-xyz", "group-nous");
        group_binding.account = Some("primary".to_owned());
        group_binding.source_kind = Some(ChannelSourceKind::Group);
        group_binding.command_tier = CommandTier::Operator;
        let mut message = group_message("+15550100", "group-xyz");
        message.account_id = Some("primary".to_owned());
        assert_eq!(
            MessageRouter::new(vec![group_binding], None)
                .resolve(&message)
                .expect("group route")
                .command_tier,
            CommandTier::Public
        );

        assert_eq!(
            MessageRouter::new(vec![], Some("global-nous".to_owned()))
                .resolve(&dm_message("+15550100"))
                .expect("global route")
                .command_tier,
            CommandTier::Public
        );
    }

    #[test]
    fn wildcard_or_empty_identity_text_cannot_grant_operator_tier() {
        let mut wildcard = binding("signal", "*", "wildcard-nous");
        wildcard.account = Some("primary".to_owned());
        wildcard.source_kind = Some(ChannelSourceKind::Direct);
        wildcard.command_tier = CommandTier::Operator;
        let mut wildcard_sender = dm_message("*");
        wildcard_sender.account_id = Some("primary".to_owned());
        assert_eq!(
            MessageRouter::new(vec![wildcard], None)
                .resolve(&wildcard_sender)
                .expect("wildcard-text route")
                .command_tier,
            CommandTier::Public
        );

        let mut empty = binding("signal", "", "empty-nous");
        empty.account = Some(String::new());
        empty.source_kind = Some(ChannelSourceKind::Direct);
        empty.command_tier = CommandTier::Operator;
        let mut empty_sender = dm_message("");
        empty_sender.account_id = Some(String::new());
        assert_eq!(
            MessageRouter::new(vec![empty], None)
                .resolve(&empty_sender)
                .expect("empty-text route")
                .command_tier,
            CommandTier::Public
        );
    }

    #[test]
    fn participant_bearing_operator_dm_is_clamped_public() {
        let mut malformed = binding("signal", "+15550100", "ops-nous");
        malformed.account = Some("primary".to_owned());
        malformed.source_kind = Some(ChannelSourceKind::Direct);
        malformed.participants = vec!["+15550100".to_owned()];
        malformed.command_tier = CommandTier::Operator;
        let mut message = dm_message("+15550100");
        message.account_id = Some("primary".to_owned());

        assert_eq!(
            MessageRouter::new(vec![malformed], None)
                .resolve(&message)
                .expect("route remains available")
                .command_tier,
            CommandTier::Public
        );
    }

    #[test]
    fn source_binding_is_dm_only() {
        let router = MessageRouter::new(
            vec![
                binding("signal", "+15550100", "dm-nous"),
                binding("signal", "*", "group-default"),
            ],
            None,
        );
        let message = group_message("+15550100", "unbound-group");

        let decision = router.resolve(&message).expect("group default");
        assert_eq!(decision.nous_id, "group-default");
        assert_eq!(decision.matched_by, MatchReason::ChannelDefault);
        assert_eq!(decision.command_tier, CommandTier::Public);
    }

    #[test]
    fn equal_specificity_divergence_fails_closed_in_either_order() {
        let first = binding("signal", "+15550100", "first-nous");
        let second = binding("signal", "+15550100", "second-nous");
        let wildcard = binding("signal", "*", "wildcard-nous");
        let message = dm_message("+15550100");

        for bindings in [
            vec![first.clone(), second.clone(), wildcard.clone()],
            vec![second.clone(), first.clone(), wildcard.clone()],
        ] {
            assert!(
                MessageRouter::new(bindings, Some("global-nous".to_owned()))
                    .resolve(&message)
                    .is_none(),
                "ambiguous exact identity must not fall through"
            );
        }
    }

    #[test]
    fn equal_specificity_authority_divergence_fails_closed_in_either_order() {
        let mut public = binding("signal", "+15550100", "same-nous");
        public.account = Some("primary".to_owned());
        public.source_kind = Some(ChannelSourceKind::Direct);
        let mut operator = public.clone();
        operator.command_tier = CommandTier::Operator;
        let mut message = dm_message("+15550100");
        message.account_id = Some("primary".to_owned());

        for bindings in [
            vec![public.clone(), operator.clone()],
            vec![operator.clone(), public.clone()],
        ] {
            assert!(
                MessageRouter::new(bindings, None)
                    .resolve(&message)
                    .is_none()
            );
        }
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
    fn account_scoped_binding_beats_unscoped_in_either_order() {
        let mut scoped = binding("signal", "*", "work-nous");
        scoped.account = Some("work".to_owned());
        let unscoped = binding("signal", "*", "any-nous");

        for bindings in [
            vec![scoped.clone(), unscoped.clone()],
            vec![unscoped.clone(), scoped.clone()],
        ] {
            let router = MessageRouter::new(bindings, None);
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
            assert_eq!(decision.command_tier, CommandTier::Public);
        }
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
