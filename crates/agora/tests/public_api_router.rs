#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    unused_imports,
    reason = "split public_api_*.rs files share the same import block; not every file uses every item"
)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use agora::registry::ChannelRegistry;
use agora::router::{MatchReason, MessageRouter, RouteDecision, reply_target};
use agora::semeion::client::{SendParams as SignalSendParams, SignalClient};
use agora::semeion::connection::{ConnectionHealthReport, ConnectionState};
use agora::semeion::envelope::{Attachment, DataMessage, GroupInfo, SignalEnvelope};
use agora::semeion::error::Error as SignalError;
use agora::semeion::{SignalProvider, SignalTarget, parse_target};
use agora::types::{
    ChannelCapabilities, ChannelProvider, InboundMessage, ProbeResult, SendParams, SendResult,
};
use taxis::config::ChannelBinding;
use tokio_util::sync::CancellationToken;

// Split: MessageRouter + helpers.

// MessageRouter
// ---------------------------------------------------------------------------

fn make_binding(channel: &str, source: &str, nous_id: &str) -> ChannelBinding {
    ChannelBinding {
        channel: channel.to_owned(),
        source: source.to_owned(),
        nous_id: nous_id.to_owned(),
        session_key: "{source}".to_owned(),
    }
}

fn make_dm_message(sender: &str) -> InboundMessage {
    InboundMessage {
        channel: "signal".to_owned(),
        sender: sender.to_owned(),
        sender_name: None,
        group_id: None,
        text: "hello".to_owned(),
        timestamp: 1_709_312_345_678,
        attachments: vec![],
        raw: None,
    }
}

fn make_group_message(sender: &str, group_id: &str) -> InboundMessage {
    InboundMessage {
        channel: "signal".to_owned(),
        sender: sender.to_owned(),
        sender_name: None,
        group_id: Some(group_id.to_owned()),
        text: "group hello".to_owned(),
        timestamp: 1_709_312_345_678,
        attachments: vec![],
        raw: None,
    }
}

#[test]
fn router_new_stores_bindings() {
    let bindings = vec![
        make_binding("signal", "*", "default-nous"),
        make_binding("signal", "+1234567890", "personal-nous"),
    ];

    let router = MessageRouter::new(bindings, Some("global".to_owned()));
    // Router construction succeeds; internal state is opaque but behavior is testable
    let _ = router;
}

#[test]
fn router_resolve_exact_group_binding() {
    let bindings = vec![make_binding("signal", "group-abc", "group-nous")];
    let router = MessageRouter::new(bindings, None);

    let msg = make_group_message("+1234567890", "group-abc");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.nous_id, "group-nous");
    assert!(matches!(decision.matched_by, MatchReason::GroupBinding));
}

#[test]
fn router_resolve_exact_source_binding() {
    let bindings = vec![make_binding("signal", "+1234567890", "alice-nous")];
    let router = MessageRouter::new(bindings, None);

    let msg = make_dm_message("+1234567890");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.nous_id, "alice-nous");
    assert!(matches!(decision.matched_by, MatchReason::SourceBinding));
}

#[test]
fn router_resolve_channel_default_wildcard() {
    let bindings = vec![make_binding("signal", "*", "catchall-nous")];
    let router = MessageRouter::new(bindings, None);

    let msg = make_dm_message("+9999999999");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.nous_id, "catchall-nous");
    assert!(matches!(decision.matched_by, MatchReason::ChannelDefault));
}

#[test]
fn router_resolve_global_default_fallback() {
    let router = MessageRouter::new(vec![], Some("global-nous".to_owned()));

    let msg = make_dm_message("+1234567890");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.nous_id, "global-nous");
    assert!(matches!(decision.matched_by, MatchReason::GlobalDefault));
}

#[test]
fn router_resolve_no_match_returns_none() {
    let router = MessageRouter::new(vec![], None);

    let msg = make_dm_message("+1234567890");
    assert!(router.resolve(&msg).is_none());
}

