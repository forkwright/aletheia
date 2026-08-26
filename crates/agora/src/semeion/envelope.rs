//! Signal envelope deserialization and inbound message extraction.

use serde::{Deserialize, Serialize};

use crate::types::InboundMessage;

/// A signal-cli envelope from the `receive` RPC response.
#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for SignalEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalEnvelope")
            .field("has_source_number", &self.source_number.is_some())
            .field("has_source_uuid", &self.source_uuid.is_some())
            .field("has_source_name", &self.source_name.is_some())
            .field("has_timestamp", &self.timestamp.is_some())
            .field("has_data_message", &self.data_message.is_some())
            .field("has_sync_message", &self.sync_message.is_some())
            .field("has_receipt_message", &self.receipt_message.is_some())
            .field("has_typing_message", &self.typing_message.is_some())
            .finish()
    }
}

/// The data payload of an inbound Signal message.
#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for DataMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataMessage")
            .field("has_timestamp", &self.timestamp.is_some())
            .field("has_message", &self.message.is_some())
            .field("has_group_info", &self.group_info.is_some())
            .field(
                "attachment_count",
                &self.attachments.as_ref().map(std::vec::Vec::len),
            )
            .finish()
    }
}

/// Group metadata attached to a data message.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    /// Base64-encoded group identifier.
    pub group_id: Option<String>,
}

impl std::fmt::Debug for GroupInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupInfo")
            .field("has_group_id", &self.group_id.is_some())
            .finish()
    }
}

/// A file attachment on a Signal message.
#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attachment")
            .field("has_id", &self.id.is_some())
            .field("has_content_type", &self.content_type.is_some())
            .field("has_filename", &self.filename.is_some())
            .field("has_size", &self.size.is_some())
            .finish()
    }
}

/// Expected Signal protocol traffic that carries no user message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedControlKind {
    Sync,
    Receipt,
    Typing,
    Call,
}

/// User-visible Signal content that this provider cannot represent honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsupportedContentKind {
    EditShape,
    Story,
    Reaction,
    RemoteDelete,
    Sticker,
    Contact,
    Payment,
    Poll,
    StateChange,
    Media,
}

/// Closed, privacy-safe reason an envelope could not be normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MalformedReason {
    ConflictingFamilies,
    EmptyEnvelope,
    MissingSender,
    MissingTimestamp,
    InvalidGroup,
    InvalidEdit,
    InvalidControl,
}

/// Exhaustive result of normalizing one destructively received envelope.
pub(crate) enum EnvelopeOutcome {
    Message {
        message: Box<InboundMessage>,
        /// Attachment entries consumed by signal-cli but lacking a stable ID.
        lost_parts: u64,
    },
    ExpectedControl(ExpectedControlKind),
    UnsupportedContentLost {
        kind: UnsupportedContentKind,
        lost_parts: u64,
    },
    MalformedLost {
        reason: MalformedReason,
        lost_parts: u64,
    },
}

/// Extract an [`InboundMessage`] from a public envelope projection.
///
/// `account_id` is the provider account that received the envelope, carried
/// through so multi-account deployments can distinguish identical senders.
///
#[must_use]
#[cfg(test)]
pub(crate) fn extract_message(
    envelope: &SignalEnvelope,
    account_id: Option<&str>,
) -> EnvelopeOutcome {
    let Ok(raw) = serde_json::to_value(envelope) else {
        return malformed(MalformedReason::EmptyEnvelope);
    };
    extract_received(envelope, &raw, account_id)
}

