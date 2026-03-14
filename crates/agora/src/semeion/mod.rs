//! Signal channel provider — wraps signal-cli JSON-RPC.

/// JSON-RPC client for the signal-cli HTTP daemon.
pub mod client;
/// Connection state machine and outbound message buffering during disconnection.
pub mod connection;
/// Signal envelope deserialization and inbound message extraction.
pub mod envelope;
/// Signal-specific error types.
pub mod error;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{Instrument, instrument};

use crate::types::{
    ChannelCapabilities, ChannelProvider, InboundMessage, ProbeResult,
    SendParams as ChannelSendParams, SendResult,
};
use connection::{AccountState, ConnectionHealthReport, ConnectionState, reconnect_delay};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_BUFFER_CAPACITY: usize = 100;

/// Parsed Signal message target.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignalTarget {
    /// Direct message to a phone number (e.g., `"+1234567890"`).
    Phone(String),
    /// Group message identified by base64 group ID.
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
/// Tracks connection state per account with reconnect backoff and
/// outbound message buffering during disconnection.
pub struct SignalProvider {
    clients: HashMap<String, client::SignalClient>,
    default_account: Option<String>,
    account_states: HashMap<String, Arc<Mutex<AccountState>>>,
    buffer_capacity: usize,
}

impl SignalProvider {
    /// Create an empty provider. Add accounts with [`add_account`](Self::add_account).
    #[must_use]
    pub fn new() -> Self {
        Self::with_buffer_capacity(DEFAULT_BUFFER_CAPACITY)
    }

    /// Create a provider with a custom outbound buffer capacity.
    #[must_use]
    pub fn with_buffer_capacity(capacity: usize) -> Self {
        Self {
            clients: HashMap::new(),
            default_account: None,
            account_states: HashMap::new(),
            buffer_capacity: capacity,
        }
    }

    /// Register a Signal account backed by a client.
    ///
    /// The first account added becomes the default.
    pub fn add_account(&mut self, account_id: String, client: client::SignalClient) {
        if self.default_account.is_none() {
            self.default_account = Some(account_id.clone());
        }
        self.account_states.insert(
            account_id.clone(),
            Arc::new(Mutex::new(AccountState::new(self.buffer_capacity))),
        );
        self.clients.insert(account_id, client);
    }

    /// Start listening for inbound messages on all registered accounts.
    ///
    /// Spawns a polling task per account with reconnect backoff.
    /// Messages from all accounts merge into the returned receiver.
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
            #[expect(
                clippy::expect_used,
                reason = "account_states and clients share the same key set; state is always inserted alongside the client in add_account"
            )]
            let state = self
                .account_states
                .get(&account_id)
                .expect("state initialized in add_account")
                .clone();
            let span = tracing::info_span!(
                "signal_poll",
                account = %account_id
            );

            let handle =
                tokio::spawn(poll_loop(signal_client, tx, interval, state).instrument(span));
            handles.push(handle);
        }

        (rx, handles)
    }

    /// Query connection health for all accounts.
    pub async fn connection_health(&self) -> HashMap<String, ConnectionHealthReport> {
        let mut reports = HashMap::new();
        for (account_id, state_mutex) in &self.account_states {
            let s = state_mutex.lock().await;
            reports.insert(
                account_id.clone(),
                ConnectionHealthReport {
                    state: s.state.clone(),
                    buffered_messages: s.buffered_count(),
                    dropped_count: s.dropped_count,
                },
            );
        }
        reports
    }

    fn resolve_client(&self, account_id: Option<&str>) -> Option<(&str, &client::SignalClient)> {
        let key = account_id.or(self.default_account.as_deref())?;
        self.clients
            .get_key_value(key)
            .map(|(k, v)| (k.as_str(), v))
    }

    fn build_send_params(account: &str, params: &ChannelSendParams) -> client::SendParams {
        let target = parse_target(&params.to);
        match target {
            SignalTarget::Phone(phone) => client::SendParams {
                message: Some(params.text.clone()),
                recipient: Some(phone),
                group_id: None,
                account: Some(account.to_owned()),
                attachments: params.attachments.clone(),
            },
            SignalTarget::Group(group_id) => client::SendParams {
                message: Some(params.text.clone()),
                recipient: None,
                group_id: Some(group_id),
                account: Some(account.to_owned()),
                attachments: params.attachments.clone(),
            },
        }
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

            let account_id = account.to_owned();
            let send_params = Self::build_send_params(account, params);

            let Some(state_mutex) = self.account_states.get(&account_id) else {
                return SendResult {
                    sent: false,
                    error: Some("account state not initialized".to_owned()),
                };
            };

            // WHY: buffer immediately when the connection is known to be unavailable.
            // For Connected state: attempt the send directly — no separate check-then-act,
            // so there is no TOCTOU window. If the connection dropped since we last polled,
            // the HTTP error handler below buffers the message.
            {
                let mut s = state_mutex.lock().await;
                if s.state != ConnectionState::Connected {
                    s.enqueue(send_params);
                    return SendResult {
                        sent: false,
                        error: Some("connection unavailable, message buffered".to_owned()),
                    };
                }
            }

            match client.send_message(&send_params).await {
                Ok(_) => SendResult {
                    sent: true,
                    error: None,
                },
                Err(e) => {
                    // WHY: buffer on transport failure — handles the case where the connection
                    // dropped between the state check and this send (no TOCTOU window).
                    if matches!(e, error::Error::Http { .. }) {
                        let mut s = state_mutex.lock().await;
                        s.enqueue(send_params);
                    }
                    SendResult {
                        sent: false,
                        error: Some(format!("{e}")),
                    }
                }
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
                let mut detail = serde_json::Map::new();
                detail.insert(String::from("reachable"), serde_json::Value::Bool(ok));

                if let Some(state_mutex) = self.account_states.get(account_id) {
                    let s = state_mutex.lock().await;
                    detail.insert(
                        String::from("connection_state"),
                        serde_json::Value::String(format!("{:?}", s.state)),
                    );
                    detail.insert(
                        String::from("buffered_messages"),
                        serde_json::Value::Number(s.buffered_count().into()),
                    );
                }

                account_results.insert(account_id.clone(), serde_json::Value::Object(detail));
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
            .field("account_states_count", &self.account_states.len())
            .field("buffer_capacity", &self.buffer_capacity)
            .finish()
    }
}

