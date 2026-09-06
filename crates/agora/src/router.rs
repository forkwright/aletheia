//! Message routing: resolves inbound messages to nous targets.

use tracing::debug;

use taxis::config::{ChannelBinding, CommandTier, InboundMessagePolicy};

use crate::types::InboundMessage;

/// A resolved routing decision.
///
/// Borrows `nous_id` from the router's binding data. The `session_key` is
/// always freshly expanded, so it remains owned.
#[derive(Debug, Clone, PartialEq, Eq)] // kanon:ignore RUST/no-debug-derive-on-public-types WHY: session_key is a routing template expansion (non-sensitive); MatchReason, nous_id, and command_tier are non-sensitive
pub struct RouteDecision<'a> {
    /// The nous agent that should handle this message.
    pub nous_id: &'a str,
    /// Session key derived from template expansion (e.g., `signal:+1234567890`).
    pub session_key: String, // kanon:ignore RUST/plain-string-secret WHY: session_key is a routing key (channel:sender template expansion), not a credential
    /// How the routing decision was determined.
    pub matched_by: MatchReason,
    /// Which `!`-command tier this route grants the sender.
    ///
    /// WHY explicit-over-wildcard precedence (decision record,
    /// forkwright/aletheia#5193): this is the binding's declared
    /// `command_tier` ONLY for [`MatchReason::GroupBinding`] and
    /// [`MatchReason::SourceBinding`] (an exact match). For
    /// [`MatchReason::ChannelDefault`] (wildcard `source = "*"`) and
    /// [`MatchReason::GlobalDefault`] (no binding at all), this is always
    /// [`CommandTier::Public`] regardless of what a matched wildcard
    /// binding's `command_tier` field says -- a wildcard/default route can
    /// never grant the operator tier.
    pub command_tier: CommandTier,
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
/// Within each of the first three tiers, a binding additionally matches
/// only if its `receiving_account_id` is unset (matches any account) or
/// equals `msg.receiving_account_id` -- see [`account_matches`]. An
/// account-scoped binding that does not match the message's receiving
/// account is skipped, not treated as a partial match, so a broader
/// binding at the same or a lower-priority tier can still apply.
pub struct MessageRouter {
    bindings: Vec<ChannelBinding>,
    default_nous: Option<String>,
    /// Inbound participant allowlist, checked by [`Self::allows_sender`].
    /// Defaults to [`InboundMessagePolicy::default`]'s fail-closed posture.
    inbound_policy: InboundMessagePolicy,
}

impl MessageRouter {
    /// Build a router from channel bindings and an optional global default nous.
    #[must_use]
    pub fn new(bindings: Vec<ChannelBinding>, default_nous: Option<String>) -> Self {
        Self {
            bindings,
            default_nous,
            inbound_policy: InboundMessagePolicy::default(),
        }
    }

    /// Set the inbound participant-allowlist policy, checked by
    /// [`Self::allows_sender`].
    #[must_use]
    pub fn with_inbound_policy(mut self, policy: InboundMessagePolicy) -> Self {
        self.inbound_policy = policy;
        self
    }

    /// Whether `msg`'s sender is permitted to be routed at all, per the
    /// inbound participant allowlist (`taxis::config::InboundMessagePolicy`).
    ///
    /// WHY a separate, mandatory pre-check rather than folding into
    /// [`Self::resolve`] (decision record, forkwright/aletheia#5193): a
    /// denied sender is not "no route" -- that could also mean nothing
    /// more than a missing binding, a config gap rather than a refusal.
    /// Keeping this a distinct, named check that the caller must branch on
    /// (see `aletheia::dispatch::dispatch_one`) makes the denial an
    /// explicit, loggable event instead of collapsing it into the generic
    /// "no match" path.
    #[must_use]
    pub fn allows_sender(&self, msg: &InboundMessage) -> bool {
        self.inbound_policy.allows(&msg.channel, &msg.sender)
    }

    /// Resolve which nous should handle this message.
    ///
    /// Does NOT consult the inbound participant allowlist -- callers must
    /// check [`Self::allows_sender`] first (see that method's docs for
    /// why the two are kept separate).
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
        let receiving_account = msg.receiving_account_id.as_deref();

        // NOTE: Priority 1: exact group match (channel + group_id)
        if let Some(group_id) = &msg.group_id {
            for b in &self.bindings {
                if b.channel == msg.channel
                    && b.source == *group_id
                    && account_matches(b.receiving_account_id.as_deref(), receiving_account)
                {
                    return Some(RouteDecision {
                        nous_id: &b.nous_id,
                        session_key: expand_session_key(&b.session_key, msg),
                        matched_by: MatchReason::GroupBinding,
                        command_tier: b.command_tier,
                    });
                }
            }
        }

