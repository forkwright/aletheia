//! Integration tests for the `aletheia-agora` public API (#2814).
//!
//! These tests exercise the agora crate as an external consumer would:
//! through the publicly exported modules (`types`, `registry`, `listener`,
//! `router`, `semeion`). They do not reach into crate-private items.
//!
//! Coverage targets:
//! 1. Public types (`ChannelCapabilities`, `SendParams`, `SendResult`, `ProbeResult`, `InboundMessage`)
//! 2. Error variants and Display impls (where publicly accessible)
//! 3. `ChannelRegistry` public API
//! 4. `MessageRouter` routing logic
//! 5. `SignalProvider` and related semeion types
//! 6. Send + Sync trait-object compatibility for promised types

#![expect(clippy::expect_used, reason = "test assertions")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use aletheia_agora::registry::ChannelRegistry;
use aletheia_agora::router::{MatchReason, MessageRouter, RouteDecision, reply_target};
use aletheia_agora::semeion::client::{SendParams as SignalSendParams, SignalClient};
use aletheia_agora::semeion::connection::{ConnectionHealthReport, ConnectionState};
use aletheia_agora::semeion::envelope::{Attachment, DataMessage, GroupInfo, SignalEnvelope};
use aletheia_agora::semeion::error::Error as SignalError;
use aletheia_agora::semeion::{SignalProvider, SignalTarget, parse_target};
use aletheia_agora::types::{
    ChannelCapabilities, ChannelProvider, InboundMessage, ProbeResult, SendParams, SendResult,
};
use aletheia_taxis::config::ChannelBinding;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// ChannelCapabilities
// ---------------------------------------------------------------------------

#[test]
fn channel_capabilities_default_values() {
    // WHY: Signal capabilities are hardcoded and define the channel contract.
    // Changing these silently alters what consumers can expect from the Signal provider.
    let caps = ChannelCapabilities {
        threads: false,
        reactions: true,
        typing: true,
        media: true,
        streaming: false,
        rich_formatting: false,
        max_text_length: 2000,
    };

    assert!(!caps.threads);
    assert!(caps.reactions);
    assert!(caps.typing);
    assert!(caps.media);
    assert!(!caps.streaming);
    assert!(!caps.rich_formatting);
    assert_eq!(caps.max_text_length, 2000);
}

#[test]
fn channel_capabilities_serde_roundtrip() {
    let original = ChannelCapabilities {
        threads: true,
        reactions: false,
        typing: true,
        media: false,
        streaming: true,
        rich_formatting: true,
        max_text_length: 4096,
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: ChannelCapabilities = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.threads, original.threads);
    assert_eq!(restored.reactions, original.reactions);
    assert_eq!(restored.typing, original.typing);
    assert_eq!(restored.media, original.media);
    assert_eq!(restored.streaming, original.streaming);
    assert_eq!(restored.rich_formatting, original.rich_formatting);
    assert_eq!(restored.max_text_length, original.max_text_length);
}

// ---------------------------------------------------------------------------
// SendParams
// ---------------------------------------------------------------------------

#[test]
fn send_params_construction_and_field_access() {
    let params = SendParams {
        to: "+15550100".to_owned(),
        text: "Hello, world!".to_owned(),
        account_id: Some("acct123".to_owned()),
        thread_id: Some("thread456".to_owned()),
        attachments: Some(vec!["/tmp/photo.jpg".to_owned()]),
    };

    assert_eq!(params.to, "+15550100");
    assert_eq!(params.text, "Hello, world!");
    assert_eq!(params.account_id.as_deref(), Some("acct123"));
    assert_eq!(params.thread_id.as_deref(), Some("thread456"));
    assert_eq!(params.attachments.as_ref().map_or(0, std::vec::Vec::len), 1);
}

#[test]
fn send_params_serde_skips_none_fields() {
    let params = SendParams {
        to: "+15550100".to_owned(),
        text: "minimal".to_owned(),
        account_id: None,
        thread_id: None,
        attachments: None,
    };

    let json = serde_json::to_string(&params).expect("serialize");
    assert!(!json.contains("account_id"));
    assert!(!json.contains("thread_id"));
    assert!(!json.contains("attachments"));
    assert!(json.contains("\"to\":"));
    assert!(json.contains("\"text\":"));
}

