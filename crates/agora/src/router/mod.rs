//! Message routing: resolves inbound messages to nous targets.

use tracing::debug;

use taxis::config::{ChannelBinding, CommandTier, GroupParticipantPolicy, InboundMessagePolicy};

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
///
/// Does NOT consult the per-group participant allowlist
/// ([`GroupParticipantPolicy`], forkwright/aletheia#5194) -- callers must
/// check [`Self::group_participant_allows`] first, mirroring
/// [`Self::allows_sender`]'s precheck contract, so a sender rejected from
/// a restricted group is refused outright rather than silently falling
/// through to a channel default or the global default nous.
pub struct MessageRouter {
    bindings: Vec<ChannelBinding>,
    default_nous: Option<String>,
    /// Inbound participant allowlist, checked by [`Self::allows_sender`].
    /// Defaults to [`InboundMessagePolicy::default`]'s fail-closed posture.
    inbound_policy: InboundMessagePolicy,
    /// Per-group participant allowlist for [`MatchReason::GroupBinding`]
    /// matches, checked by [`Self::group_participant_allows`]. Defaults to
    /// unrestricted (any sender), preserving pre-#5194 behavior for groups
    /// with no allowlist entry.
    group_participants: GroupParticipantPolicy,
}

impl MessageRouter {
    /// Build a router from channel bindings and an optional global default nous.
    #[must_use]
    pub fn new(bindings: Vec<ChannelBinding>, default_nous: Option<String>) -> Self {
        Self {
            bindings,
            default_nous,
            inbound_policy: InboundMessagePolicy::default(),
            group_participants: GroupParticipantPolicy::default(),
        }
    }

    /// Set the inbound participant-allowlist policy, checked by
    /// [`Self::allows_sender`].
    #[must_use]
    pub fn with_inbound_policy(mut self, policy: InboundMessagePolicy) -> Self {
        self.inbound_policy = policy;
        self
    }

    /// Set the per-group participant allowlist for [`MatchReason::GroupBinding`]
    /// matches (forkwright/aletheia#5194). See [`GroupParticipantPolicy`].
    #[must_use]
    pub fn with_group_participants(mut self, policy: GroupParticipantPolicy) -> Self {
        self.group_participants = policy;
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

    /// Whether `msg`'s sender is permitted to match a
    /// [`MatchReason::GroupBinding`] route, per the per-group participant
    /// allowlist ([`GroupParticipantPolicy`], forkwright/aletheia#5194).
    ///
    /// Returns `true` (no denial) when `msg` has no `group_id`, when no
    /// binding matches `(channel, group_id)` for it, or when that group
    /// has no allowlist entry -- there is nothing to gate in those cases
    /// and [`Self::resolve`]'s normal tier order still applies.
    ///
    /// WHY a separate, mandatory pre-check rather than folding into
    /// [`Self::resolve`]'s fall-through (mirrors [`Self::allows_sender`]'s
    /// #5193 precedent): a sender rejected from an explicitly restricted
    /// group must not silently fall through to that channel's wildcard
    /// default or the global default nous -- either would still hand the
    /// rejected sender an agent, defeating the allowlist's purpose.
    /// Keeping this a distinct, named check the caller must branch on
    /// (see `aletheia::dispatch::dispatch_one`) makes the denial an
    /// explicit, loggable event instead of a silent tier fall-through.
    #[must_use]
    pub fn group_participant_allows(&self, msg: &InboundMessage) -> bool {
        let Some(group_id) = &msg.group_id else {
            return true;
        };
        let receiving_account = msg.receiving_account_id.as_deref();
        let has_matching_group_binding = self.bindings.iter().any(|b| {
            b.channel == msg.channel
                && b.source == *group_id
                && account_matches(b.receiving_account_id.as_deref(), receiving_account)
        });
        if !has_matching_group_binding {
            return true;
        }
        self.group_participants
            .allows(&msg.channel, group_id, &msg.sender)
    }

    /// Resolve which nous should handle this message.
    ///
    /// Does NOT consult the inbound participant allowlist or the per-group
    /// participant allowlist -- callers must check [`Self::allows_sender`]
    /// and [`Self::group_participant_allows`] first (see those methods'
    /// docs for why the checks are kept separate).
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

        // NOTE: Priority 1: exact group match (channel + group_id). The
        // per-group participant allowlist (#5194) is NOT checked here --
        // see Self::group_participant_allows, which callers must check
        // before resolve() so a rejected sender is denied outright rather
        // than falling through to a lower tier.
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
mod tests;
