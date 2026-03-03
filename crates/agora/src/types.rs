//! Core types for the channel abstraction layer.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// Capability flags for a channel provider.
///
/// Channels vary widely in what they support. Use these flags to guard
/// features (e.g., skip typing indicators on channels where `typing` is false).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "capability flags are inherently boolean"
)]
pub struct ChannelCapabilities {
    /// Whether threaded replies are supported.
    pub threads: bool,
    /// Whether emoji reactions are supported.
    pub reactions: bool,
    /// Whether typing indicators are supported.
    pub typing: bool,
    /// Whether file/image attachments are supported.
    pub media: bool,
    /// Whether real-time streaming responses are supported.
    pub streaming: bool,
    /// Whether markdown or other rich formatting is rendered.
    pub rich_formatting: bool,
    /// Maximum message length in characters before truncation is required.
    pub max_text_length: usize,
}

/// Parameters for sending a message through a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendParams {
    /// Target identifier (channel-specific: phone number, group ID, etc.)
    pub to: String,
    /// Message text (markdown).
    pub text: String,
    /// Account ID within the channel (for multi-account setups).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
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
    pub sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Health probe result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    /// Message text content.
    pub text: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Attachment file paths or identifiers.
    pub attachments: Vec<String>,
    /// Raw channel-specific payload for extensions.
    pub raw: Option<serde_json::Value>,
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

    /// Health probe for this channel.
    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_message_serde_roundtrip() {
        let msg = InboundMessage {
            channel: "signal".to_owned(),
            sender: "+1234567890".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: Some("grp123".to_owned()),
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
        assert_eq!(back.text, msg.text);
        assert_eq!(back.timestamp, msg.timestamp);
        assert_eq!(back.attachments, msg.attachments);
        assert_eq!(back.raw, msg.raw);
    }
}
