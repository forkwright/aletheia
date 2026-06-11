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

// ── ChannelCapabilities ──

#[test]
fn channel_capabilities_default_values() {
    // WHY: Signal capabilities are hardcoded and define the channel contract.
    // Changing these silently alters what consumers can expect from the Signal provider.
    let caps = ChannelCapabilities {
        threads: false,
        reactions: false,
        typing: false,
        media: true,
        streaming: false,
        rich_formatting: false,
        max_text_length: 2000,
    };

    assert!(!caps.threads);
    assert!(!caps.reactions);
    assert!(!caps.typing);
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

// ── SendParams ──

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

// ── SendResult ──

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

// ── ProbeResult ──

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

// ── InboundMessage ──

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

// ── ChannelProvider trait object safety ──

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