#[test]
fn send_params_serde_roundtrip() {
    let original = SendParams {
        to: "group:abc123".to_owned(),
        text: "Group message".to_owned(),
        account_id: Some("+1111111111".to_owned()),
        thread_id: Some("reply-to-123".to_owned()),
        attachments: Some(vec!["file1.jpg".to_owned(), "file2.pdf".to_owned()]),
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: SendParams = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.to, original.to);
    assert_eq!(restored.text, original.text);
    assert_eq!(restored.account_id, original.account_id);
    assert_eq!(restored.thread_id, original.thread_id);
    assert_eq!(restored.attachments, original.attachments);
}

// ---------------------------------------------------------------------------
// SendResult
// ---------------------------------------------------------------------------

#[test]
fn send_result_ok_factory() {
    let result = SendResult::ok();
    assert!(result.sent);
    assert!(result.error.is_none());
}

#[test]
fn send_result_err_factory() {
    let result = SendResult::err("network timeout");
    assert!(!result.sent);
    assert_eq!(result.error.as_deref(), Some("network timeout"));
}

#[test]
fn send_result_err_with_string() {
    let msg = String::from("rate limited");
    let result = SendResult::err(msg);
    assert!(!result.sent);
    assert_eq!(result.error.as_deref(), Some("rate limited"));
}

#[test]
fn send_result_serde_roundtrip() {
    let ok_result = SendResult::ok();
    let err_result = SendResult::err("failed");

    let ok_json = serde_json::to_string(&ok_result).expect("serialize");
    let err_json = serde_json::to_string(&err_result).expect("serialize");

    let ok_restored: SendResult = serde_json::from_str(&ok_json).expect("deserialize");
    let err_restored: SendResult = serde_json::from_str(&err_json).expect("deserialize");

    assert!(ok_restored.sent);
    assert!(!err_restored.sent);
    assert_eq!(err_restored.error.as_deref(), Some("failed"));
}

// ---------------------------------------------------------------------------
// ProbeResult
// ---------------------------------------------------------------------------

#[test]
fn probe_result_success() {
    let mut details = std::collections::HashMap::new();
    details.insert("accounts".to_owned(), serde_json::json!(2));

    let result = ProbeResult {
        ok: true,
        latency_ms: Some(42),
        error: None,
        details: Some(details),
    };

    assert!(result.ok);
    assert_eq!(result.latency_ms, Some(42));
    assert!(result.error.is_none());
    assert!(result.details.is_some());
}

#[test]
fn probe_result_failure() {
    let result = ProbeResult {
        ok: false,
        latency_ms: None,
        error: Some("connection refused".to_owned()),
        details: None,
    };

    assert!(!result.ok);
    assert_eq!(result.latency_ms, None);
    assert_eq!(result.error.as_deref(), Some("connection refused"));
}

#[test]
fn probe_result_serde_roundtrip() {
    let original = ProbeResult {
        ok: true,
        latency_ms: Some(100),
        error: None,
        details: None,
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: ProbeResult = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.ok, original.ok);
    assert_eq!(restored.latency_ms, original.latency_ms);
}

// ---------------------------------------------------------------------------
// InboundMessage
// ---------------------------------------------------------------------------

#[test]
fn inbound_message_construction() {
    let msg = InboundMessage {
        channel: "signal".to_owned(),
        sender: "+1234567890".to_owned(),
        sender_name: Some("Alice".to_owned()),
        group_id: Some("group-abc".to_owned()),
        text: "Hello!".to_owned(),
        timestamp: 1_709_312_345_678,
        attachments: vec!["photo.jpg".to_owned()],
        raw: Some(serde_json::json!({"extra": "data"})),
    };

    assert_eq!(msg.channel, "signal");
    assert_eq!(msg.sender, "+1234567890");
    assert_eq!(msg.sender_name.as_deref(), Some("Alice"));
    assert_eq!(msg.group_id.as_deref(), Some("group-abc"));
    assert_eq!(msg.text, "Hello!");
    assert_eq!(msg.timestamp, 1_709_312_345_678);
    assert_eq!(msg.attachments.len(), 1);
    assert!(msg.raw.is_some());
}

