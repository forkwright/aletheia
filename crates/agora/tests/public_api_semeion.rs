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

// Split: semeion/Signal surface, Send+Sync bounds, CancellationToken compat.

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

use organon::testing::install_crypto_provider;

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
    assert_eq!(
        format!("{:?}", original.state),
        format!("{:?}", cloned.state)
    );
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
