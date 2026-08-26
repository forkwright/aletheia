//! Core types for the channel abstraction layer.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// What a channel supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "capability flags are inherently boolean"
)]
pub struct ChannelCapabilities {
    /// Whether the channel supports threaded replies.
    pub threads: bool,
    /// Whether message reactions (emoji, etc.) are supported.
    pub reactions: bool,
    /// Whether typing indicators can be sent.
    pub typing: bool,
    /// Whether file/media attachments are supported.
    pub media: bool,
    /// Whether real-time streaming delivery is supported.
    pub streaming: bool,
    /// Whether markdown or other rich text formatting is supported.
    pub rich_formatting: bool,
    /// Maximum text length in a single message (channel-imposed limit).
    pub max_text_length: usize,
}

/// Parameters for sending a message through a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendParams {
    /// Target identifier (channel-specific: phone number, group ID, etc.)
    pub to: String,
    /// Message text (markdown).
    pub text: String,
    /// Account ID within the channel (for multi-account setups): selects
    /// WHICH provider account/identity a message is sent FROM (e.g. which
    /// registered Signal phone number, which Matrix account). `None` falls
    /// back to the provider's configured default account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Identity of the sending agent (`nous_id`), used ONLY for outbound
    /// attribution/policy (`OutboundMessagePolicy`, checked in
    /// `ChannelRegistry::send`) and audit records.
    ///
    /// SECURITY(#4788): distinct from `account_id` on purpose -- an
    /// earlier version of this field carried the sending agent's identity
    /// in `account_id` itself, which broke provider account routing:
    /// `SignalProvider`/`MatrixProvider` resolve `account_id` against
    /// their configured account keyspace (phone numbers / Matrix account
    /// IDs), so a `nous_id` like `"syn"` landing there fails lookup
    /// (`resolve_client`/`resolve_account` return `None`) instead of
    /// falling through to the provider's default account. `None` here
    /// means the send is unattributed (e.g. `dispatch::send_reply`
    /// completing an inbound conversation), not agent-initiated, and
    /// bypasses the outbound-recipient policy -- see
    /// `ChannelRegistry::outbound_policy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,
    /// Provider-level idempotency key for this send, when the channel
    /// supports one (Matrix transaction IDs). Re-sending with the same key
    /// must not produce a duplicate visible message. `None` lets the
    /// provider generate a fresh per-attempt key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Thread/reply context identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// File attachment paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
}

/// Result of a send operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResult {
    /// Whether the message was successfully delivered to the channel.
    pub sent: bool,
    /// Error description if the send failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SendResult {
    /// Successful send.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            sent: true,
            error: None,
        }
    }

    /// Failed send with an error description.
    #[must_use]
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            sent: false,
            error: Some(message.into()),
        }
    }
}

/// Health probe result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    /// Whether the channel is reachable.
    pub ok: bool,
    /// Round-trip latency in milliseconds, if measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Error description if the probe failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Provider-specific health details (e.g., per-account status).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, serde_json::Value>>,
}

/// A normalized inbound message received from any channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    /// Channel this message came from (e.g., "signal").
    pub channel: String,
    /// Sender identifier (phone number, user ID, etc.).
    pub sender: String,
    /// Display name if known.
    pub sender_name: Option<String>,
    /// Group/conversation identifier (None for DM).
    pub group_id: Option<String>,
    /// Provider account that RECEIVED this message (multi-account routing).
    ///
    /// Identifies which configured account identity accepted the message
    /// (e.g. which registered Signal number, which Matrix account), so
    /// identical senders/rooms on different accounts stay distinct and
    /// replies leave from the receiving account. `None` when the provider
    /// cannot attribute an account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Stable provider message ID (e.g. Matrix `event_id`), when the
    /// provider exposes one. Drives dedupe and reply idempotency; `None`
    /// for providers without message IDs (signal-cli) — callers fall back
    /// to [`Self::dedupe_key`]'s content hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Message text content.
    pub text: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Attachment file paths or identifiers.
    pub attachments: Vec<String>,
    /// Raw channel-specific payload for extensions.
    pub raw: Option<serde_json::Value>,
}

impl InboundMessage {
    /// Stable identity key for dedupe and reply idempotency.
    ///
    /// The provider message ID when one exists; otherwise a SHA-256 content
    /// hash over the routing identity, receiving account, timestamp, and
    /// text, so a redelivered message hashes to the same key while
    /// distinct messages that happen to share a timestamp do not collide.
    #[must_use]
    pub fn dedupe_key(&self) -> String {
        if let Some(id) = &self.message_id {
            return format!("{}:id:{id}", self.channel);
        }
        let mut hasher = Sha256::new();
        hash_part(&mut hasher, &self.channel);
        hash_part(&mut hasher, self.account_id.as_deref().unwrap_or(""));
        hash_part(&mut hasher, &self.sender);
        hash_part(&mut hasher, self.group_id.as_deref().unwrap_or(""));
        hash_part(&mut hasher, &self.timestamp.to_string());
        hash_part(&mut hasher, &self.text);
        format!("{}:h:{}", self.channel, hex_lower(&hasher.finalize()))
    }
}

fn hash_part(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        out.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        out.push(char::from(
            b"0123456789abcdef"[usize::from(byte & 0x0f)],
        ));
    }
    out
}

