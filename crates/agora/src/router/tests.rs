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

// ── group participant allowlist (PROOF: denied sender does not fall
// through, not just "resolve() returns None", #5194) ──
// WHY this block renames/inverts the old `group_binding_matches_
// regardless_of_sender`: that test proved any participant in the
// group matched unconditionally -- the exact gap #5194 reported.

fn allowlisted_group_policy() -> GroupParticipantPolicy {
    let mut policy = GroupParticipantPolicy::default();
    policy
        .allowlist
        .entry("signal".to_owned())
        .or_default()
        .insert("group-xyz".to_owned(), vec!["+15550100".to_owned()]);
    policy
}

#[test]
fn group_participant_allows_denies_a_sender_not_in_the_allowlist() {
    let router = MessageRouter::new(vec![binding("signal", "group-xyz", "group-nous")], None)
        .with_group_participants(allowlisted_group_policy());
    assert!(
        !router.group_participant_allows(&group_message("+15559999", "group-xyz")),
        "a sender absent from the group's participant allowlist must be denied"
    );
}

#[test]
fn group_participant_allows_permits_an_allowlisted_sender() {
    let router = MessageRouter::new(vec![binding("signal", "group-xyz", "group-nous")], None)
        .with_group_participants(allowlisted_group_policy());
    assert!(router.group_participant_allows(&group_message("+15550100", "group-xyz")));
}

#[test]
fn group_participant_allows_is_independent_of_resolve() {
    // WHY: mirrors allows_sender_is_independent_of_resolve below --
    // resolve() still matches a denied group participant's binding;
    // the caller (aletheia::dispatch::dispatch_one) is responsible for
    // checking group_participant_allows first and never dispatching
    // when it returns false. This is what makes the denial a hard
    // stop rather than a fall-through to a channel default or the
    // global default nous.
    let router = MessageRouter::new(vec![binding("signal", "group-xyz", "group-nous")], None)
        .with_group_participants(allowlisted_group_policy());
    let msg = group_message("+15559999", "group-xyz");
    assert!(!router.group_participant_allows(&msg));
    assert!(router.resolve(&msg).is_some());
}

#[test]
fn group_participant_allows_denies_regardless_of_which_tier_would_route_it() {
    // PROOF (#5194 follow-up): the allowlist entry names a group, not a
    // routing tier. The router here has NO exact `group-xyz` binding --
    // only a channel wildcard (which would otherwise silently resolve
    // the message via MatchReason::ChannelDefault) -- yet a sender
    // absent from the allowlist must still be denied. Before this fix,
    // `group_participant_allows` returned `true` whenever no exact
    // `(channel, group_id)` binding existed, making the allowlist entry
    // a silent no-op for any group reached only through a wildcard or
    // the global default.
    let router = MessageRouter::new(vec![binding("signal", "*", "wildcard-nous")], None)
        .with_group_participants(allowlisted_group_policy());
    let msg = group_message("+15559999", "group-xyz");
    assert!(
        !router.group_participant_allows(&msg),
        "an allowlist entry for (channel, group_id) must gate the sender even with no exact group binding"
    );
    // The wildcard binding still matches at the routing-tier level --
    // group_participant_allows is what the caller must check first.
    assert!(router.resolve(&msg).is_some());
}

#[test]
fn group_binding_with_no_participant_policy_still_matches_any_sender() {
    // Backward compatibility: a group with no GroupParticipantPolicy
    // entry keeps pre-#5194 behavior -- opting in is additive.
    let router = MessageRouter::new(vec![binding("signal", "group-xyz", "group-nous")], None);
    let msg_a = group_message("+15550100", "group-xyz");
    let msg_b = group_message("+15550199", "group-xyz");
    assert!(router.group_participant_allows(&msg_a));
    assert!(router.group_participant_allows(&msg_b));
    assert_eq!(
        router.resolve(&msg_a).expect("should match").nous_id,
        "group-nous"
    );
    assert_eq!(
        router.resolve(&msg_b).expect("should match").nous_id,
        "group-nous"
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
    let router =
        MessageRouter::new(vec![binding("signal", "*", "syn")], None).with_inbound_policy(policy);
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
