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
#[derive(Clone, Serialize, Deserialize)]
pub struct SendParams {
    /// Target identifier (channel-specific: phone number, group ID, etc.)
    pub to: String,
    /// Message text (markdown).
    pub text: String,
    /// Stable logical account label within the channel (for example,
    /// `"primary"`). Providers resolve this label to their private wire
    /// identity; callers never put a phone number or access identity here.
    /// `None` selects the provider's configured logical default.
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

impl std::fmt::Debug for SendParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendParams")
            .field(
                "to",
                &crate::redact::opaque_identifier("send-target", &self.to),
            )
            .field("text", &format_args!("<{} bytes>", self.text.len()))
            .field(
                "account_id",
                &self
                    .account_id
                    .as_deref()
                    .map(|value| crate::redact::opaque_identifier("logical-account", value)),
            )
            .field("sender_id", &self.sender_id.as_ref().map(|_| "<present>"))
            .field(
                "idempotency_key",
                &self.idempotency_key.as_ref().map(|_| "<present>"),
            )
            .field("thread_id", &self.thread_id.as_ref().map(|_| "<present>"))
            .field(
                "attachments",
                &self.attachments.as_ref().map(std::vec::Vec::len),
            )
            .finish()
    }
}

/// Result of a send operation.
#[derive(Clone, Serialize, Deserialize)]
pub struct SendResult {
    /// Whether the message was successfully delivered to the channel.
    pub sent: bool,
    /// Error description if the send failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl std::fmt::Debug for SendResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendResult")
            .field("sent", &self.sent)
            .field("error", &self.error.as_ref().map(|_| "<present>"))
            .finish()
    }
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
#[derive(Clone, Serialize, Deserialize)]
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
    /// Identifies which configured logical account label accepted the message
    /// (for example, `"primary"`), so
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
    /// Always returns an opaque SHA-256 handle. Provider IDs are scoped by channel and
    /// logical receiving account before hashing; the fallback also commits to
    /// sender, group, timestamp, text, and attachments. The returned handle
    /// contains no raw provider ID.
    #[must_use]
    pub fn dedupe_key(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"aletheia.agora.inbound-message.v1\0");
        hash_part(&mut hasher, &self.channel);
        hash_optional_part(&mut hasher, self.account_id.as_deref());
        if let Some(id) = self.message_id.as_deref().filter(|id| !id.is_empty()) {
            hash_part(&mut hasher, "provider-id");
            match self.group_id.as_deref() {
                Some(group) => {
                    hash_part(&mut hasher, "group");
                    hash_part(&mut hasher, group);
                }
                None => {
                    hash_part(&mut hasher, "dm");
                    hash_part(&mut hasher, &self.sender);
                }
            }
            hash_part(&mut hasher, id);
        } else {
            hash_part(&mut hasher, "content-fallback");
            hash_part(&mut hasher, &self.sender);
            hash_optional_part(&mut hasher, self.group_id.as_deref());
            hash_part(&mut hasher, &self.timestamp.to_string());
            hash_part(&mut hasher, &self.text);
            hash_part(&mut hasher, &self.attachments.len().to_string());
            for attachment in &self.attachments {
                hash_part(&mut hasher, attachment);
            }
        }
        format!("sha256:{}", hex_lower(&hasher.finalize()))
    }
}

/// Channel identities are personal identifiers: the Debug representation
/// redacts sender/group/account via [`crate::redact::identifier`], never
/// prints the message text or attachment references, and reports a captured
/// raw payload only as present-or-absent.
impl std::fmt::Debug for InboundMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboundMessage")
            .field("channel", &self.channel)
            .field("sender", &crate::redact::identifier(&self.sender))
            .field(
                "sender_name",
                &self.sender_name.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "group_id",
                &self.group_id.as_deref().map(crate::redact::identifier),
            )
            .field(
                "account_id",
                &self.account_id.as_deref().map(crate::redact::identifier),
            )
            .field("message_id", &self.message_id.as_ref().map(|_| "<present>"))
            .field("text", &format_args!("<{} bytes>", self.text.len()))
            .field("timestamp", &self.timestamp)
            .field("attachments", &self.attachments.len())
            .field("raw", &self.raw.as_ref().map(|_| "<captured>"))
            .finish()
    }
}

fn hash_part(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}

