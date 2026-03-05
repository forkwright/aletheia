//! Signal envelope deserialization and inbound message extraction.

use serde::{Deserialize, Serialize};

use crate::types::InboundMessage;

/// A signal-cli envelope from the `receive` RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalEnvelope {
    /// Sender's phone number (e.g., `"+1234567890"`).
    pub source_number: Option<String>,
    /// Sender's UUID (alternative identifier when phone number is unavailable).
    pub source_uuid: Option<String>,
    /// Sender's display name from their Signal profile.
    pub source_name: Option<String>,
    /// Unix timestamp in milliseconds when the envelope was created.
    pub timestamp: Option<u64>,
    /// Data message payload (the actual message content).
    #[serde(default)]
    pub data_message: Option<DataMessage>,
    /// Sync message from a linked device (ignored for inbound processing).
    #[serde(default)]
    pub sync_message: Option<serde_json::Value>,
    /// Delivery/read receipt (ignored for inbound processing).
    #[serde(default)]
    pub receipt_message: Option<serde_json::Value>,
    /// Typing indicator (ignored for inbound processing).
    #[serde(default)]
    pub typing_message: Option<serde_json::Value>,
}

/// The data payload of an inbound Signal message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMessage {
    /// Unix timestamp in milliseconds for this specific message.
    pub timestamp: Option<u64>,
    /// Text body of the message.
    pub message: Option<String>,
    /// Group metadata if this message was sent to a group.
    #[serde(default)]
    pub group_info: Option<GroupInfo>,
    /// File attachments included with the message.
    #[serde(default)]
    pub attachments: Option<Vec<Attachment>>,
}

/// Group metadata attached to a data message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    /// Base64-encoded group identifier.
    pub group_id: Option<String>,
}

/// A file attachment on a Signal message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// Signal-assigned attachment identifier.
    pub id: Option<String>,
    /// MIME type (e.g., `"image/jpeg"`, `"application/pdf"`).
    pub content_type: Option<String>,
    /// Original filename if provided by the sender.
    pub filename: Option<String>,
    /// File size in bytes.
    pub size: Option<u64>,
}