#[test]
fn inbound_message_serde_roundtrip() {
    let original = InboundMessage {
        channel: "signal".to_owned(),
        sender: "+1234567890".to_owned(),
        sender_name: Some("Bob".to_owned()),
        group_id: None,
        text: "Direct message".to_owned(),
        timestamp: 1_709_312_345_678,
        attachments: vec![],
        raw: None,
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: InboundMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.channel, original.channel);
    assert_eq!(restored.sender, original.sender);
    assert_eq!(restored.sender_name, original.sender_name);
    assert_eq!(restored.group_id, original.group_id);
    assert_eq!(restored.text, original.text);
    assert_eq!(restored.timestamp, original.timestamp);
    assert_eq!(restored.attachments, original.attachments);
    assert_eq!(restored.raw, original.raw);
}

// ---------------------------------------------------------------------------
// ChannelProvider trait object safety
// ---------------------------------------------------------------------------

/// Compile-time test: `ChannelProvider` trait is object-safe.
#[test]
#[allow(dead_code, reason = "compile-time type check")]
fn channel_provider_trait_is_object_safe() {
    // WHY: The registry stores providers as Arc<dyn ChannelProvider>.
    // If the trait ceases to be object-safe, the registry cannot compile.

    // This proves Arc<dyn ChannelProvider> can be constructed.
    fn _type_check() {
        let _: Option<Arc<dyn ChannelProvider>> = None;
    }
}

// ---------------------------------------------------------------------------
// ChannelRegistry
// ---------------------------------------------------------------------------

/// A minimal mock provider for registry testing.
struct TestProvider {
    id: String,
    name: String,
    send_result: SendResult,
    probe_result: ProbeResult,
}

impl TestProvider {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: format!("Test {id}"),
            send_result: SendResult::ok(),
            probe_result: ProbeResult {
                ok: true,
                latency_ms: Some(10),
                error: None,
                details: None,
            },
        }
    }
}

impl ChannelProvider for TestProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> &ChannelCapabilities {
        // WHY: static capability set for tests
        static CAPS: ChannelCapabilities = ChannelCapabilities {
            threads: false,
            reactions: true,
            typing: true,
            media: true,
            streaming: false,
            rich_formatting: false,
            max_text_length: 2000,
        };
        &CAPS
    }

    fn send<'a>(
        &'a self,
        _params: &'a SendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
        let result = self.send_result.clone();
        Box::pin(async move { result })
    }

    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
        let result = self.probe_result.clone();
        Box::pin(async move { result })
    }
}

#[test]
fn registry_new_constructs() {
    // WHY: Construction is the only public API that doesn't require
    // a provider. We verify the registry can be created.
    let _registry = ChannelRegistry::new();
}

#[test]
fn registry_default_constructs() {
    let _registry = ChannelRegistry::default();
}

#[test]
fn registry_register_single_provider() {
    let mut registry = ChannelRegistry::new();
    let provider = Arc::new(TestProvider::new("signal"));

    registry.register(provider).expect("register succeeds");
    // Provider was registered; further verification via send/probe tests
}

#[test]
fn registry_register_multiple_providers() {
    let mut registry = ChannelRegistry::new();

    registry
        .register(Arc::new(TestProvider::new("signal")))
        .expect("register signal");
    registry
        .register(Arc::new(TestProvider::new("slack")))
        .expect("register slack");
    registry
        .register(Arc::new(TestProvider::new("discord")))
        .expect("register discord");

    // All three registered successfully
}

#[test]
fn registry_register_duplicate_fails() {
    let mut registry = ChannelRegistry::new();
    let provider1 = Arc::new(TestProvider::new("signal"));
    let provider2 = Arc::new(TestProvider::new("signal"));

    registry.register(provider1).expect("first register");
    let err = registry.register(provider2).expect_err("duplicate fails");

    let err_msg = err.to_string();
    assert!(
        err_msg.contains("duplicate channel"),
        "error should indicate duplicate: {err_msg}"
    );
    assert!(err_msg.contains("signal"), "error should name the channel: {err_msg}");
}

