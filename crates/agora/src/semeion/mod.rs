//! Signal channel provider — wraps signal-cli JSON-RPC.

pub mod client;
pub mod envelope;
pub mod error;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{Instrument, instrument};

use crate::types::{
    ChannelCapabilities, ChannelProvider, InboundMessage, ProbeResult,
    SendParams as ChannelSendParams, SendResult,
};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Parsed Signal message target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalTarget {
    Phone(String),
    Group(String),
}

/// Parse a target string into a `SignalTarget`.
///
/// - `"group:<base64id>"` → `Group`
/// - anything else (e.g., `"+1234567890"`) → `Phone`
#[must_use]
pub fn parse_target(to: &str) -> SignalTarget {
    if let Some(group_id) = to.strip_prefix("group:") {
        SignalTarget::Group(group_id.to_owned())
    } else {
        SignalTarget::Phone(to.to_owned())
    }
}

static SIGNAL_CAPABILITIES: ChannelCapabilities = ChannelCapabilities {
    threads: false,
    reactions: true,
    typing: true,
    media: true,
    streaming: false,
    rich_formatting: false,
    max_text_length: 2000,
};

/// Signal channel provider implementing `ChannelProvider`.
///
/// Manages multiple Signal accounts, each backed by a `SignalClient`.
/// The first registered account becomes the default for sends without
/// an explicit `account_id`.
pub struct SignalProvider {
    clients: HashMap<String, client::SignalClient>,
    default_account: Option<String>,
}

impl SignalProvider {
    /// Create an empty provider. Add accounts with [`add_account`](Self::add_account).
    #[must_use]
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            default_account: None,
        }
    }

    /// Register a Signal account backed by a client.
    ///
    /// The first account added becomes the default.
    pub fn add_account(&mut self, account_id: String, client: client::SignalClient) {
        if self.default_account.is_none() {
            self.default_account = Some(account_id.clone());
        }
        self.clients.insert(account_id, client);
    }

    /// Start listening for inbound messages on all registered accounts.
    ///
    /// Spawns a polling task per account. Messages from all accounts merge
    /// into the returned receiver. Dropping the receiver stops all tasks
    /// (the send half detects the closed channel).
    #[instrument(skip(self))]
    pub fn listen(
        &self,
        poll_interval: Option<Duration>,
    ) -> (mpsc::Receiver<InboundMessage>, Vec<JoinHandle<()>>) {
        let interval = poll_interval.unwrap_or(DEFAULT_POLL_INTERVAL);
        let (tx, rx) = mpsc::channel(64);
        let mut handles = Vec::with_capacity(self.clients.len());

        for (account_id, signal_client) in &self.clients {
            let tx = tx.clone();
            let account_id = account_id.clone();
            let signal_client = signal_client.clone();
            let span = tracing::info_span!(
                "signal_poll",
                account = %account_id
            );

            let handle =
                tokio::spawn(poll_loop(signal_client, account_id, tx, interval).instrument(span));
            handles.push(handle);
        }

        (rx, handles)
    }

    fn resolve_client(&self, account_id: Option<&str>) -> Option<(&str, &client::SignalClient)> {
        let key = account_id.or(self.default_account.as_deref())?;
        self.clients
            .get_key_value(key)
            .map(|(k, v)| (k.as_str(), v))
    }
}

impl Default for SignalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelProvider for SignalProvider {
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature requires &str"
    )]
    fn id(&self) -> &str {
        "signal"
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature requires &str"
    )]
    fn name(&self) -> &str {
        "Signal"
    }

    fn capabilities(&self) -> &ChannelCapabilities {
        &SIGNAL_CAPABILITIES
    }

    fn send<'a>(
        &'a self,
        params: &'a ChannelSendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
        Box::pin(async move {
            let Some((account, client)) = self.resolve_client(params.account_id.as_deref()) else {
                return SendResult {
                    sent: false,
                    error: Some("no Signal client available".to_owned()),
                };
            };

            let target = parse_target(&params.to);

            let send_params = match target {
                SignalTarget::Phone(ref phone) => client::SendParams {
                    message: Some(params.text.clone()),
                    recipient: Some(phone.clone()),
                    group_id: None,
                    account: Some(account.to_owned()),
                    attachments: params.attachments.clone(),
                },
                SignalTarget::Group(ref group_id) => client::SendParams {
                    message: Some(params.text.clone()),
                    recipient: None,
                    group_id: Some(group_id.clone()),
                    account: Some(account.to_owned()),
                    attachments: params.attachments.clone(),
                },
            };

            match client.send_message(&send_params).await {
                Ok(_) => SendResult {
                    sent: true,
                    error: None,
                },
                Err(e) => SendResult {
                    sent: false,
                    error: Some(format!("{e}")),
                },
            }
        })
    }

    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
        Box::pin(async move {
            if self.clients.is_empty() {
                return ProbeResult {
                    ok: false,
                    latency_ms: None,
                    error: Some("no Signal clients configured".to_owned()),
                    details: None,
                };
            }

            let mut account_results = HashMap::new();
            let mut any_ok = false;

            for (account_id, client) in &self.clients {
                let ok = client.health().await;
                if ok {
                    any_ok = true;
                }
                account_results.insert(account_id.clone(), serde_json::Value::Bool(ok));
            }

            ProbeResult {
                ok: any_ok,
                latency_ms: None,
                error: if any_ok {
                    None
                } else {
                    Some("all Signal accounts unreachable".to_owned())
                },
                details: Some(account_results),
            }
        })
    }
}