/// Extract an [`InboundMessage`] from a signal envelope, if it contains usable content.
///
/// Returns `None` for sync messages, receipt messages, typing indicators,
/// data messages with no text, and messages with no identifiable sender.
pub fn extract_message(envelope: &SignalEnvelope) -> Option<InboundMessage> {
    let data = envelope.data_message.as_ref()?;

    let text = data.message.as_deref()?;
    if text.is_empty() {
        return None;
    }

    let sender = envelope
        .source_number
        .as_deref()
        .or(envelope.source_uuid.as_deref())?;

    let group_id = data.group_info.as_ref().and_then(|g| g.group_id.clone());

    let attachments = data
        .attachments
        .as_ref()
        .map(|atts| {
            atts.iter()
                .filter_map(|a| a.filename.clone().or_else(|| a.id.clone()))
                .collect()
        })
        .unwrap_or_default();

    let raw_value = serde_json::to_value(envelope).ok();

    Some(InboundMessage {
        channel: "signal".to_owned(),
        sender: sender.to_owned(),
        sender_name: envelope.source_name.clone(),
        group_id,
        text: text.to_owned(),
        timestamp: envelope.timestamp.or(data.timestamp).unwrap_or(0),
        attachments,
        raw: raw_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dm_envelope() -> serde_json::Value {
        serde_json::json!({
            "sourceNumber": "+1234567890",
            "sourceUuid": "uuid-abc",
            "sourceName": "Alice",
            "timestamp": 1_709_312_345_678_u64,
            "dataMessage": {
                "timestamp": 1_709_312_345_678_u64,
                "message": "hello",
                "groupInfo": null
            }
        })
    }

    fn group_envelope() -> serde_json::Value {
        serde_json::json!({
            "sourceNumber": "+1234567890",
            "sourceName": "Bob",
            "timestamp": 1_709_312_345_000_u64,
            "dataMessage": {
                "timestamp": 1_709_312_345_000_u64,
                "message": "group hello",
                "groupInfo": {
                    "groupId": "YWJjMTIz"
                }
            }
        })
    }

    #[test]
    fn extract_dm_with_text() {
        let env: SignalEnvelope = serde_json::from_value(dm_envelope()).unwrap();
        let msg = extract_message(&env).unwrap();

        assert_eq!(msg.channel, "signal");
        assert_eq!(msg.sender, "+1234567890");
        assert_eq!(msg.sender_name.as_deref(), Some("Alice"));
        assert_eq!(msg.text, "hello");
        assert!(msg.group_id.is_none());
        assert_eq!(msg.timestamp, 1_709_312_345_678);
        assert!(msg.attachments.is_empty());
        assert!(msg.raw.is_some());
    }

    #[test]
    fn extract_group_message() {
        let env: SignalEnvelope = serde_json::from_value(group_envelope()).unwrap();
        let msg = extract_message(&env).unwrap();

        assert_eq!(msg.sender, "+1234567890");
        assert_eq!(msg.text, "group hello");
        assert_eq!(msg.group_id.as_deref(), Some("YWJjMTIz"));
    }

    #[test]
    fn extract_skips_sync_message() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "timestamp": 100,
            "syncMessage": {"sentMessage": {}}
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert!(extract_message(&env).is_none());
    }

    #[test]
    fn extract_skips_receipt_message() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "timestamp": 100,
            "receiptMessage": {"type": "DELIVERY"}
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert!(extract_message(&env).is_none());
    }

    #[test]
    fn extract_skips_typing_indicator() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "timestamp": 100,
            "typingMessage": {"action": "STARTED"}
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert!(extract_message(&env).is_none());
    }

    #[test]
    fn extract_skips_empty_data_message() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "timestamp": 100,
            "dataMessage": {
                "timestamp": 100
            }
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert!(extract_message(&env).is_none());
    }

    #[test]
    fn extract_uses_attachment_filenames() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "timestamp": 100,
            "dataMessage": {
                "timestamp": 100,
                "message": "see attached",
                "attachments": [
                    {"id": "att-1", "filename": "photo.jpg", "contentType": "image/jpeg", "size": 1024},
                    {"id": "att-2", "contentType": "application/pdf", "size": 2048}
                ]
            }
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        let msg = extract_message(&env).unwrap();

        assert_eq!(msg.attachments.len(), 2);
        assert_eq!(msg.attachments[0], "photo.jpg");
        assert_eq!(msg.attachments[1], "att-2"); // falls back to id
    }

    #[test]
    fn extract_no_sender_returns_none() {
        let json = serde_json::json!({
            "timestamp": 100,
            "dataMessage": {
                "timestamp": 100,
                "message": "ghost message"
            }
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert!(extract_message(&env).is_none());
    }

    #[test]
    fn envelope_deserialize_full() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "sourceUuid": "abc-def",
            "sourceName": "Test User",
            "timestamp": 1_709_312_345_678_u64,
            "dataMessage": {
                "timestamp": 1_709_312_345_678_u64,
                "message": "full message",
                "groupInfo": {
                    "groupId": "grp123"
                },
                "attachments": [
                    {"id": "a1", "filename": "doc.pdf", "contentType": "application/pdf", "size": 999}
                ]
            }
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();

        assert_eq!(env.source_number.as_deref(), Some("+1234567890"));
        assert_eq!(env.source_uuid.as_deref(), Some("abc-def"));
        assert_eq!(env.source_name.as_deref(), Some("Test User"));
        assert_eq!(env.timestamp, Some(1_709_312_345_678));

        let data = env.data_message.as_ref().unwrap();
        assert_eq!(data.message.as_deref(), Some("full message"));
        assert_eq!(
            data.group_info.as_ref().unwrap().group_id.as_deref(),
            Some("grp123")
        );
        assert_eq!(data.attachments.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn envelope_deserialize_minimal() {
        let json = serde_json::json!({
            "sourceNumber": "+5555555555",
            "dataMessage": {
                "message": "hi"
            }
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();

        assert_eq!(env.source_number.as_deref(), Some("+5555555555"));
        assert!(env.source_uuid.is_none());
        assert!(env.source_name.is_none());
        assert!(env.timestamp.is_none());
        assert!(env.sync_message.is_none());

        let msg = extract_message(&env).unwrap();
        assert_eq!(msg.text, "hi");
        assert_eq!(msg.timestamp, 0); // no timestamp available
    }
}
