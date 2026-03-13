//! Message routing — resolves inbound messages to nous targets.

use tracing::debug;

use aletheia_taxis::config::ChannelBinding;

use crate::types::InboundMessage;

/// A resolved routing decision.
///
/// Borrows `nous_id` from the router's binding data. The `session_key` is
/// always freshly expanded, so it remains owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecision<'a> {
    /// The nous agent that should handle this message.
    pub nous_id: &'a str,
    /// Session key derived from template expansion (e.g., `signal:+1234567890`).
    pub session_key: String,
    /// How the routing decision was determined.
    pub matched_by: MatchReason,
}

/// How the routing decision was made.
#[derive(Debug, Clone, PartialEq, Eq)]
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
pub struct MessageRouter {
    bindings: Vec<ChannelBinding>,
    default_nous: Option<String>,
}

impl MessageRouter {
    /// Build a router from channel bindings and an optional global default nous.
    pub fn new(bindings: Vec<ChannelBinding>, default_nous: Option<String>) -> Self {
        Self {
            bindings,
            default_nous,
        }
    }

    /// Resolve which nous should handle this message.
    pub fn resolve(&self, msg: &InboundMessage) -> Option<RouteDecision<'_>> {
        let decision = self.match_route(msg);
        if let Some(ref d) = decision {
            debug!(nous_id = %d.nous_id, matched_by = ?d.matched_by, "message routed");
        }
        decision
    }

    fn match_route(&self, msg: &InboundMessage) -> Option<RouteDecision<'_>> {
        // Priority 1: exact group match (channel + group_id)
        if let Some(group_id) = &msg.group_id {
            for b in &self.bindings {
                if b.channel == msg.channel && b.source == *group_id {
                    return Some(RouteDecision {
                        nous_id: &b.nous_id,
                        session_key: expand_session_key(&b.session_key, msg),
                        matched_by: MatchReason::GroupBinding,
                    });
                }
            }
        }

        // Priority 2: exact source match (channel + sender)
        for b in &self.bindings {
            if b.channel == msg.channel && b.source == msg.sender {
                return Some(RouteDecision {
                    nous_id: &b.nous_id,
                    session_key: expand_session_key(&b.session_key, msg),
                    matched_by: MatchReason::SourceBinding,
                });
            }
        }

        // Priority 3: channel default (source = "*")
        for b in &self.bindings {
            if b.channel == msg.channel && b.source == "*" {
                return Some(RouteDecision {
                    nous_id: &b.nous_id,
                    session_key: expand_session_key(&b.session_key, msg),
                    matched_by: MatchReason::ChannelDefault,
                });
            }
        }

        // Priority 4: global default
        self.default_nous.as_deref().map(|id| RouteDecision {
            nous_id: id,
            session_key: expand_session_key("{source}", msg),
            matched_by: MatchReason::GlobalDefault,
        })
    }
}

/// Expand session key template placeholders.
fn expand_session_key(template: &str, msg: &InboundMessage) -> String {
    template
        .replace("{source}", &msg.sender)
        .replace("{group}", msg.group_id.as_deref().unwrap_or("dm"))
}

/// Determine reply target for outbound response.
///
/// Group messages reply to the group. DMs reply to the sender.
pub fn reply_target(msg: &InboundMessage) -> String {
    match &msg.group_id {
        Some(group) => format!("group:{group}"),
        None => msg.sender.clone(),
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
}