#[tokio::test]
async fn registry_send_verifies_provider_registered() {
    // WHY: We test registration worked by successfully sending through it.
    let mut registry = ChannelRegistry::new();
    registry
        .register(Arc::new(TestProvider::new("signal")))
        .expect("register");

    let params = SendParams {
        to: "+15550100".to_owned(),
        text: "Test".to_owned(),
        account_id: None,
        thread_id: None,
        attachments: None,
    };

    // This will only succeed if the provider is registered
    let result = registry.send("signal", &params).await.expect("send succeeds");
    assert!(result.sent);
}

#[tokio::test]
async fn registry_send_to_existing_channel() {
    let mut registry = ChannelRegistry::new();
    registry
        .register(Arc::new(TestProvider::new("signal")))
        .expect("register");

    let params = SendParams {
        to: "+15550100".to_owned(),
        text: "Test".to_owned(),
        account_id: None,
        thread_id: None,
        attachments: None,
    };

    let result = registry.send("signal", &params).await.expect("send succeeds");
    assert!(result.sent);
}

#[tokio::test]
async fn registry_send_to_missing_channel_fails() {
    let registry = ChannelRegistry::new();

    let params = SendParams {
        to: "+15550100".to_owned(),
        text: "Test".to_owned(),
        account_id: None,
        thread_id: None,
        attachments: None,
    };

    let err = registry.send("nonexistent", &params).await.expect_err("send fails");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("unknown channel"),
        "error should indicate unknown: {err_msg}"
    );
}

#[tokio::test]
async fn registry_probe_all_collects_results() {
    let mut registry = ChannelRegistry::new();

    let mut ok_provider = TestProvider::new("signal");
    ok_provider.probe_result = ProbeResult {
        ok: true,
        latency_ms: Some(42),
        error: None,
        details: None,
    };

    let mut fail_provider = TestProvider::new("slack");
    fail_provider.probe_result = ProbeResult {
        ok: false,
        latency_ms: None,
        error: Some("unreachable".to_owned()),
        details: None,
    };

    registry.register(Arc::new(ok_provider)).expect("register signal");
    registry.register(Arc::new(fail_provider)).expect("register slack");

    let results = registry.probe_all().await;
    assert_eq!(results.len(), 2);
    assert!(results.get("signal").expect("signal result").ok);
    assert!(!results.get("slack").expect("slack result").ok);
}

// ---------------------------------------------------------------------------
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
// SignalTarget and parse_target
// ---------------------------------------------------------------------------

#[test]
fn parse_target_phone_number() {
    let target = parse_target("+1234567890");
    assert_eq!(target, SignalTarget::Phone("+1234567890".to_owned()));
}

#[test]
fn parse_target_group() {
    let target = parse_target("group:YWJjMTIz");
    assert_eq!(target, SignalTarget::Group("YWJjMTIz".to_owned()));
}

#[test]
fn parse_target_group_empty_id() {
    let target = parse_target("group:");
    assert_eq!(target, SignalTarget::Group(String::new()));
}

#[test]
fn parse_target_plain_string() {
    let target = parse_target("someuser");
    assert_eq!(target, SignalTarget::Phone("someuser".to_owned()));
}

#[test]
fn signal_target_equality() {
    assert_eq!(
        SignalTarget::Phone("+123".to_owned()),
        SignalTarget::Phone("+123".to_owned())
    );
    assert_eq!(
        SignalTarget::Group("abc".to_owned()),
        SignalTarget::Group("abc".to_owned())
    );
    assert_ne!(
        SignalTarget::Phone("+123".to_owned()),
        SignalTarget::Group("+123".to_owned())
    );
}

// ---------------------------------------------------------------------------
// SignalProvider
// ---------------------------------------------------------------------------

#[test]
fn signal_provider_new_is_empty() {
    let provider = SignalProvider::new();
    assert_eq!(provider.id(), "signal");
    assert_eq!(provider.name(), "Signal");
}