impl std::fmt::Debug for SignalProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalProvider")
            .field("accounts", &self.clients.keys().collect::<Vec<_>>())
            .field("default_account", &self.default_account)
            .finish()
    }
}

async fn poll_loop(
    signal_client: client::SignalClient,
    account_id: String,
    tx: mpsc::Sender<InboundMessage>,
    interval: Duration,
) {
    tracing::info!("polling started");
    loop {
        match signal_client.receive(Some(&account_id)).await {
            Ok(envelopes) => {
                for env in &envelopes {
                    if let Some(msg) = envelope::extract_message(env) {
                        if tx.send(msg).await.is_err() {
                            tracing::info!("receiver dropped, stopping poll");
                            return;
                        }
                    } else {
                        tracing::debug!("skipping non-message envelope");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "receive poll failed");
            }
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_phone() {
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
    fn parse_target_plain_text() {
        let target = parse_target("someuser");
        assert_eq!(target, SignalTarget::Phone("someuser".to_owned()));
    }

    #[test]
    fn signal_capabilities() {
        assert!(!SIGNAL_CAPABILITIES.threads);
        assert!(SIGNAL_CAPABILITIES.reactions);
        assert!(SIGNAL_CAPABILITIES.typing);
        assert!(SIGNAL_CAPABILITIES.media);
        assert!(!SIGNAL_CAPABILITIES.streaming);
        assert!(!SIGNAL_CAPABILITIES.rich_formatting);
        assert_eq!(SIGNAL_CAPABILITIES.max_text_length, 2000);
    }

    #[test]
    fn provider_id_and_name() {
        let provider = SignalProvider::new();
        assert_eq!(ChannelProvider::id(&provider), "signal");
        assert_eq!(ChannelProvider::name(&provider), "Signal");
    }

    #[test]
    fn provider_capabilities_ref() {
        let provider = SignalProvider::new();
        let caps = provider.capabilities();
        assert_eq!(caps.max_text_length, 2000);
    }

    #[test]
    fn listen_empty_provider_returns_empty() {
        // Create a runtime manually to avoid #[tokio::test] overhead
        // when we just need to verify the return shape.
        let provider = SignalProvider::new();
        let (rx, handles) = provider.listen(None);
        assert!(handles.is_empty());
        drop(rx);
    }

    #[tokio::test]
    async fn listen_returns_receiver_and_handles() {
        let server = wiremock::MockServer::start().await;

        // Return empty result so the poll loop has something to do
        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": [],
            "id": "test"
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&rpc_response))
            .mount(&server)
            .await;

        let mut provider = SignalProvider::new();
        let signal_client = client::SignalClient::new(&server.uri()).expect("client");
        provider.add_account("+1111111111".to_owned(), signal_client);

        let (rx, handles) = provider.listen(Some(Duration::from_secs(60)));
        assert_eq!(handles.len(), 1);

        // Drop receiver to stop the poll tasks
        drop(rx);

        // Wait for task to finish (it should detect closed channel)
        for h in handles {
            // Use a timeout so the test doesn't hang
            let _ = tokio::time::timeout(Duration::from_secs(5), h).await;
        }
    }

    #[tokio::test]
    async fn poll_loop_stops_on_receiver_drop() {
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": [
                {
                    "envelope": {
                        "sourceNumber": "+9999999999",
                        "timestamp": 100,
                        "dataMessage": {
                            "timestamp": 100,
                            "message": "test msg"
                        }
                    }
                }
            ],
            "id": "test"
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&rpc_response))
            .mount(&server)
            .await;

        let signal_client = client::SignalClient::new(&server.uri()).expect("client");
        let (tx, mut rx) = mpsc::channel(16);

        let handle = tokio::spawn(super::poll_loop(
            signal_client,
            "+0000000000".to_owned(),
            tx,
            Duration::from_millis(50),
        ));

        // Receive one message
        let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("message");
        assert_eq!(msg.text, "test msg");

        // Drop receiver — poll loop should stop
        drop(rx);
        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "poll loop should stop when receiver is dropped"
        );
    }
}
