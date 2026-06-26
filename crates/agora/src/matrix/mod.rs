//! Matrix channel provider backed by the Matrix Client-Server API.

/// Matrix Client-Server API client.
pub mod client;
/// Matrix-specific error types.
pub mod error;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, instrument};

use crate::types::{
    ChannelCapabilities, ChannelProvider, InboundMessage, ProbeResult,
    SendParams as ChannelSendParams, SendResult,
};

/// Fallback default; runtime reads `MessagingConfig::poll_interval_ms`.
pub(crate) const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

static MATRIX_CAPABILITIES: ChannelCapabilities = ChannelCapabilities {
    threads: true,
    reactions: false,
    typing: false,
    media: false,
    streaming: false,
    rich_formatting: false,
    max_text_length: 65_536,
};

/// Joined-room section from Matrix `/sync`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixSyncResponse {
    /// Batch token to pass as `since` on the next sync.
    pub next_batch: Option<String>,
    /// Joined rooms returned by sync.
    #[serde(default)]
    pub rooms: MatrixRooms,
}

/// Matrix `/sync` rooms container.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixRooms {
    /// Joined rooms keyed by room ID.
    #[serde(default)]
    pub join: HashMap<String, MatrixJoinedRoom>,
}

/// Matrix joined-room sync payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixJoinedRoom {
    /// Timeline events returned for the room.
    #[serde(default)]
    pub timeline: MatrixTimeline,
}

/// Matrix room timeline payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixTimeline {
    /// Timeline events.
    #[serde(default)]
    pub events: Vec<MatrixEvent>,
}

/// Matrix event subset used by message extraction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixEvent {
    /// Event type, e.g. `m.room.message`.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Matrix user ID of the sender.
    pub sender: Option<String>,
    /// Event ID.
    pub event_id: Option<String>,
    /// Server timestamp in milliseconds.
    pub origin_server_ts: Option<u64>,
    /// Event content.
    #[serde(default)]
    pub content: MatrixEventContent,
    /// Raw unsigned metadata.
    #[serde(default)]
    pub unsigned: Option<serde_json::Value>,
}

/// Matrix room-message content subset.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixEventContent {
    /// Matrix message type, e.g. `m.text`.
    pub msgtype: Option<String>,
    /// Plain-text body.
    pub body: Option<String>,
    /// Additional content fields retained for attachments and raw diagnostics.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Clone)]
struct MatrixAccount {
    client: client::MatrixClient,
    user_id: Option<String>,
    auto_start: bool,
    since: Arc<Mutex<Option<String>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
}

/// Matrix channel provider implementing `ChannelProvider`.
pub struct MatrixProvider {
    accounts: HashMap<String, MatrixAccount>,
    default_account: Option<String>,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
}