fn hash_optional_part(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hash_part(hasher, "some");
            hash_part(hasher, value);
        }
        None => hash_part(hasher, "none"),
    }
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    // NOTE: call sites pass `byte >> 4` / `byte & 0x0f` (always 0..=15).
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        _ => char::from(b'a' + (nibble - 10)),
    }
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
        let provider_key = msg.dedupe_key();
        assert!(provider_key.starts_with("sha256:"));
        assert!(!provider_key.contains("$event1"));

        let mut other_account = msg.clone();
        other_account.account_id = Some("secondary".to_owned());
        assert_ne!(provider_key, other_account.dedupe_key());

        let mut other_room = msg.clone();
        other_room.group_id = Some("!other:example.org".to_owned());
        assert_ne!(provider_key, other_room.dedupe_key());

        let mut direct_message = msg.clone();
        direct_message.group_id = None;
        assert_ne!(provider_key, direct_message.dedupe_key());

        msg.message_id = None;
        let hashed = msg.dedupe_key();
        assert!(
            hashed.starts_with("sha256:"),
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

        let mut with_attachment = a.clone();
        with_attachment.attachments.push("first".to_owned());
        let mut other_attachment = a.clone();
        other_attachment.attachments.push("second".to_owned());
        assert_ne!(a.dedupe_key(), with_attachment.dedupe_key());
        assert_ne!(with_attachment.dedupe_key(), other_attachment.dedupe_key());
    }

    #[test]
    fn blank_provider_id_uses_content_fallback() {
        let base = InboundMessage {
            channel: "matrix".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_name: None,
            group_id: Some("!room:example.org".to_owned()),
            account_id: Some("primary".to_owned()),
            message_id: None,
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            raw: None,
        };
        let mut blank = base.clone();
        blank.message_id = Some(String::new());
        assert_eq!(base.dedupe_key(), blank.dedupe_key());
    }

    #[test]
    fn optional_identity_presence_is_hash_significant() {
        let mut absent = InboundMessage {
            channel: "signal".to_owned(),
            sender: "sender".to_owned(),
            sender_name: None,
            group_id: None,
            account_id: None,
            message_id: None,
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            raw: None,
        };
        let mut present_empty = absent.clone();
        present_empty.account_id = Some(String::new());
        assert_ne!(absent.dedupe_key(), present_empty.dedupe_key());

        absent.account_id = Some("primary".to_owned());
        present_empty.account_id = Some("primary".to_owned());
        present_empty.group_id = Some(String::new());
        assert_ne!(absent.dedupe_key(), present_empty.dedupe_key());
    }

    #[test]
    fn send_debug_redacts_payload_and_provider_identities() {
        let params = SendParams {
            to: "+15550100".to_owned(),
            text: "private body".to_owned(),
            account_id: Some("primary-private".to_owned()),
            sender_id: Some("syn".to_owned()),
            idempotency_key: Some("reply-secret".to_owned()),
            thread_id: Some("thread-secret".to_owned()),
            attachments: Some(vec!["private.jpg".to_owned()]),
        };
        let debug = format!("{params:?}");
        for secret in [
            "+15550100",
            "private body",
            "primary-private",
            "reply-secret",
            "thread-secret",
            "private.jpg",
        ] {
            assert!(!debug.contains(secret), "Debug leaked {secret}: {debug}");
        }
    }

    #[test]
    fn debug_redacts_channel_identities_and_payload() {
        let msg = InboundMessage {
            channel: "signal".to_owned(),
            sender: "+1234567890".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: Some("group-abc-secret".to_owned()),
            account_id: Some("+1098765432".to_owned()),
            message_id: Some("evt-1".to_owned()),
            text: "a private message body".to_owned(),
            timestamp: 100,
            attachments: vec!["s3://bucket/private.jpg".to_owned()],
            raw: Some(serde_json::json!({"provider": "payload"})),
        };
        let debug = format!("{msg:?}");
        assert!(debug.contains("...7890"), "{debug}");
        assert!(debug.contains("...cret"), "{debug}");
        assert!(debug.contains("...5432"), "{debug}");
        for leaked in [
            "+1234567890",
            "Alice",
            "group-abc",
            "+1098765432",
            "private message body",
            "s3://bucket/private.jpg",
            "provider",
        ] {
            assert!(!debug.contains(leaked), "Debug leaked {leaked}: {debug}");
        }
    }
}