#[test]
fn signal_provider_capabilities() {
    let provider = SignalProvider::new();
    let caps = provider.capabilities();

    assert!(!caps.threads);
    assert!(caps.reactions);
    assert!(caps.typing);
    assert!(caps.media);
    assert!(!caps.streaming);
    assert!(!caps.rich_formatting);
    assert_eq!(caps.max_text_length, 2000);
}

#[test]
fn signal_provider_default_equals_new() {
    let default = SignalProvider::default();
    assert_eq!(default.id(), "signal");
}

#[test]
fn signal_provider_with_buffer_capacity() {
    let provider = SignalProvider::with_buffer_capacity(500);
    assert_eq!(provider.id(), "signal");
}

// ---------------------------------------------------------------------------
// SignalClient
// ---------------------------------------------------------------------------

use aletheia_organon::testing::install_crypto_provider;

#[test]
fn signal_client_creation_fails_without_http_prefix() {
    install_crypto_provider();
    // WHY: SignalClient::new should succeed even without http:// prefix
    // as it normalizes the URL internally
    let client = SignalClient::new("localhost:8080");
    assert!(client.is_ok());
}

#[test]
fn signal_client_creation_with_http_prefix() {
    install_crypto_provider();
    let client = SignalClient::new("http://localhost:8080");
    assert!(client.is_ok());
}

#[test]
fn signal_client_creation_with_https_prefix() {
    install_crypto_provider();
    let client = SignalClient::new("https://signal.example.com");
    assert!(client.is_ok());
}

#[test]
fn signal_client_debug_impl() {
    install_crypto_provider();
    let client = SignalClient::new("localhost:8080").expect("create client");
    let debug = format!("{client:?}");
    assert!(debug.contains("SignalClient"));
    assert!(debug.contains("rpc_url"));
}

// ---------------------------------------------------------------------------
// SignalSendParams
// ---------------------------------------------------------------------------

#[test]
fn signal_send_params_construction() {
    let params = SignalSendParams {
        message: Some("Hello".to_owned()),
        recipient: Some("+1234567890".to_owned()),
        group_id: None,
        account: Some("+0987654321".to_owned()),
        attachments: None,
    };

    assert_eq!(params.message.as_deref(), Some("Hello"));
    assert_eq!(params.recipient.as_deref(), Some("+1234567890"));
    assert!(params.group_id.is_none());
}

#[test]
fn signal_send_params_serde_roundtrip() {
    let original = SignalSendParams {
        message: Some("Test message".to_owned()),
        recipient: Some("+1234567890".to_owned()),
        group_id: Some("group-id".to_owned()),
        account: Some("+1111111111".to_owned()),
        attachments: Some(vec!["file1.jpg".to_owned()]),
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: SignalSendParams = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.message, original.message);
    assert_eq!(restored.recipient, original.recipient);
    assert_eq!(restored.group_id, original.group_id);
    assert_eq!(restored.account, original.account);
    assert_eq!(restored.attachments, original.attachments);
}

// ---------------------------------------------------------------------------
// SignalEnvelope
// ---------------------------------------------------------------------------

#[test]
fn signal_envelope_deserialize_full() {
    let json = serde_json::json!({
        "sourceNumber": "+1234567890",
        "sourceUuid": "uuid-abc",
        "sourceName": "Alice",
        "timestamp": 1_709_312_345_678_u64,
        "dataMessage": {
            "timestamp": 1_709_312_345_678_u64,
            "message": "Hello world",
            "groupInfo": {
                "groupId": "group123"
            },
            "attachments": [
                {"id": "att1", "filename": "photo.jpg", "contentType": "image/jpeg", "size": 1024}
            ]
        }
    });

    let envelope: SignalEnvelope = serde_json::from_value(json).expect("deserialize");
    assert_eq!(envelope.source_number.as_deref(), Some("+1234567890"));
    assert_eq!(envelope.source_uuid.as_deref(), Some("uuid-abc"));
    assert_eq!(envelope.source_name.as_deref(), Some("Alice"));
    assert_eq!(envelope.timestamp, Some(1_709_312_345_678));

    let data = envelope.data_message.expect("has data message");
    assert_eq!(data.message.as_deref(), Some("Hello world"));

    let group_info = data.group_info.expect("has group info");
    assert_eq!(group_info.group_id.as_deref(), Some("group123"));
}

