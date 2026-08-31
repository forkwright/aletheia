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

use koina::redact::redact_channel_id;

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
///
/// WHY manual `Debug` (#5198): the derived `Debug` printed `sender` (a
/// phone number or Matrix ID), `sender_name` (a real display name), and
/// the full `raw` provider payload verbatim into any log or trace that
/// captured this type -- the exact leak this issue centralizes a fix for.
/// `Serialize`/`Deserialize` stay derived: those drive the store's own
/// wire format (round-tripped byte-for-byte, e.g. in session persistence),
/// not a log or diagnostic sink; callers that DO serialize an
/// `InboundMessage` toward a log or command record must redact it
/// explicitly at that boundary (see `dispatch::command_origin_record`).
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
    /// Stable provider message ID (e.g. Matrix `event_id`), when the
    /// provider exposes one. Drives inbound dedupe; `None` for providers
    /// without message IDs (signal-cli) -- [`Self::dedupe_key`] falls back
    /// to a content hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Message text content.
    pub text: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Attachment file paths or identifiers.
    pub attachments: Vec<String>,
    /// Raw channel-specific payload for extensions.
    ///
    /// Opt-in via `taxis::config::RawPayloadPolicy::capture` (default
    /// `false`), bounded by `RawPayloadPolicy::max_bytes`, and redacted of
    /// PII-shaped fields (phone numbers, Matrix/user IDs, names,
    /// attachment URLs) before it is ever attached here -- see
    /// [`capture_raw_payload`].
    pub raw: Option<serde_json::Value>,
}

impl std::fmt::Debug for InboundMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboundMessage")
            .field("channel", &self.channel)
            .field("sender", &redact_channel_id(&self.sender))
            .field(
                "sender_name",
                &self.sender_name.as_ref().map(|_| "[REDACTED]"),
            )
            .field("group_id", &self.group_id.as_deref().map(redact_channel_id))
            .field(
                "message_id",
                &self.message_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("text_len", &self.text.len())
            .field("timestamp", &self.timestamp)
            .field("attachment_count", &self.attachments.len())
            .field(
                "raw_bytes",
                &self
                    .raw
                    .as_ref()
                    .and_then(|v| serde_json::to_vec(v).ok())
                    .map(|bytes| bytes.len()),
            )
            .finish()
    }
}

impl InboundMessage {
    /// Stable identity key for inbound dedupe.
    ///
    /// Always returns an opaque SHA-256 handle. A provider message ID is
    /// scoped by channel and conversation before hashing; the fallback for
    /// providers without message IDs commits to sender, group, timestamp,
    /// text, and attachments instead. The returned handle contains no raw
    /// provider identifier, so it is safe to log or export as-is.
    #[must_use]
    pub fn dedupe_key(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"aletheia.agora.inbound-message.v1\0");
        hash_part(&mut hasher, &self.channel);
        if let Some(id) = self.message_id.as_deref().filter(|id| !id.is_empty()) {
            hash_part(&mut hasher, "provider-id");
            if let Some(group) = self.group_id.as_deref() {
                hash_part(&mut hasher, "group");
                hash_part(&mut hasher, group);
            } else {
                hash_part(&mut hasher, "dm");
                hash_part(&mut hasher, &self.sender);
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

/// Feed one length-delimited field into a dedupe-key hash, so adjacent
/// fields can never collide by concatenation.
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

fn hex_lower(bytes: &[u8]) -> String {
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

/// Compute an [`InboundMessage::raw`] value for a captured provider
/// payload under the opt-in, bounded, redacted raw-payload policy
/// (#5198).
///
/// Returns `None` outright when `policy.capture` is `false` (the
/// default). When capture is enabled, `value` is redacted via
/// [`koina::redact::redact_json_identifiers`] and the result is kept only
/// if its encoded size is within `policy.max_bytes`; an oversized payload
/// is dropped rather than truncated into invalid JSON.
#[must_use]
pub fn capture_raw_payload<T: Serialize>(
    policy: &taxis::config::RawPayloadPolicy,
    value: &T,
) -> Option<serde_json::Value> {
    if !policy.capture {
        return None;
    }
    // WHY: raw payload capture is best-effort diagnostic data; a
    // serialization failure aborts capture for this message via `?`
    // rather than being silently discarded.
    let raw = serde_json::to_value(value).ok()?;
    koina::redact::bounded_redacted_payload(&raw, policy.max_bytes)
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
            thread_id: None,
            attachments: Some(vec!["photo.jpg".to_owned()]),
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: SendParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.to, params.to);
        assert_eq!(back.text, params.text);
        assert_eq!(back.account_id, params.account_id);
        assert_eq!(back.sender_id, params.sender_id);
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
            thread_id: None,
            attachments: None,
        };
        let json = serde_json::to_string(&params).expect("serialize");
        assert!(!json.contains("account_id"));
        assert!(!json.contains("sender_id"));
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
            message_id: Some("msg-1".to_owned()),
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
        assert_eq!(back.message_id, msg.message_id);
        assert_eq!(back.text, msg.text);
        assert_eq!(back.timestamp, msg.timestamp);
        assert_eq!(back.attachments, msg.attachments);
        assert_eq!(back.raw, msg.raw);
    }

    #[test]
    fn inbound_message_debug_redacts_identity_and_raw() {
        let msg = InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: Some("group-abc123".to_owned()),
            message_id: Some("provider-msg-abc123".to_owned()),
            text: "my ssn is 123-45-6789".to_owned(), // pii-allow: synthetic SSN fixture asserting redaction, not a real value
            timestamp: 1_709_312_345_678,
            attachments: vec!["photo.jpg".to_owned()],
            raw: Some(serde_json::json!({"sourceNumber": "+15550100"})),
        };

        let debug = format!("{msg:?}");
        assert!(!debug.contains("+15550100"), "{debug}");
        assert!(!debug.contains("Alice"), "{debug}");
        assert!(!debug.contains("group-abc123"), "{debug}");
        assert!(!debug.contains("provider-msg-abc123"), "{debug}");
        assert!(!debug.contains("my ssn is"), "{debug}");
        assert!(!debug.contains("photo.jpg"), "{debug}");
        assert!(debug.contains("channel"), "{debug}");
    }