        // NOTE: Priority 2: exact source match (channel + sender)
        for b in &self.bindings {
            if b.channel == msg.channel
                && b.source == msg.sender
                && account_matches(b.receiving_account_id.as_deref(), receiving_account)
            {
                return Some(RouteDecision {
                    nous_id: &b.nous_id,
                    session_key: expand_session_key(&b.session_key, msg),
                    matched_by: MatchReason::SourceBinding,
                    command_tier: b.command_tier,
                });
            }
        }

        // NOTE: Priority 3: channel default (source = "*"). WHY always
        // Public (explicit-over-wildcard, #5193): a wildcard binding can
        // never grant the operator command tier, regardless of its
        // configured `command_tier` -- see `RouteDecision::command_tier`.
        for b in &self.bindings {
            if b.channel == msg.channel
                && b.source == "*"
                && account_matches(b.receiving_account_id.as_deref(), receiving_account)
            {
                return Some(RouteDecision {
                    nous_id: &b.nous_id,
                    session_key: expand_session_key(&b.session_key, msg),
                    matched_by: MatchReason::ChannelDefault,
                    command_tier: CommandTier::Public,
                });
            }
        }

        // NOTE: Priority 4: global default. No binding backs this route,
        // so -- same as the wildcard tier -- it is always Public.
        self.default_nous.as_deref().map(|id| RouteDecision {
            nous_id: id,
            session_key: expand_session_key("{source}", msg),
            matched_by: MatchReason::GlobalDefault,
            command_tier: CommandTier::Public,
        })
    }
}

/// Whether a binding scoped to `binding_account` may match a message
/// received on `msg_account`.
///
/// `binding_account: None` matches any (or no) message account -- an
/// unscoped binding. `Some(_)` matches only the identical account; it
/// never matches `msg_account: None`, since an account-scoped binding on a
/// provider that has not been wired to carry `receiving_account_id` yet
/// must not silently start matching everything.
fn account_matches(binding_account: Option<&str>, msg_account: Option<&str>) -> bool {
    match binding_account {
        None => true,
        Some(bound) => Some(bound) == msg_account,
    }
}