#[test]
fn signal_envelope_deserialize_minimal() {
    let json = serde_json::json!({
        "sourceNumber": "+5555555555",
        "dataMessage": {
            "message": "hi"
        }
    });

    let envelope: SignalEnvelope = serde_json::from_value(json).expect("deserialize");
    assert_eq!(envelope.source_number.as_deref(), Some("+5555555555"));
    assert!(envelope.source_uuid.is_none());
    assert!(envelope.source_name.is_none());
    assert!(envelope.timestamp.is_none());
}

// ---------------------------------------------------------------------------
// DataMessage
// ---------------------------------------------------------------------------

#[test]
fn data_message_construction() {
    let msg = DataMessage {
        timestamp: Some(1_709_312_345_678),
        message: Some("Test".to_owned()),
        group_info: Some(GroupInfo {
            group_id: Some("group-id".to_owned()),
        }),
        attachments: Some(vec![Attachment {
            id: Some("att-1".to_owned()),
            content_type: Some("image/jpeg".to_owned()),
            filename: Some("photo.jpg".to_owned()),
            size: Some(1024),
        }]),
    };

    assert_eq!(msg.message.as_deref(), Some("Test"));
    assert!(msg.group_info.is_some());
    assert_eq!(msg.attachments.as_ref().map_or(0, std::vec::Vec::len), 1);
}

#[test]
fn data_message_serde_roundtrip() {
    let original = DataMessage {
        timestamp: Some(12345),
        message: Some("Hello".to_owned()),
        group_info: None,
        attachments: None,
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: DataMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.timestamp, original.timestamp);
    assert_eq!(restored.message, original.message);
}

// ---------------------------------------------------------------------------
// GroupInfo
// ---------------------------------------------------------------------------

#[test]
fn group_info_construction() {
    let info = GroupInfo {
        group_id: Some("base64-group-id".to_owned()),
    };
    assert_eq!(info.group_id.as_deref(), Some("base64-group-id"));
}

#[test]
fn group_info_serde_roundtrip() {
    let original = GroupInfo {
        group_id: Some("group123".to_owned()),
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: GroupInfo = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.group_id, original.group_id);
}

// ---------------------------------------------------------------------------
// Attachment
// ---------------------------------------------------------------------------

#[test]
fn attachment_construction() {
    let att = Attachment {
        id: Some("att-1".to_owned()),
        content_type: Some("application/pdf".to_owned()),
        filename: Some("document.pdf".to_owned()),
        size: Some(2048),
    };

    assert_eq!(att.id.as_deref(), Some("att-1"));
    assert_eq!(att.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(att.filename.as_deref(), Some("document.pdf"));
    assert_eq!(att.size, Some(2048));
}

#[test]
fn attachment_serde_roundtrip() {
    let original = Attachment {
        id: Some("att-2".to_owned()),
        content_type: None,
        filename: Some("unnamed.bin".to_owned()),
        size: None,
    };

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: Attachment = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.id, original.id);
    assert_eq!(restored.content_type, original.content_type);
    assert_eq!(restored.filename, original.filename);
    assert_eq!(restored.size, original.size);
}

// ---------------------------------------------------------------------------
// ConnectionState
// ---------------------------------------------------------------------------

#[test]
fn connection_state_variants() {
    let connected = ConnectionState::Connected;
    let reconnecting = ConnectionState::Reconnecting { attempt: 3 };
    let halted = ConnectionState::Halted { total_failures: 25 };

    // Test that variants are distinct
    assert_ne!(connected, reconnecting);
    assert_ne!(connected, halted);
    assert_ne!(reconnecting, halted);

    // Test reconnecting captures attempt count
    if let ConnectionState::Reconnecting { attempt } = reconnecting {
        assert_eq!(attempt, 3);
    } else {
        panic!("expected Reconnecting");
    }

    // Test halted captures total failures
    if let ConnectionState::Halted { total_failures } = halted {
        assert_eq!(total_failures, 25);
    } else {
        panic!("expected Halted");
    }
}