    fn dedupe_fixture() -> InboundMessage {
        InboundMessage {
            channel: "matrix".to_owned(),
            sender: "@alice:acme.corp".to_owned(),
            sender_name: None,
            group_id: Some("!room:acme.corp".to_owned()),
            message_id: Some("$event1".to_owned()),
            text: "hello".to_owned(),
            timestamp: 1,
            attachments: vec![],
            raw: None,
        }
    }

    #[test]
    fn dedupe_key_follows_provider_message_id_across_content() {
        let msg = dedupe_fixture();
        let mut redelivered = dedupe_fixture();
        // WHY: a provider may re-render the same event differently on
        // redelivery (edited body, refreshed timestamp); the provider ID
        // is the identity, not the content.
        redelivered.text = "hello (edited)".to_owned();
        redelivered.timestamp = 2;
        assert_eq!(msg.dedupe_key(), redelivered.dedupe_key());

        let mut other_event = dedupe_fixture();
        other_event.message_id = Some("$event2".to_owned());
        assert_ne!(msg.dedupe_key(), other_event.dedupe_key());
    }

    #[test]
    fn dedupe_key_without_provider_id_commits_to_content() {
        let mut msg = dedupe_fixture();
        msg.message_id = None;
        let mut same = dedupe_fixture();
        same.message_id = None;
        assert_eq!(msg.dedupe_key(), same.dedupe_key());

        let mut different = dedupe_fixture();
        different.message_id = None;
        different.timestamp = 2;
        assert_ne!(msg.dedupe_key(), different.dedupe_key());
    }

    #[test]
    fn dedupe_key_is_opaque() {
        let key = dedupe_fixture().dedupe_key();
        assert!(key.starts_with("sha256:"), "{key}");
        assert!(!key.contains("$event1"), "{key}");
        assert!(!key.contains("alice"), "{key}");
    }

    #[test]
    fn capture_raw_payload_disabled_by_default() {
        let policy = taxis::config::RawPayloadPolicy::default();
        assert!(!policy.capture);
        let value = serde_json::json!({"sourceNumber": "+15550100"});
        assert_eq!(capture_raw_payload(&policy, &value), None);
    }

    #[test]
    fn capture_raw_payload_redacts_and_bounds_when_enabled() {
        let policy = taxis::config::RawPayloadPolicy {
            capture: true,
            max_bytes: 4096,
        };
        let value = serde_json::json!({"sourceNumber": "+15550100", "text": "hi"});
        let captured = capture_raw_payload(&policy, &value).expect("captured");
        assert!(!captured.to_string().contains("+15550100"));

        let tiny_budget = taxis::config::RawPayloadPolicy {
            capture: true,
            max_bytes: 1,
        };
        assert_eq!(capture_raw_payload(&tiny_budget, &value), None);
    }
}