/// The contract every channel provider must implement.
///
/// Object-safe via `Pin<Box<dyn Future>>` (matches `ToolExecutor` in organon).
/// Implementations are stored as `Arc<dyn ChannelProvider>` in the registry.
pub trait ChannelProvider: Send + Sync {
    /// Unique channel identifier (e.g., `"signal"`, `"slack"`).
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn name(&self) -> &str;

    /// What this channel supports.
    fn capabilities(&self) -> &ChannelCapabilities;

    /// Send a message outbound through this channel.
    fn send<'a>(
        &'a self,
        params: &'a SendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>>;

    /// Start listening for inbound messages from this channel.
    fn listen(
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>);

    /// Health probe for this channel.
    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>>;
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn send_params_serde_roundtrip() {
        let params = SendParams {
            to: "+15550100".to_owned(),
            text: "hello world".to_owned(),
            account_id: Some("acct1".to_owned()),
            sender_id: Some("syn".to_owned()),
            idempotency_key: Some("reply:abc".to_owned()),
            thread_id: None,
            attachments: Some(vec!["photo.jpg".to_owned()]),
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: SendParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.to, params.to);
        assert_eq!(back.text, params.text);
        assert_eq!(back.account_id, params.account_id);
        assert_eq!(back.sender_id, params.sender_id);
        assert_eq!(back.idempotency_key, params.idempotency_key);
        assert_eq!(back.thread_id, params.thread_id);
        assert_eq!(back.attachments, params.attachments);
    }

    #[test]
    fn send_params_skips_none_fields_in_json() {
        let params = SendParams {
            to: "+15550100".to_owned(),
            text: "hello".to_owned(),
            account_id: None,
            sender_id: None,
            idempotency_key: None,
            thread_id: None,
            attachments: None,
        };
        let json = serde_json::to_string(&params).expect("serialize");
        assert!(!json.contains("account_id"));
        assert!(!json.contains("sender_id"));
        assert!(!json.contains("idempotency_key"));
        assert!(!json.contains("thread_id"));
        assert!(!json.contains("attachments"));
    }

    #[test]
    fn inbound_message_serde_roundtrip() {
        let msg = InboundMessage {
            channel: "signal".to_owned(),
            sender: "+1234567890".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: Some("grp123".to_owned()),
            account_id: Some("acct1".to_owned()),
            message_id: Some("evt-1".to_owned()),
            text: "hello world".to_owned(),
            timestamp: 1_709_312_345_678,
            attachments: vec!["photo.jpg".to_owned()],
            raw: Some(serde_json::json!({"extra": "data"})),
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: InboundMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.channel, msg.channel);
        assert_eq!(back.sender, msg.sender);
        assert_eq!(back.sender_name, msg.sender_name);
        assert_eq!(back.group_id, msg.group_id);
        assert_eq!(back.account_id, msg.account_id);
        assert_eq!(back.message_id, msg.message_id);
        assert_eq!(back.text, msg.text);
        assert_eq!(back.timestamp, msg.timestamp);
        assert_eq!(back.attachments, msg.attachments);
        assert_eq!(back.raw, msg.raw);
    }

    #[test]
    fn inbound_message_account_id_defaults_to_none() {
        let json = serde_json::json!({
            "channel": "signal",
            "sender": "+1234567890",
            "sender_name": null,
            "group_id": null,
            "text": "hello",
            "timestamp": 100,
            "attachments": [],
            "raw": null,
        });
        let msg: InboundMessage = serde_json::from_value(json).expect("deserialize");
        assert_eq!(msg.account_id, None);
        assert_eq!(msg.message_id, None);
        let serialized = serde_json::to_string(&msg).expect("serialize");
        assert!(
            !serialized.contains("account_id"),
            "None account_id must be omitted: {serialized}"
        );
        assert!(
            !serialized.contains("message_id"),
            "None message_id must be omitted: {serialized}"
        );
    }

    #[test]
    fn dedupe_key_prefers_provider_message_id() {
        let mut msg = InboundMessage {
            channel: "matrix".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_name: None,
            group_id: Some("!room:example.org".to_owned()),
            account_id: Some("primary".to_owned()),
            message_id: Some("$event1".to_owned()),
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            raw: None,
        };
        assert_eq!(msg.dedupe_key(), "matrix:id:$event1");

        msg.message_id = None;
        let hashed = msg.dedupe_key();
        assert!(
            hashed.starts_with("matrix:h:"),
            "content-hash fallback: {hashed}"
        );
        assert!(
            !hashed.contains("@alice:example.org"),
            "hash must not carry the raw sender: {hashed}"
        );
    }

    #[test]
    fn dedupe_key_content_hash_is_stable_and_distinguishing() {
        let build = |text: &str, timestamp: u64| InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: None,
            group_id: None,
            account_id: None,
            message_id: None,
            text: text.to_owned(),
            timestamp,
            attachments: vec![],
            raw: None,
        };
        let a = build("hello", 100);
        let a_again = build("hello", 100);
        let other_text = build("goodbye", 100);
        let other_ts = build("hello", 101);

        assert_eq!(a.dedupe_key(), a_again.dedupe_key());
        assert_ne!(a.dedupe_key(), other_text.dedupe_key());
        assert_ne!(a.dedupe_key(), other_ts.dedupe_key());
    }
}