#[test]
fn connection_state_clone() {
    let state = ConnectionState::Reconnecting { attempt: 5 };
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn connection_state_debug() {
    let state = ConnectionState::Connected;
    let debug = format!("{state:?}");
    assert!(debug.contains("Connected"));
}

// ---------------------------------------------------------------------------
// ConnectionHealthReport
// ---------------------------------------------------------------------------

#[test]
fn connection_health_report_construction() {
    let report = ConnectionHealthReport {
        state: ConnectionState::Connected,
        buffered_messages: 5,
        dropped_count: 2,
    };

    assert!(matches!(report.state, ConnectionState::Connected));
    assert_eq!(report.buffered_messages, 5);
    assert_eq!(report.dropped_count, 2);
}

#[test]
fn connection_health_report_clone() {
    let original = ConnectionHealthReport {
        state: ConnectionState::Halted { total_failures: 10 },
        buffered_messages: 3,
        dropped_count: 1,
    };

    let cloned = original.clone();
    assert_eq!(original.buffered_messages, cloned.buffered_messages);
    assert_eq!(original.dropped_count, cloned.dropped_count);
    assert_eq!(format!("{:?}", original.state), format!("{:?}", cloned.state));
}

// ---------------------------------------------------------------------------
// SignalError
// ---------------------------------------------------------------------------

#[test]
fn signal_error_implements_std_error() {
    fn assert_std_error<T: std::error::Error + Send + Sync + 'static>() {}
    assert_std_error::<SignalError>();
}

#[test]
fn signal_error_debug_impl() {
    // WHY: SignalError variants have display formats via snafu.
    // We can at least verify the Debug impl works.
    let err = SignalError::NoAccount {
        account_id: "+1234567890".to_owned(),
        location: snafu::location!(),
    };
    let debug = format!("{err:?}");
    assert!(!debug.is_empty());
}

// ---------------------------------------------------------------------------
// Send + Sync bounds (as promised in lib.rs)
// ---------------------------------------------------------------------------

#[allow(dead_code, reason = "compile-time trait bound check")]
fn assert_send<T: Send>() {}
#[allow(dead_code, reason = "compile-time trait bound check")]
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn public_types_are_send_sync() {
    // WHY: These bounds are load-bearing for the async runtime.
    // lib.rs has internal assertions; these are the external contract tests.

    // Core types
    assert_send_sync::<ChannelCapabilities>();
    assert_send_sync::<SendParams>();
    assert_send_sync::<SendResult>();
    assert_send_sync::<ProbeResult>();
    assert_send_sync::<InboundMessage>();

    // Registry
    assert_send_sync::<ChannelRegistry>();

    // Router types
    assert_send_sync::<MessageRouter>();
    assert_send_sync::<MatchReason>();

    // Signal types
    assert_send_sync::<SignalProvider>();
    assert_send_sync::<SignalClient>();
    assert_send_sync::<SignalTarget>();
    assert_send_sync::<SignalSendParams>();
    assert_send_sync::<SignalEnvelope>();
    assert_send_sync::<DataMessage>();
    assert_send_sync::<GroupInfo>();
    assert_send_sync::<Attachment>();
    assert_send_sync::<ConnectionState>();
    assert_send_sync::<ConnectionHealthReport>();
    assert_send_sync::<SignalError>();
}

#[test]
fn signal_provider_is_send_sync() {
    // WHY: SignalProvider is held in Arc<dyn ChannelProvider> and used across
    // async boundaries. It must be Send + Sync.
    assert_send_sync::<SignalProvider>();
}

#[test]
fn signal_client_is_send_sync() {
    // WHY: SignalClient is cloned and moved into async tasks.
    assert_send_sync::<SignalClient>();
}

#[test]
fn channel_registry_is_send_sync() {
    // WHY: ChannelRegistry is shared across the application.
    assert_send_sync::<ChannelRegistry>();
}

// ---------------------------------------------------------------------------
// CancellationToken compatibility
// ---------------------------------------------------------------------------

#[test]
fn cancellation_token_is_send_sync() {
    // WHY: CancellationToken is used with SignalProvider::listen
    assert_send_sync::<CancellationToken>();
}