/// Expand session key template placeholders, then fold in the receiving
/// account structurally.
///
/// WHY the account is folded, not a `{account}` template placeholder
/// (decision record, forkwright/aletheia#5193 -- "no session-key template
/// expansion across accounts"): `{source}` and `{group}` are operator-
/// authored template content the operator could omit or reorder; the
/// account boundary between two accounts on the same provider must not be
/// something a template can accidentally drop. So it is prepended here,
/// after template expansion, unconditionally when
/// `msg.receiving_account_id` is set -- a literal `{account}` in a
/// template is therefore NEVER substituted; it passes through as inert
/// text, distinct from `{source}`/`{group}`.
///
/// This changes the session key for any message that carries a receiving
/// account where it previously did not (a live-conversation-history
/// migration note, not a config edit -- see the ADR).
fn expand_session_key(template: &str, msg: &InboundMessage) -> String {
    let expanded = template
        .replace("{source}", &msg.sender)
        .replace("{group}", msg.group_id.as_deref().unwrap_or("dm"));
    match msg.receiving_account_id.as_deref() {
        Some(account) if !account.is_empty() => format!("{account}:{expanded}"),
        _ => expanded,
    }
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
            message_id: None,
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            receiving_account_id: None,
            raw: None,
        }
    }

    fn group_message(sender: &str, group_id: &str) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: sender.to_owned(),
            sender_name: None,
            group_id: Some(group_id.to_owned()),
            message_id: None,
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            receiving_account_id: None,
            raw: None,
        }
    }

    fn binding(channel: &str, source: &str, nous_id: &str) -> ChannelBinding {
        ChannelBinding {
            channel: channel.to_owned(),
            source: source.to_owned(),
            nous_id: nous_id.to_owned(),
            session_key: "{source}".to_owned(),
            receiving_account_id: None,
            command_tier: CommandTier::default(),
        }
    }

    fn message_from_account(sender: &str, account: &str) -> InboundMessage {
        let mut msg = dm_message(sender);
        msg.receiving_account_id = Some(account.to_owned());
        msg
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
        let router = MessageRouter::new(vec![binding("signal", "group-xyz", "group-nous")], None);
        let msg_a = group_message("+15550100", "group-xyz");
        let msg_b = group_message("+15550199", "group-xyz");
        let decision_a = router.resolve(&msg_a).expect("should match");
        let decision_b = router.resolve(&msg_b).expect("should match");
        assert_eq!(decision_a.nous_id, "group-nous");
        assert_eq!(decision_b.nous_id, "group-nous");
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

    // ── account matching (#5193 prerequisite) ──

    #[test]
    fn account_scoped_binding_matches_its_own_account() {
        let mut b = binding("signal", "+15550100", "work-nous");
        b.receiving_account_id = Some("work".to_owned());
        let router = MessageRouter::new(vec![b], None);
        let msg = message_from_account("+15550100", "work");
        let decision = router.resolve(&msg).expect("should match");
        assert_eq!(decision.nous_id, "work-nous");
    }

    #[test]
    fn account_scoped_binding_does_not_match_a_different_account() {
        let mut b = binding("signal", "+15550100", "work-nous");
        b.receiving_account_id = Some("work".to_owned());
        let router = MessageRouter::new(vec![b], None);
        let msg = message_from_account("+15550100", "personal");
        assert!(
            router.resolve(&msg).is_none(),
            "an account-scoped binding must not leak its grant to a different account"
        );
    }

    #[test]
    fn account_scoped_binding_falls_through_to_unscoped_wildcard() {
        let mut scoped = binding("signal", "+15550100", "work-nous");
        scoped.receiving_account_id = Some("work".to_owned());
        let wildcard = binding("signal", "*", "catchall-nous");
        let router = MessageRouter::new(vec![scoped, wildcard], None);
        let msg = message_from_account("+15550100", "personal");
        let decision = router.resolve(&msg).expect("wildcard should still match");
        assert_eq!(decision.nous_id, "catchall-nous");
        assert_eq!(decision.matched_by, MatchReason::ChannelDefault);
    }

    #[test]
    fn unscoped_binding_matches_any_account() {
        let b = binding("signal", "+15550100", "syn");
        let router = MessageRouter::new(vec![b], None);
        assert!(
            router
                .resolve(&message_from_account("+15550100", "work"))
                .is_some()
        );
        assert!(
            router
                .resolve(&message_from_account("+15550100", "personal"))
                .is_some()
        );
        assert!(router.resolve(&dm_message("+15550100")).is_some());
    }

    // ── command-tier grant, explicit-over-wildcard precedence (PROOF, #5193) ──

    #[test]
    fn exact_source_binding_grants_its_declared_command_tier() {
        let mut b = binding("signal", "+15550100", "syn");
        b.command_tier = CommandTier::Operator;
        let router = MessageRouter::new(vec![b], None);
        let decision = router
            .resolve(&dm_message("+15550100"))
            .expect("should match");
        assert_eq!(decision.command_tier, CommandTier::Operator);
        assert_eq!(decision.matched_by, MatchReason::SourceBinding);
    }

    #[test]
    fn exact_group_binding_grants_its_declared_command_tier() {
        let mut b = binding("signal", "group-abc", "syn");
        b.command_tier = CommandTier::Operator;
        let router = MessageRouter::new(vec![b], None);
        let decision = router
            .resolve(&group_message("+15550100", "group-abc"))
            .expect("should match");
        assert_eq!(decision.command_tier, CommandTier::Operator);
    }

    #[test]
    fn wildcard_binding_never_grants_operator_tier_even_if_configured() {
        let mut b = binding("signal", "*", "syn");
        // WHY set anyway: proves the router itself refuses to honor this,
        // not merely that operators are expected not to set it -- config
        // validation (taxis::validate) is a second, independent guard.
        b.command_tier = CommandTier::Operator;
        let router = MessageRouter::new(vec![b], None);
        let decision = router
            .resolve(&dm_message("+15550100"))
            .expect("should match");
        assert_eq!(
            decision.command_tier,
            CommandTier::Public,
            "explicit-over-wildcard: a wildcard binding cannot grant the operator tier"
        );
    }

    #[test]
    fn global_default_never_grants_operator_tier() {
        let router = MessageRouter::new(vec![], Some("fallback-nous".to_owned()));
        let decision = router
            .resolve(&dm_message("+15550100"))
            .expect("should match");
        assert_eq!(decision.command_tier, CommandTier::Public);
    }

    #[test]
    fn no_command_tier_configured_defaults_to_public() {
        let b = binding("signal", "+15550100", "syn");
        let router = MessageRouter::new(vec![b], None);
        let decision = router
            .resolve(&dm_message("+15550100"))
            .expect("should match");
        assert_eq!(decision.command_tier, CommandTier::Public);
    }

    #[test]
    fn explicit_source_binding_outranks_a_coexisting_operator_wildcard() {
        // WHY this is the sharpest form of "explicit over wildcard": an
        // Operator-tier wildcard binding and a Public-tier (default) exact
        // binding both exist for the same sender. The exact binding must
        // win the match (existing priority order) AND its own (lower)
        // tier is what applies -- the wildcard's Operator grant never
        // leaks through by virtue of coexisting in the same binding list.
        let mut wildcard = binding("signal", "*", "wildcard-nous");
        wildcard.command_tier = CommandTier::Operator;
        let exact = binding("signal", "+15550100", "exact-nous");
        let router = MessageRouter::new(vec![wildcard, exact], None);
        let decision = router
            .resolve(&dm_message("+15550100"))
            .expect("should match");
        assert_eq!(decision.matched_by, MatchReason::SourceBinding);
        assert_eq!(decision.command_tier, CommandTier::Public);
    }

    // ── session-key account fold (PROOF: cross-account template never expands, #5193) ──

    #[test]
    fn session_key_folds_in_receiving_account_as_a_structural_prefix() {
        let mut b = binding("signal", "+15550100", "syn");
        b.session_key = "signal:{source}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let decision = router
            .resolve(&message_from_account("+15550100", "work"))
            .expect("should match");
        assert_eq!(decision.session_key, "work:signal:+15550100");
    }

    #[test]
    fn session_key_without_receiving_account_is_unchanged() {
        let mut b = binding("signal", "+15550100", "syn");
        b.session_key = "signal:{source}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let decision = router
            .resolve(&dm_message("+15550100"))
            .expect("should match");
        assert_eq!(decision.session_key, "signal:+15550100");
    }

    #[test]
    fn same_sender_different_accounts_get_different_session_keys() {
        // WHY(#5193 decision "why"): before this fold, the same sender
        // messaging two accounts on a multi-account deployment collapsed
        // onto one session key -- conversation history bled across
        // accounts with no config able to prevent it.
        let mut b = binding("signal", "+15550100", "syn");
        b.session_key = "signal:{source}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let work = router
            .resolve(&message_from_account("+15550100", "work"))
            .expect("should match")
            .session_key;
        let personal = router
            .resolve(&message_from_account("+15550100", "personal"))
            .expect("should match")
            .session_key;
        assert_ne!(work, personal);
    }

    #[test]
    fn literal_account_placeholder_in_template_never_expands() {
        // WHY(#5193 decision "no session-key template expansion across
        // accounts"): only `{source}` and `{group}` are substitutable
        // template placeholders. A literal `{account}` written into an
        // operator's session_key template is not a recognized placeholder
        // -- it must pass through unexpanded, distinct from the
        // structural account prefix expand_session_key always applies.
        let mut b = binding("signal", "+15550100", "syn");
        b.session_key = "{account}-{source}".to_owned();
        let router = MessageRouter::new(vec![b], None);
        let decision = router
            .resolve(&message_from_account("+15550100", "work"))
            .expect("should match");
        assert_eq!(decision.session_key, "work:{account}-+15550100");
    }

    // ── inbound participant allowlist (PROOF: denied by default, surfaced not silent, #5193) ──

    #[test]
    fn allows_sender_denies_by_default() {
        let router = MessageRouter::new(vec![binding("signal", "*", "syn")], None);
        assert!(
            !router.allows_sender(&dm_message("+15550100")),
            "an unconfigured inbound policy must fail closed"
        );
    }

    #[test]
    fn allows_sender_permits_an_allowlisted_sender() {
        let mut policy = InboundMessagePolicy::default();
        policy
            .allowlist
            .insert("signal".to_owned(), vec!["+15550100".to_owned()]);
        let router = MessageRouter::new(vec![binding("signal", "*", "syn")], None)
            .with_inbound_policy(policy);
        assert!(router.allows_sender(&dm_message("+15550100")));
        assert!(!router.allows_sender(&dm_message("+15559999")));
    }

    #[test]
    fn allows_sender_is_independent_of_resolve() {
        // WHY: allows_sender and resolve are deliberately decoupled (see
        // MessageRouter::resolve's docs) -- a denied sender can still
        // resolve to a route; the caller is responsible for checking
        // allows_sender first and never dispatching when it returns false.
        let router = MessageRouter::new(vec![binding("signal", "*", "syn")], None);
        let msg = dm_message("+15550100");
        assert!(!router.allows_sender(&msg));
        assert!(router.resolve(&msg).is_some());
    }
}