/// Normalize a Signal envelope while retaining its complete wire shape for
/// edit/control classification. Built-in Signal ingress never retains the raw
/// envelope because it contains private identifiers and message content.
#[must_use]
pub(crate) fn extract_received(
    envelope: &SignalEnvelope,
    raw: &serde_json::Value,
    account_id: Option<&str>,
) -> EnvelopeOutcome {
    let families = [
        "dataMessage",
        "editMessage",
        "storyMessage",
        "syncMessage",
        "receiptMessage",
        "typingMessage",
        "callMessage",
    ];
    let family_count = families
        .iter()
        .filter(|name| raw.get(**name).is_some_and(|value| !value.is_null()))
        .count();
    if family_count > 1 {
        return malformed(MalformedReason::ConflictingFamilies);
    }

    if let Some(data) = envelope.data_message.as_ref() {
        return normalize_data_message(envelope, data, raw, account_id);
    }

    if let Some(edit) = raw.get("editMessage") {
        let Some(data_value) = edit.get("dataMessage") else {
            return unsupported(UnsupportedContentKind::EditShape, 1);
        };
        let Ok(data) = serde_json::from_value::<DataMessage>(data_value.clone()) else {
            return malformed(MalformedReason::InvalidEdit);
        };
        return normalize_data_message(envelope, &data, raw, account_id);
    }

    if raw
        .get("storyMessage")
        .is_some_and(|value| !value.is_null())
    {
        return unsupported(UnsupportedContentKind::Story, 1);
    }
    for (field, kind) in [
        ("syncMessage", ExpectedControlKind::Sync),
        ("receiptMessage", ExpectedControlKind::Receipt),
        ("typingMessage", ExpectedControlKind::Typing),
        ("callMessage", ExpectedControlKind::Call),
    ] {
        if let Some(value) = raw.get(field).filter(|value| !value.is_null()) {
            return if value.is_object() {
                EnvelopeOutcome::ExpectedControl(kind)
            } else {
                malformed(MalformedReason::InvalidControl)
            };
        }
    }

    malformed(MalformedReason::EmptyEnvelope)
}

fn normalize_data_message(
    envelope: &SignalEnvelope,
    data: &DataMessage,
    raw: &serde_json::Value,
    account_id: Option<&str>,
) -> EnvelopeOutcome {
    let text = data
        .message
        .as_deref()
        .filter(|message| !message.is_empty())
        .unwrap_or_default();
    let attachment_count = data.attachments.as_ref().map_or(0, std::vec::Vec::len);
    let mut lost_parts = u64::try_from(attachment_count).unwrap_or(u64::MAX);

    if text.is_empty() {
        if attachment_count > 0 {
            return unsupported(UnsupportedContentKind::Media, lost_parts.max(1));
        }
        if let Some(kind) = unsupported_data_kind(raw) {
            return unsupported(kind, 1);
        }
        return malformed(MalformedReason::EmptyEnvelope);
    }
    if unsupported_data_kind(raw).is_some() {
        // Mixed text/media plus unsupported state is still partially lossy.
        lost_parts = lost_parts.saturating_add(1);
    }

    let sender = envelope
        .source_uuid
        .as_deref()
        .and_then(nonempty)
        .or_else(|| envelope.source_number.as_deref().and_then(nonempty))
        .or_else(|| {
            raw.get("source")
                .and_then(serde_json::Value::as_str)
                .and_then(nonempty)
        });
    let Some(sender) = sender else {
        return malformed(MalformedReason::MissingSender);
    };

    let group_id = match data.group_info.as_ref() {
        Some(group) => match group.group_id.as_deref().and_then(nonempty) {
            Some(id) => Some(id.to_owned()),
            None => return malformed(MalformedReason::InvalidGroup),
        },
        None => None,
    };
    let Some(timestamp) = data
        .timestamp
        .or(envelope.timestamp)
        .filter(|timestamp| *timestamp > 0)
    else {
        return malformed(MalformedReason::MissingTimestamp);
    };

    EnvelopeOutcome::Message {
        message: Box::new(InboundMessage {
            channel: "signal".to_owned(),
            sender: sender.to_owned(),
            sender_name: envelope
                .source_name
                .as_deref()
                .and_then(nonempty)
                .map(ToOwned::to_owned),
            group_id,
            account_id: account_id.map(ToOwned::to_owned),
            // signal-cli exposes no message ID; dedupe and reply idempotency
            // fall back to InboundMessage::dedupe_key's content hash.
            message_id: None,
            text: text.to_owned(),
            timestamp,
            attachments: Vec::new(),
            raw: None,
        }),
        lost_parts,
    }
}