async fn poll_loop(
    signal_client: client::SignalClient,
    tx: mpsc::Sender<InboundMessage>,
    interval: Duration,
    state: Arc<Mutex<AccountState>>,
) {
    tracing::info!("polling started");
    loop {
        match signal_client.receive(None).await {
            Ok(envelopes) => {
                {
                    let mut s = state.lock().await;
                    if s.state != ConnectionState::Connected {
                        tracing::info!("connection restored");
                        s.state = ConnectionState::Connected;

                        let buffered = s.drain_all();
                        drop(s); // release lock before sending

                        if !buffered.is_empty() {
                            tracing::info!(count = buffered.len(), "draining buffered messages");
                            let mut failed = Vec::new();
                            for params in buffered {
                                if let Err(e) = signal_client.send_message(&params).await {
                                    tracing::warn!(error = %e, "failed to send buffered message");
                                    failed.push(params);
                                }
                            }
                            if !failed.is_empty() {
                                tracing::warn!(
                                    count = failed.len(),
                                    "retaining undelivered messages in buffer for next connection"
                                );
                                let mut s = state.lock().await;
                                for params in failed {
                                    s.enqueue(params);
                                }
                            }
                        }
                    }
                }

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
                tokio::time::sleep(interval).await;
            }
            Err(e) => {
                let attempt = {
                    let mut s = state.lock().await;
                    match s.state {
                        ConnectionState::Connected => {
                            s.state = ConnectionState::Reconnecting { attempt: 1 };
                            1
                        }
                        ConnectionState::Reconnecting { attempt } => {
                            let next = attempt.saturating_add(1);
                            s.state = ConnectionState::Reconnecting { attempt: next };
                            next
                        }
                    }
                };

                let delay = reconnect_delay(attempt);
                tracing::warn!(
                    error = %e,
                    attempt,
                    backoff_secs = delay.as_secs(),
                    "receive poll failed, backing off"
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

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
        let provider = SignalProvider::new();
        let (rx, handles) = provider.listen(None);
        assert!(handles.is_empty());
        drop(rx);
    }

    #[tokio::test]
    async fn listen_returns_receiver_and_handles() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

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

        drop(rx);
        for h in handles {
            let _ = tokio::time::timeout(Duration::from_secs(5), h).await;
        }
    }

    #[tokio::test]
    async fn poll_loop_stops_on_receiver_drop() {
        install_crypto_provider();
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
        let account_state = Arc::new(Mutex::new(AccountState::new(100)));

        let handle = tokio::spawn(super::poll_loop(
            signal_client,
            tx,
            Duration::from_millis(50),
            account_state,
        ));

        let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("message");
        assert_eq!(msg.text, "test msg");

        drop(rx);
        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "poll loop should stop when receiver is dropped"
        );
    }

    #[tokio::test]
    async fn connection_health_reports_state() {
        install_crypto_provider();
        let mut provider = SignalProvider::with_buffer_capacity(50);
        let server = wiremock::MockServer::start().await;
        let signal_client = client::SignalClient::new(&server.uri()).expect("client");
        provider.add_account("+1111111111".to_owned(), signal_client);

        let health = provider.connection_health().await;
        let report = health.get("+1111111111").expect("account present");
        assert_eq!(report.state, ConnectionState::Connected);
        assert_eq!(report.buffered_messages, 0);
        assert_eq!(report.dropped_count, 0);
    }
}