#[test]
fn router_resolve_group_binding_takes_priority() {
    // Group binding should match even when source also matches
    let bindings = vec![
        make_binding("signal", "+1234567890", "source-nous"),
        make_binding("signal", "group-abc", "group-nous"),
    ];
    let router = MessageRouter::new(bindings, None);

    let msg = make_group_message("+1234567890", "group-abc");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.nous_id, "group-nous");
    assert!(matches!(decision.matched_by, MatchReason::GroupBinding));
}

#[test]
fn router_resolve_source_binding_takes_priority_over_wildcard() {
    let bindings = vec![
        make_binding("signal", "*", "wildcard-nous"),
        make_binding("signal", "+1234567890", "specific-nous"),
    ];
    let router = MessageRouter::new(bindings, None);

    let msg = make_dm_message("+1234567890");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.nous_id, "specific-nous");
    assert!(matches!(decision.matched_by, MatchReason::SourceBinding));
}

#[test]
fn router_session_key_source_interpolation() {
    let mut binding = make_binding("signal", "*", "nous");
    binding.session_key = "signal:{source}".to_owned();

    let router = MessageRouter::new(vec![binding], None);
    let msg = make_dm_message("+1234567890");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.session_key, "signal:+1234567890");
}

#[test]
fn router_session_key_group_interpolation() {
    let mut binding = make_binding("signal", "group-abc", "nous");
    binding.session_key = "signal:{group}".to_owned();

    let router = MessageRouter::new(vec![binding], None);
    let msg = make_group_message("+1234567890", "group-abc");
    let decision = router.resolve(&msg).expect("should match");

    assert_eq!(decision.session_key, "signal:group-abc");
}

#[test]
fn router_session_key_both_placeholders() {
    let mut binding = make_binding("signal", "*", "nous");
    binding.session_key = "{source}:{group}".to_owned();

    let router = MessageRouter::new(vec![binding], None);

    // DM: group placeholder becomes "dm"
    let dm = make_dm_message("+1234567890");
    let dm_decision = router.resolve(&dm).expect("should match");
    assert_eq!(dm_decision.session_key, "+1234567890:dm");

    // Group message: group placeholder is actual group id
    let group = make_group_message("+1234567890", "my-group");
    let group_decision = router.resolve(&group).expect("should match");
    assert_eq!(group_decision.session_key, "+1234567890:my-group");
}

#[test]
fn reply_target_dm_returns_sender() {
    let msg = make_dm_message("+1234567890");
    assert_eq!(reply_target(&msg), "+1234567890");
}

#[test]
fn reply_target_group_returns_group_prefix() {
    let msg = make_group_message("+1234567890", "group-xyz");
    assert_eq!(reply_target(&msg), "group:group-xyz");
}

#[test]
fn route_decision_equality() {
    let binding = make_binding("signal", "*", "nous");

    let decision1 = RouteDecision {
        nous_id: &binding.nous_id,
        session_key: "key1".to_owned(),
        matched_by: MatchReason::ChannelDefault,
    };

    let decision2 = RouteDecision {
        nous_id: &binding.nous_id,
        session_key: "key1".to_owned(),
        matched_by: MatchReason::ChannelDefault,
    };

    let decision3 = RouteDecision {
        nous_id: &binding.nous_id,
        session_key: "key2".to_owned(),
        matched_by: MatchReason::GlobalDefault,
    };

    assert_eq!(decision1, decision2);
    assert_ne!(decision1, decision3);
}

#[test]
fn match_reason_variants_distinct() {
    // WHY: MatchReason is #[non_exhaustive] and public. The variants must
    // remain distinct for pattern matching.
    let r1 = MatchReason::GroupBinding;
    let r2 = MatchReason::SourceBinding;
    let r3 = MatchReason::ChannelDefault;
    let r4 = MatchReason::GlobalDefault;

    assert_ne!(r1, r2);
    assert_ne!(r1, r3);
    assert_ne!(r1, r4);
    assert_ne!(r2, r3);
    assert_ne!(r2, r4);
    assert_ne!(r3, r4);
}

// ---------------------------------------------------------------------------