fn unsupported(kind: UnsupportedContentKind, lost_parts: u64) -> EnvelopeOutcome {
    EnvelopeOutcome::UnsupportedContentLost {
        kind,
        lost_parts: lost_parts.max(1),
    }
}

fn malformed(reason: MalformedReason) -> EnvelopeOutcome {
    EnvelopeOutcome::MalformedLost {
        reason,
        lost_parts: 1,
    }
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

fn unsupported_data_kind(raw: &serde_json::Value) -> Option<UnsupportedContentKind> {
    let data = raw
        .get("dataMessage")
        .or_else(|| raw.get("editMessage")?.get("dataMessage"))?;
    let fields = [
        ("reaction", UnsupportedContentKind::Reaction),
        ("remoteDelete", UnsupportedContentKind::RemoteDelete),
        ("sticker", UnsupportedContentKind::Sticker),
        ("contacts", UnsupportedContentKind::Contact),
        ("payment", UnsupportedContentKind::Payment),
        ("pollCreate", UnsupportedContentKind::Poll),
        ("pollVote", UnsupportedContentKind::Poll),
        ("pollTerminate", UnsupportedContentKind::Poll),
        ("pinMessage", UnsupportedContentKind::StateChange),
        ("unpinMessage", UnsupportedContentKind::StateChange),
        ("adminDelete", UnsupportedContentKind::StateChange),
        ("groupCallUpdate", UnsupportedContentKind::StateChange),
    ];
    fields.into_iter().find_map(|(field, kind)| {
        data.get(field)
            .is_some_and(|value| !value.is_null())
            .then_some(kind)
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON key indexing on known-present keys"
)]
mod tests {
    use super::*;

    fn extracted_message(outcome: EnvelopeOutcome) -> InboundMessage {
        match outcome {
            EnvelopeOutcome::Message { message, .. } => *message,
            EnvelopeOutcome::ExpectedControl(_)
            | EnvelopeOutcome::UnsupportedContentLost { .. }
            | EnvelopeOutcome::MalformedLost { .. } => panic!("expected a normalized message"),
        }
    }

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
        let msg = extracted_message(extract_message(&env, Some("logical-account")));

        assert_eq!(msg.channel, "signal");
        assert_eq!(msg.sender, "uuid-abc");
        assert_eq!(msg.sender_name.as_deref(), Some("Alice"));
        assert_eq!(msg.text, "hello");
        assert!(msg.group_id.is_none());
        assert_eq!(msg.account_id.as_deref(), Some("logical-account"));
        assert_eq!(msg.timestamp, 1_709_312_345_678);
        assert!(msg.attachments.is_empty());
        assert!(msg.raw.is_none());
    }

    #[test]
    fn raw_envelope_is_never_retained() {
        let env: SignalEnvelope = serde_json::from_value(dm_envelope()).unwrap();
        let msg = extracted_message(extract_message(&env, Some("logical-account")));
        assert!(
            msg.raw.is_none(),
            "built-in Signal ingress must never expose the raw envelope"
        );
    }

    #[test]
    fn extract_group_message() {
        let env: SignalEnvelope = serde_json::from_value(group_envelope()).unwrap();
        let msg = extracted_message(extract_message(&env, None));

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
        assert!(matches!(
            extract_message(&env, None),
            EnvelopeOutcome::ExpectedControl(ExpectedControlKind::Sync)
        ));
    }

    #[test]
    fn extract_skips_receipt_message() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "timestamp": 100,
            "receiptMessage": {"type": "DELIVERY"}
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert!(matches!(
            extract_message(&env, None),
            EnvelopeOutcome::ExpectedControl(ExpectedControlKind::Receipt)
        ));
    }

    #[test]
    fn extract_skips_typing_indicator() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "timestamp": 100,
            "typingMessage": {"action": "STARTED"}
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert!(matches!(
            extract_message(&env, None),
            EnvelopeOutcome::ExpectedControl(ExpectedControlKind::Typing)
        ));
    }

    #[test]
    fn control_families_require_object_payloads() {
        for (field, kind) in [
            ("syncMessage", ExpectedControlKind::Sync),
            ("receiptMessage", ExpectedControlKind::Receipt),
            ("typingMessage", ExpectedControlKind::Typing),
            ("callMessage", ExpectedControlKind::Call),
        ] {
            let mut valid = serde_json::Map::new();
            valid.insert(field.to_owned(), serde_json::json!({}));
            let valid = serde_json::Value::Object(valid);
            let envelope: SignalEnvelope = serde_json::from_value(valid.clone()).unwrap();
            assert!(matches!(
                extract_received(&envelope, &valid, None),
                EnvelopeOutcome::ExpectedControl(actual) if actual == kind
            ));

            for invalid_payload in [
                serde_json::json!(42),
                serde_json::json!([]),
                serde_json::json!("invalid"),
            ] {
                let mut invalid = serde_json::Map::new();
                invalid.insert(field.to_owned(), invalid_payload);
                let invalid = serde_json::Value::Object(invalid);
                let envelope: SignalEnvelope = serde_json::from_value(invalid.clone()).unwrap();
                assert!(matches!(
                    extract_received(&envelope, &invalid, None),
                    EnvelopeOutcome::MalformedLost {
                        reason: MalformedReason::InvalidControl,
                        lost_parts: 1
                    }
                ));
            }
        }
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
        assert!(matches!(
            extract_message(&env, None),
            EnvelopeOutcome::MalformedLost {
                reason: MalformedReason::EmptyEnvelope,
                lost_parts: 1
            }
        ));
    }

    #[test]
    fn mixed_text_and_media_forwards_text_but_accounts_for_every_attachment() {
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
        match extract_message(&env, None) {
            EnvelopeOutcome::Message {
                message,
                lost_parts,
            } => {
                assert_eq!(message.text, "see attached");
                assert!(message.attachments.is_empty());
                assert_eq!(lost_parts, 2);
            }
            _ => panic!("text portion should remain forwardable"),
        }
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
        assert!(matches!(
            extract_message(&env, None),
            EnvelopeOutcome::MalformedLost {
                reason: MalformedReason::MissingSender,
                lost_parts: 1
            }
        ));
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

        assert!(matches!(
            extract_message(&env, None),
            EnvelopeOutcome::MalformedLost {
                reason: MalformedReason::MissingTimestamp,
                lost_parts: 1
            }
        ));
    }

    #[test]
    fn extract_uses_data_message_timestamp_as_fallback() {
        let json = serde_json::json!({
            "sourceNumber": "+1234567890",
            "dataMessage": {
                "timestamp": 1_709_000_000_000_u64,
                "message": "fallback ts"
            }
        });
        let env: SignalEnvelope = serde_json::from_value(json).unwrap();
        let msg = extracted_message(extract_message(&env, None));
        assert_eq!(msg.timestamp, 1_709_000_000_000);
    }

    #[test]
    fn attachment_only_message_is_loss_not_an_empty_turn() {
        let raw = serde_json::json!({
            "sourceUuid": "uuid-attachment",
            "timestamp": 100,
            "dataMessage": {
                "timestamp": 100,
                "attachments": [
                    {"id": "att-stable", "filename": "photo.jpg"},
                    {"filename": "display-only.jpg"}
                ]
            }
        });
        let envelope: SignalEnvelope = serde_json::from_value(raw.clone()).unwrap();
        match extract_received(&envelope, &raw, None) {
            EnvelopeOutcome::UnsupportedContentLost {
                kind: UnsupportedContentKind::Media,
                lost_parts,
            } => {
                assert_eq!(lost_parts, 2);
            }
            _ => panic!("attachment-only media must not become an empty turn"),
        }
    }

    #[test]
    fn edit_message_uses_nested_data_message() {
        let raw = serde_json::json!({
            "sourceUuid": "uuid-edit",
            "timestamp": 100,
            "editMessage": {
                "dataMessage": {"timestamp": 101, "message": "corrected"}
            }
        });
        let envelope: SignalEnvelope = serde_json::from_value(raw.clone()).unwrap();
        let message = extracted_message(extract_received(&envelope, &raw, None));
        assert_eq!(message.sender, "uuid-edit");
        assert_eq!(message.text, "corrected");
        assert_eq!(message.timestamp, 101);
    }

    #[test]
    fn call_is_expected_but_story_and_reaction_are_user_content_losses() {
        for (raw, expected) in [
            (
                serde_json::json!({"callMessage": {"offerMessage": {}}}),
                "call",
            ),
            (
                serde_json::json!({"storyMessage": {"textAttachment": {}}}),
                "story",
            ),
            (
                serde_json::json!({
                    "sourceUuid": "uuid-reaction",
                    "timestamp": 100,
                    "dataMessage": {"timestamp": 100, "reaction": {"emoji": "x"}}
                }),
                "reaction",
            ),
        ] {
            let envelope: SignalEnvelope = serde_json::from_value(raw.clone()).unwrap();
            let outcome = extract_received(&envelope, &raw, None);
            match expected {
                "call" => assert!(matches!(
                    outcome,
                    EnvelopeOutcome::ExpectedControl(ExpectedControlKind::Call)
                )),
                "story" => assert!(matches!(
                    outcome,
                    EnvelopeOutcome::UnsupportedContentLost {
                        kind: UnsupportedContentKind::Story,
                        lost_parts: 1
                    }
                )),
                "reaction" => assert!(matches!(
                    outcome,
                    EnvelopeOutcome::UnsupportedContentLost {
                        kind: UnsupportedContentKind::Reaction,
                        lost_parts: 1
                    }
                )),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn empty_primary_sender_falls_back_to_uuid_then_deprecated_source() {
        let uuid_raw = serde_json::json!({
            "sourceNumber": "",
            "sourceUuid": "uuid-fallback",
            "timestamp": 100,
            "dataMessage": {"message": "hello", "timestamp": 100}
        });
        let uuid_envelope: SignalEnvelope = serde_json::from_value(uuid_raw.clone()).unwrap();
        let uuid_message = extracted_message(extract_received(&uuid_envelope, &uuid_raw, None));
        assert_eq!(uuid_message.sender, "uuid-fallback");

        let source_raw = serde_json::json!({
            "sourceNumber": "",
            "sourceUuid": "",
            "source": "+15550001",
            "timestamp": 100,
            "dataMessage": {"message": "hello", "timestamp": 100}
        });
        let source_envelope: SignalEnvelope = serde_json::from_value(source_raw.clone()).unwrap();
        let source_message =
            extracted_message(extract_received(&source_envelope, &source_raw, None));
        assert_eq!(source_message.sender, "+15550001");
    }

    #[test]
    fn group_presence_requires_nonempty_group_id() {
        let raw = serde_json::json!({
            "sourceUuid": "uuid-group",
            "timestamp": 100,
            "dataMessage": {
                "message": "hello",
                "timestamp": 100,
                "groupInfo": {"groupId": ""}
            }
        });
        let envelope: SignalEnvelope = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(
            extract_received(&envelope, &raw, None),
            EnvelopeOutcome::MalformedLost {
                reason: MalformedReason::InvalidGroup,
                lost_parts: 1
            }
        ));
    }

    #[test]
    fn debug_projections_do_not_expose_signal_content_or_identifiers() {
        let raw = dm_envelope();
        let envelope: SignalEnvelope = serde_json::from_value(raw).unwrap();
        let debug = format!("{envelope:?}");
        for secret in ["+1234567890", "uuid-abc", "Alice", "hello"] {
            assert!(!debug.contains(secret), "Debug leaked {secret}: {debug}");
        }
    }
}