impl MatrixProvider {
    /// Create an empty Matrix provider.
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            default_account: None,
            circuit_breaker_threshold: 5,
            halted_health_check_interval: Duration::from_mins(1),
        }
    }

    /// Create a Matrix provider from messaging config.
    #[must_use]
    pub fn from_config(config: &taxis::config::MessagingConfig) -> Self {
        Self {
            accounts: HashMap::new(),
            default_account: None,
            circuit_breaker_threshold: config.circuit_breaker_threshold,
            halted_health_check_interval: Duration::from_secs(
                config.halted_health_check_interval_secs,
            ),
        }
    }

    /// Register a Matrix account.
    pub fn add_account(
        &mut self,
        account_id: String,
        client: client::MatrixClient,
        user_id: Option<String>,
        auto_start: bool,
        initial_since: Option<String>,
    ) {
        if self.default_account.is_none() {
            self.default_account = Some(account_id.clone());
        }
        self.accounts.insert(
            account_id,
            MatrixAccount {
                client,
                user_id,
                auto_start,
                since: Arc::new(Mutex::new(initial_since)), // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
            },
        );
    }

    /// Start Matrix `/sync` loops for accounts with `auto_start=true`.
    #[instrument(skip(self, cancel))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "CancellationToken is Arc-backed; pass-by-value is idiomatic"
    )]
    pub fn listen(
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
        let interval = poll_interval.unwrap_or(DEFAULT_POLL_INTERVAL);
        let (tx, rx) = mpsc::channel(64);
        let mut handles = JoinSet::new();

        for (account_id, account) in &self.accounts {
            if !account.auto_start {
                tracing::info!(account = %account_id, "skipping Matrix sync loop (auto_start=false)");
                continue;
            }

            let tx = tx.clone();
            let token = cancel.clone();
            let client = account.client.clone();
            let since = Arc::clone(&account.since);
            let user_id = account.user_id.clone();
            let span = tracing::info_span!("matrix_sync", account = %account_id);

            handles.spawn(
                sync_loop(
                    client,
                    tx,
                    interval,
                    since,
                    user_id,
                    token,
                    self.circuit_breaker_threshold,
                    self.halted_health_check_interval,
                )
                .instrument(span),
            );
        }

        (rx, handles)
    }

    fn resolve_account(&self, account_id: Option<&str>) -> Option<&MatrixAccount> {
        let key = account_id.or(self.default_account.as_deref())?;
        self.accounts.get(key)
    }
}

impl Default for MatrixProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelProvider for MatrixProvider {
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature requires &str"
    )]
    fn id(&self) -> &str {
        "matrix"
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature requires &str"
    )]
    fn name(&self) -> &str {
        "Matrix"
    }

    fn capabilities(&self) -> &ChannelCapabilities {
        &MATRIX_CAPABILITIES
    }

    fn send<'a>(
        &'a self,
        params: &'a ChannelSendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
        Box::pin(async move {
            if params
                .attachments
                .as_ref()
                .is_some_and(|items| !items.is_empty())
            {
                return SendResult::err("Matrix media attachments are not supported");
            }

            let Some(account) = self.resolve_account(params.account_id.as_deref()) else {
                return SendResult::err("no Matrix client available");
            };

            match account
                .client
                .send_text(&params.to, &params.text, params.thread_id.as_deref())
                .await
            {
                Ok(_) => SendResult::ok(),
                Err(e) => SendResult::err(e.to_string()),
            }
        })
    }

    fn listen(
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
        MatrixProvider::listen(self, poll_interval, cancel)
    }

    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
        Box::pin(async move {
            if self.accounts.is_empty() {
                return ProbeResult {
                    ok: false,
                    latency_ms: None,
                    error: Some("no Matrix clients configured".to_owned()),
                    details: None,
                };
            }

            let mut details = HashMap::new();
            let mut any_ok = false;
            for (account_id, account) in &self.accounts {
                let ok = account.client.health().await;
                any_ok |= ok;
                details.insert(
                    account_id.clone(),
                    serde_json::json!({
                        "reachable": ok,
                        "auto_start": account.auto_start,
                    }),
                );
            }

            ProbeResult {
                ok: any_ok,
                latency_ms: None,
                error: if any_ok {
                    None
                } else {
                    Some("all Matrix accounts unreachable".to_owned())
                },
                details: Some(details),
            }
        })
    }
}

