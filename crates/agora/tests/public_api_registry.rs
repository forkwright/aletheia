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

// ── ChannelRegistry ──

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

    fn listen(
        &self,
        _poll_interval: Option<std::time::Duration>,
        _cancel: CancellationToken,
    ) -> (
        tokio::sync::mpsc::Receiver<InboundMessage>,
        tokio::task::JoinSet<()>,
    ) {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        (rx, tokio::task::JoinSet::new())
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
    assert!(
        err_msg.contains("signal"),
        "error should name the channel: {err_msg}"
    );
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
    let result = registry
        .send("signal", &params)
        .await
        .expect("send succeeds");
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

    let result = registry
        .send("signal", &params)
        .await
        .expect("send succeeds");
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

    let err = registry
        .send("nonexistent", &params)
        .await
        .expect_err("send fails");
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

    registry
        .register(Arc::new(ok_provider))
        .expect("register signal");
    registry
        .register(Arc::new(fail_provider))
        .expect("register slack");

    let results = registry.probe_all().await;
    assert_eq!(results.len(), 2);
    assert!(results.get("signal").expect("signal result").ok);
    assert!(!results.get("slack").expect("slack result").ok);
}