impl std::fmt::Debug for MatrixProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixProvider")
            .field("accounts", &self.accounts.keys().collect::<Vec<_>>())
            .field("default_account", &self.default_account)
            .field("circuit_breaker_threshold", &self.circuit_breaker_threshold)
            .field(
                "halted_health_check_interval",
                &self.halted_health_check_interval,
            )
            .finish()
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "sync_loop is a single cohesive state machine; the shared halted-state recovery               loop requires these parameters together — splitting would obscure the state transitions"
)]
async fn sync_loop(
    client: client::MatrixClient,
    tx: mpsc::Sender<InboundMessage>,
    interval: Duration,
    since: Arc<Mutex<Option<String>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
    user_id: Option<String>,
    cancel: CancellationToken,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
) {
    tracing::info!("Matrix sync started");
    let mut consecutive_failures = 0_u32;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                tracing::info!("cancellation received, stopping Matrix sync");
                return;
            }
            result = sync_once(&client, &tx, &since, user_id.as_deref()) => {
                match result {
                    Ok(()) => {
                        consecutive_failures = 0;
                        tokio::time::sleep(interval).await;
                    }
                    Err(error::Error::ReceiverDropped { .. }) => {
                        tracing::info!("receiver dropped, stopping Matrix sync");
                        return;
                    }
                    Err(e) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        tracing::warn!(
                            error = %e,
                            consecutive_failures,
                            "Matrix sync failed"
                        );
                        if consecutive_failures >= circuit_breaker_threshold {
                            tracing::error!(
                                consecutive_failures,
                                "Matrix sync halted after repeated failures; will probe for recovery"
                            );
                            // WHY: mirror Signal provider resilience — enter halted state with
                            // periodic health probes and backoff rather than exiting permanently;
                            // this avoids requiring a full process restart after a transient
                            // Matrix homeserver outage.
                            loop {
                                tokio::select! {
                                    biased;
                                    () = cancel.cancelled() => {
                                        tracing::info!("cancellation received, stopping Matrix sync");
                                        return;
                                    }
                                    () = tokio::time::sleep(halted_health_check_interval) => {}
                                }
                                if client.health().await {
                                    tracing::info!(
                                        previous_failures = consecutive_failures,
                                        "Matrix health check passed, resuming sync"
                                    );
                                    consecutive_failures = 0;
                                    break;
                                }
                                tracing::debug!("Matrix health check failed, remaining halted");
                            }
                        } else {
                            tokio::time::sleep(interval).await;
                        }
                    }
                }
            }
        }
    }
}

async fn sync_once(
    client: &client::MatrixClient,
    tx: &mpsc::Sender<InboundMessage>,
    since: &Arc<Mutex<Option<String>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
    own_user_id: Option<&str>,
) -> error::Result<()> {
    let since_token = { since.lock().await.clone() };
    let response = client.sync(since_token.as_deref()).await?;

    for (room_id, room) in &response.rooms.join {
        for event in &room.timeline.events {
            if let Some(message) = extract_message(room_id, event, own_user_id)
                && tx.send(message).await.is_err()
            {
                // WHY: the listener has shut down; returning an error lets
                // sync_loop exit instead of issuing another long-poll.
                return error::ReceiverDroppedSnafu.fail();
            }
        }
    }

    if let Some(next_batch) = response.next_batch {
        let mut guard = since.lock().await;
        *guard = Some(next_batch);
    }

    Ok(())
}

fn extract_message(
    room_id: &str,
    event: &MatrixEvent,
    own_user_id: Option<&str>,
) -> Option<InboundMessage> {
    if event.event_type != "m.room.message" {
        return None;
    }

    let sender = event.sender.as_deref()?;
    if own_user_id.is_some_and(|own| own == sender) {
        return None;
    }

    let text = event.content.body.as_deref()?;
    if text.is_empty() {
        return None;
    }

    let attachments = event
        .content
        .extra
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(|url| vec![url.to_owned()])
        .unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option::unwrap_or_default on Option<Vec<_>> chain; no Result involved

    Some(InboundMessage {
        channel: "matrix".to_owned(),
        sender: sender.to_owned(),
        sender_name: None,
        group_id: Some(room_id.to_owned()),
        text: text.to_owned(),
        timestamp: event.origin_server_ts.unwrap_or_else(|| {
            tracing::warn!("Matrix event has no timestamp, defaulting to 0");
            0
        }),
        attachments,
        raw: serde_json::to_value(event).ok(), // kanon:ignore RUST/silent-error-ok WHY: optional diagnostic field; serde failure is non-fatal and pre-existing WHY documents this
    })
}

pub(crate) fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => {
                encoded.push('%');
                encoded.push(hex_digit(byte >> 4));
                encoded.push(hex_digit(byte & 0x0f));
            }
        }
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + (nibble - 10)),
        // WHY: call sites pass `byte >> 4` and `byte & 0x0f`, which are always 0..=15.
        _ => unreachable!("nibble is always 0..=15"),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use organon::testing::install_crypto_provider;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[test]
    fn encode_matrix_room_id_as_path_segment() {
        assert_eq!(
            encode_path_segment("!room:example.org"),
            "%21room%3Aexample.org"
        );
        assert_eq!(
            encode_path_segment("#alias:example.org"),
            "%23alias%3Aexample.org"
        );
    }

    #[test]
    fn extract_matrix_room_message() {
        let event: MatrixEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "sender": "@alice:example.org",
            "event_id": "$event",
            "origin_server_ts": 100,
            "content": {
                "msgtype": "m.text",
                "body": "hello"
            }
        }))
        .expect("event");

        let msg = extract_message("!room:example.org", &event, Some("@bot:example.org"))
            .expect("message");
        assert_eq!(msg.channel, "matrix");
        assert_eq!(msg.sender, "@alice:example.org");
        assert_eq!(msg.group_id.as_deref(), Some("!room:example.org"));
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.timestamp, 100);
    }

    #[test]
    fn extract_matrix_message_skips_own_sender() {
        let event: MatrixEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "sender": "@bot:example.org",
            "origin_server_ts": 100,
            "content": {
                "msgtype": "m.text",
                "body": "echo"
            }
        }))
        .expect("event");

        assert!(extract_message("!room:example.org", &event, Some("@bot:example.org")).is_none());
    }

    #[tokio::test]
    async fn provider_send_uses_real_matrix_send() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(header("authorization", "Bearer token-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "event_id": "$event"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut provider = MatrixProvider::new();
        let client = client::MatrixClient::new(&server.uri(), "token-123").expect("client");
        provider.add_account(
            "primary".to_owned(),
            client,
            Some("@bot:example.org".to_owned()),
            true,
            None,
        );
        let result = provider
            .send(&ChannelSendParams {
                to: "!room:example.org".to_owned(),
                text: "hello".to_owned(),
                account_id: None,
                thread_id: None,
                attachments: None,
            })
            .await;

        assert!(result.sent, "send should succeed: {:?}", result.error);
    }

    #[tokio::test]
    async fn provider_listen_maps_sync_events() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .and(header("authorization", "Bearer token-123"))
            .and(query_param("timeout", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s1",
                "rooms": {
                    "join": {
                        "!room:example.org": {
                            "timeline": {
                                "events": [
                                    {
                                        "type": "m.room.message",
                                        "sender": "@alice:example.org",
                                        "event_id": "$event",
                                        "origin_server_ts": 123,
                                        "content": {
                                            "msgtype": "m.text",
                                            "body": "hello from Matrix"
                                        }
                                    }
                                ]
                            }
                        }
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut provider = MatrixProvider::from_config(&taxis::config::MessagingConfig {
            receive_timeout_secs: 1,
            circuit_breaker_threshold: 1,
            ..taxis::config::MessagingConfig::default()
        });
        let client = client::MatrixClient::with_timeouts(
            &server.uri(),
            "token-123",
            Duration::from_secs(1),
            Duration::from_millis(50),
        )
        .expect("client");
        provider.add_account(
            "primary".to_owned(),
            client,
            Some("@bot:example.org".to_owned()),
            true,
            None,
        );

        let token = CancellationToken::new();
        let (mut rx, mut handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());
        let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("message");

        assert_eq!(msg.channel, "matrix");
        assert_eq!(msg.sender, "@alice:example.org");
        assert_eq!(msg.group_id.as_deref(), Some("!room:example.org"));
        assert_eq!(msg.text, "hello from Matrix");

        token.cancel();
        drop(rx);
        while let Some(result) = tokio::time::timeout(Duration::from_secs(5), handles.join_next())
            .await
            .ok()
            .flatten()
        {
            let _ = result;
        }
    }
}
