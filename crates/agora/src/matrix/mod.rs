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
use snafu::ResultExt as _;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, instrument};

use koina::redact::{opaque_channel_id, redact_channel_id};

use crate::connection_utils::reconnect_delay;
use crate::cursor::CursorStore;
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
    /// Matrix message type, e.g. `m.text`. Read by `extract_message` to
    /// decide which message kinds become inbound messages: `m.text` and
    /// `m.emote` are accepted, everything else (including an absent
    /// msgtype) is dropped.
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

/// One account's cursor persistence wiring, resolved at listen time (#7104).
struct AccountCursor {
    store: Arc<dyn CursorStore>,
    account_id: String,
    /// Cursor loaded before the sync loop starts; it seeds `since` ahead of
    /// the first request and takes precedence over the configured
    /// `initial_since` — config seeds a brand-new account, while the store
    /// knows where this instance actually stopped.
    persisted: Option<String>,
}

impl AccountCursor {
    fn save(&self, cursor: &str) -> std::io::Result<()> {
        self.store.save("matrix", &self.account_id, cursor)
    }
}

/// Matrix channel provider implementing `ChannelProvider`.
pub struct MatrixProvider {
    accounts: HashMap<String, MatrixAccount>,
    default_account: Option<String>,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
    /// Opt-in, bounded raw-event retention on inbound messages. Off by
    /// default (#5198): see `taxis::config::RawPayloadPolicy`.
    raw_payload: taxis::config::RawPayloadPolicy,
    /// Persistent store for `/sync` resumption cursors, when the runtime
    /// wires one in (#7104). Without it, cursors live only in memory and a
    /// restart replays the batch preceding it.
    cursor_store: Option<Arc<dyn CursorStore>>,
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
            raw_payload: taxis::config::RawPayloadPolicy::default(),
            cursor_store: None,
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
            raw_payload: config.raw_payload.clone(),
            cursor_store: None,
        }
    }

    /// Wire a persistent store for `/sync` resumption cursors (#7104).
    ///
    /// With a store attached, each auto-started account seeds its `since`
    /// token from the last persisted cursor — taking precedence over the
    /// configured `initial_since` — and checkpoints every accepted batch, so
    /// a restart resumes after that batch instead of replaying it.
    pub fn set_cursor_store(&mut self, store: Arc<dyn CursorStore>) {
        self.cursor_store = Some(store);
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
                tracing::info!(
                    account = %redact_channel_id(account_id),
                    "skipping Matrix sync loop (auto_start=false)"
                );
                continue;
            }

            let cursor = match self.load_account_cursor(account_id) {
                Ok(cursor) => cursor,
                Err(error) => {
                    // WHY fail closed (#7104): an unreadable cursor means the
                    // resumption point is unknown, and syncing anyway would
                    // replay already-accepted batches — the exact defect the
                    // cursor exists to prevent. Corruption must not degrade
                    // into a silent fresh start.
                    tracing::error!(
                        account = %redact_channel_id(account_id),
                        error = %error,
                        "failed to load the persisted Matrix sync cursor; not starting this account's sync loop"
                    );
                    continue;
                }
            };

            let tx = tx.clone();
            let token = cancel.clone();
            let client = account.client.clone();
            let since = Arc::clone(&account.since);
            let user_id = account.user_id.clone();
            let span = tracing::info_span!("matrix_sync", account = %redact_channel_id(account_id));

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
                    self.raw_payload.clone(),
                    cursor,
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

    /// Resolve one account's cursor wiring: `Ok(None)` without a store,
    /// `Ok(Some(_))` with a store and a readable (possibly absent) cursor,
    /// `Err` when a present cursor cannot be read.
    fn load_account_cursor(&self, account_id: &str) -> std::io::Result<Option<AccountCursor>> {
        let Some(store) = self.cursor_store.as_ref() else {
            return Ok(None);
        };
        let persisted = store.load("matrix", account_id)?;
        Ok(Some(AccountCursor {
            store: Arc::clone(store),
            account_id: account_id.to_owned(),
            persisted,
        }))
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
                // WHY(#5198): `account_id` is a Matrix user ID; probe
                // details are Serialize and reach diagnostic/health
                // surfaces, so the raw value never leaves here.
                //
                // WHY opaque (#7101): a map key is a correlation key, and
                // suffix-redacted keys alias -- two accounts sharing their
                // last four characters would silently overwrite each
                // other's detail -- so the key is the collision-resistant
                // handle and the lossy suffix form rides inside the detail
                // as `display`.
                details.insert(
                    opaque_channel_id("matrix-account", account_id),
                    serde_json::json!({
                        "reachable": ok,
                        "auto_start": account.auto_start,
                        "display": redact_channel_id(account_id),
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
            .field("raw_payload", &self.raw_payload)
            .field("cursor_store", &self.cursor_store.is_some())
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
    raw_payload: taxis::config::RawPayloadPolicy,
    cursor: Option<AccountCursor>,
) {
    tracing::info!("Matrix sync started");
    if let Some(persisted) = cursor.as_ref().and_then(|c| c.persisted.clone()) {
        // WHY(#7104): resume after the last batch this instance accepted
        // rather than from `initial_since` (or the server's default window),
        // which would replay it.
        *since.lock().await = Some(persisted);
    }
    let mut consecutive_failures = 0_u32;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                tracing::info!("cancellation received, stopping Matrix sync");
                return;
            }
            result = sync_once(&client, &tx, &since, user_id.as_deref(), &raw_payload, cursor.as_ref()) => {
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
                            let delay = reconnect_delay(consecutive_failures);
                            tracing::debug!(
                                consecutive_failures,
                                backoff_secs = delay.as_secs(),
                                "Matrix sync backing off after error"
                            );
                            tokio::time::sleep(delay).await;
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
    raw_payload: &taxis::config::RawPayloadPolicy,
    cursor: Option<&AccountCursor>,
) -> error::Result<()> {
    let since_token = { since.lock().await.clone() };
    let response = client.sync(since_token.as_deref()).await?;

    // WHY: advance the cursor immediately after the HTTP response returns,
    // before any tx.send await points. If cancellation interrupts the sends
    // below, we would rather lose the in-flight events than replay the whole
    // batch on the next sync.
    if let Some(next_batch) = response.next_batch {
        // WHY(#7104): checkpoint durably before the in-memory token advances
        // and before any event is forwarded. A batch that cannot be
        // checkpointed is not accepted: the error propagates, `since` keeps
        // its previous value, and the retry re-fetches the same window, so
        // nothing is forwarded twice within a run or lost across a restart.
        // Skipped when the token is unchanged (an idle long-poll) to avoid a
        // redundant durable write per poll.
        if since_token.as_deref() != Some(next_batch.as_str())
            && let Some(cursor) = cursor
        {
            cursor.save(&next_batch).context(error::CursorSnafu)?;
        }
        let mut guard = since.lock().await;
        *guard = Some(next_batch);
    }

    for (room_id, room) in &response.rooms.join {
        for event in &room.timeline.events {
            if let Some(message) = extract_message(room_id, event, own_user_id, raw_payload)
                && tx.send(message).await.is_err()
            {
                // WHY: the listener has shut down; returning an error lets
                // sync_loop exit instead of issuing another long-poll.
                return error::ReceiverDroppedSnafu.fail();
            }
        }
    }

    Ok(())
}

fn extract_message(
    room_id: &str,
    event: &MatrixEvent,
    own_user_id: Option<&str>,
    raw_payload: &taxis::config::RawPayloadPolicy,
) -> Option<InboundMessage> {
    if event.event_type != "m.room.message" {
        return None;
    }

    let sender = event.sender.as_deref()?;
    if own_user_id.is_some_and(|own| own == sender) {
        return None;
    }

    // WHY(#7082): msgtype decides whether an event is turn-worthy text.
    // `m.notice` is reserved by the Matrix spec for automated clients so
    // that other automated clients do not respond to it -- forwarding one
    // risks a bot loop. Media and every other non-text kind are dropped
    // because this provider declares `media: false` and their `body` is a
    // filename or caption, not a message. A missing msgtype violates the
    // spec for `m.room.message` and is treated as non-text, not as
    // implicit `m.text`.
    let annotate_emote = match event.content.msgtype.as_deref() {
        Some("m.text") => false,
        Some("m.emote") => true,
        other => {
            tracing::debug!(
                msgtype = other.unwrap_or("<absent>"),
                "dropping Matrix message with non-text msgtype"
            );
            return None;
        }
    };

    let body = event.content.body.as_deref()?;
    if body.is_empty() {
        return None;
    }
    // NOTE: `* ` is the conventional client rendering for emotes; it keeps
    // the action framing ("* waves" vs "waves") through normalization,
    // since downstream sees only `text` plus a separate `sender`.
    let text = if annotate_emote {
        format!("* {body}")
    } else {
        body.to_owned()
    };

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
        message_id: event.event_id.clone(),
        text,
        timestamp: event.origin_server_ts.unwrap_or_else(|| {
            tracing::warn!("Matrix event has no timestamp, defaulting to 0");
            0
        }),
        attachments,
        raw: crate::types::capture_raw_payload(raw_payload, event),
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
    use std::sync::Arc;
    use std::time::Duration;

    use organon::testing::install_crypto_provider;
    use tokio::sync::{Mutex, mpsc};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// In-memory `CursorStore` double: persistence-free, but shared across
    /// provider instances via `Arc` to model state surviving a restart.
    #[derive(Default)]
    struct MemoryCursorStore {
        cursors: std::sync::RwLock<HashMap<(String, String), String>>,
    }

    impl crate::cursor::CursorStore for MemoryCursorStore {
        fn load(&self, channel: &str, account: &str) -> std::io::Result<Option<String>> {
            Ok(self
                .cursors
                .read()
                .expect("cursor lock")
                .get(&(channel.to_owned(), account.to_owned()))
                .cloned())
        }

        fn save(&self, channel: &str, account: &str, cursor: &str) -> std::io::Result<()> {
            self.cursors
                .write()
                .expect("cursor lock")
                .insert((channel.to_owned(), account.to_owned()), cursor.to_owned());
            Ok(())
        }
    }

    /// `CursorStore` double whose loads always fail, modelling a corrupt or
    /// unreadable persisted cursor.
    struct FailingCursorStore;

    impl crate::cursor::CursorStore for FailingCursorStore {
        fn load(&self, _channel: &str, _account: &str) -> std::io::Result<Option<String>> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "cursor record is corrupt",
            ))
        }

        fn save(&self, _channel: &str, _account: &str, _cursor: &str) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sync_body(next_batch: &str, message: &str) -> serde_json::Value {
        serde_json::json!({
            "next_batch": next_batch,
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
                                        "body": message
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        })
    }

    fn provider_with_store<S: crate::cursor::CursorStore + 'static>(
        server_uri: &str,
        store: Arc<S>,
    ) -> MatrixProvider {
        let mut provider = MatrixProvider::from_config(&taxis::config::MessagingConfig {
            receive_timeout_secs: 1,
            circuit_breaker_threshold: 1,
            ..taxis::config::MessagingConfig::default()
        });
        provider.set_cursor_store(store);
        let client = client::MatrixClient::with_timeouts(
            server_uri,
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
        provider
    }

    async fn recv_one(
        provider: &MatrixProvider,
    ) -> (
        InboundMessage,
        CancellationToken,
        mpsc::Receiver<InboundMessage>,
        JoinSet<()>,
    ) {
        let token = CancellationToken::new();
        let (mut rx, handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());
        let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("message");
        (msg, token, rx, handles)
    }

    async fn shut_down(
        token: CancellationToken,
        rx: mpsc::Receiver<InboundMessage>,
        mut handles: JoinSet<()>,
    ) {
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

    #[test]
    fn matrix_sync_error_backoff_grows_with_failures() {
        let mut previous = reconnect_delay(0);
        for failures in 1..=5 {
            let delay = reconnect_delay(failures);
            assert!(
                delay > previous,
                "backoff should grow at failure count {failures}"
            );
            previous = delay;
        }
        assert_eq!(reconnect_delay(6), Duration::from_mins(1));
        assert_eq!(reconnect_delay(100), Duration::from_mins(1));
    }

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

        let policy = taxis::config::RawPayloadPolicy::default();
        let msg = extract_message(
            "!room:example.org",
            &event,
            Some("@bot:example.org"),
            &policy,
        )
        .expect("message");
        assert_eq!(msg.channel, "matrix");
        assert_eq!(msg.sender, "@alice:example.org");
        assert_eq!(msg.group_id.as_deref(), Some("!room:example.org"));
        assert_eq!(msg.message_id.as_deref(), Some("$event"));
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.timestamp, 100);
        assert_eq!(
            msg.raw, None,
            "raw payload capture is opt-in and off by default"
        );
    }

    #[test]
    fn extract_matrix_message_captures_redacted_raw_when_enabled() {
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

        let policy = taxis::config::RawPayloadPolicy {
            capture: true,
            max_bytes: 4096,
        };
        let msg = extract_message(
            "!room:example.org",
            &event,
            Some("@bot:example.org"),
            &policy,
        )
        .expect("message");
        let raw = msg.raw.expect("raw captured when policy enables it");
        assert!(!raw.to_string().contains("@alice:example.org"));
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

        let policy = taxis::config::RawPayloadPolicy::default();
        assert!(
            extract_message(
                "!room:example.org",
                &event,
                Some("@bot:example.org"),
                &policy
            )
            .is_none()
        );
    }

    #[test]
    fn extract_matrix_message_drops_notice() {
        // WHY(#7082): the Matrix spec reserves m.notice for automated
        // clients precisely so other automated clients do not respond to
        // it; forwarding one into the turn pipeline risks a bot loop.
        let event: MatrixEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "sender": "@alice:example.org",
            "event_id": "$notice",
            "origin_server_ts": 100,
            "content": {
                "msgtype": "m.notice",
                "body": "automated status report"
            }
        }))
        .expect("event");

        let policy = taxis::config::RawPayloadPolicy::default();
        assert!(
            extract_message(
                "!room:example.org",
                &event,
                Some("@bot:example.org"),
                &policy
            )
            .is_none(),
            "m.notice must not produce an inbound message"
        );
    }

    #[test]
    fn extract_matrix_message_drops_non_text_msgtypes() {
        let policy = taxis::config::RawPayloadPolicy::default();
        for msgtype in [
            "m.image",
            "m.file",
            "m.audio",
            "m.video",
            "m.location",
            "m.server_notice",
            "m.key.verification.request",
            "com.example.custom",
        ] {
            let event: MatrixEvent = serde_json::from_value(serde_json::json!({
                "type": "m.room.message",
                "sender": "@alice:example.org",
                "event_id": "$media",
                "origin_server_ts": 100,
                "content": {
                    "msgtype": msgtype,
                    "body": "photo.jpg",
                    "url": "mxc://example.org/abc123"
                }
            }))
            .expect("event");

            assert!(
                extract_message(
                    "!room:example.org",
                    &event,
                    Some("@bot:example.org"),
                    &policy
                )
                .is_none(),
                "{msgtype} must not produce an inbound message"
            );
        }
    }

    #[test]
    fn extract_matrix_message_drops_missing_msgtype() {
        // WHY(#7082): the spec requires msgtype on every m.room.message;
        // an event without one is malformed, not implicitly m.text.
        let event: MatrixEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "sender": "@alice:example.org",
            "event_id": "$malformed",
            "origin_server_ts": 100,
            "content": {
                "body": "hello"
            }
        }))
        .expect("event");

        let policy = taxis::config::RawPayloadPolicy::default();
        assert!(
            extract_message(
                "!room:example.org",
                &event,
                Some("@bot:example.org"),
                &policy
            )
            .is_none(),
            "an event without a msgtype must not produce an inbound message"
        );
    }

    #[test]
    fn extract_matrix_message_annotates_emote() {
        let event: MatrixEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "sender": "@alice:example.org",
            "event_id": "$emote",
            "origin_server_ts": 100,
            "content": {
                "msgtype": "m.emote",
                "body": "waves"
            }
        }))
        .expect("event");

        let policy = taxis::config::RawPayloadPolicy::default();
        let msg = extract_message(
            "!room:example.org",
            &event,
            Some("@bot:example.org"),
            &policy,
        )
        .expect("emote is user-authored text and must be accepted");
        assert_eq!(
            msg.text, "* waves",
            "emote text must carry the action annotation"
        );
    }

    #[tokio::test]
    async fn provider_listen_drops_non_text_events() {
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
                                        "event_id": "$notice",
                                        "origin_server_ts": 121,
                                        "content": {
                                            "msgtype": "m.notice",
                                            "body": "automated status report"
                                        }
                                    },
                                    {
                                        "type": "m.room.message",
                                        "sender": "@alice:example.org",
                                        "event_id": "$image",
                                        "origin_server_ts": 122,
                                        "content": {
                                            "msgtype": "m.image",
                                            "body": "photo.jpg",
                                            "url": "mxc://example.org/abc123"
                                        }
                                    },
                                    // NOTE: the text event comes last so receiving it
                                    // proves the earlier non-text events were dropped,
                                    // not merely still in flight (forwarding is in
                                    // timeline order).
                                    {
                                        "type": "m.room.message",
                                        "sender": "@alice:example.org",
                                        "event_id": "$text",
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

        assert_eq!(
            msg.message_id.as_deref(),
            Some("$text"),
            "only the m.text event may produce a turn"
        );
        assert!(
            rx.try_recv().is_err(),
            "the m.notice and m.image events must not produce turns"
        );

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
                sender_id: None,
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

    #[tokio::test]
    async fn sync_once_advances_since_before_sends_complete() {
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
                                        "event_id": "$event1",
                                        "origin_server_ts": 123,
                                        "content": {
                                            "msgtype": "m.text",
                                            "body": "hello from Matrix"
                                        }
                                    },
                                    {
                                        "type": "m.room.message",
                                        "sender": "@alice:example.org",
                                        "event_id": "$event2",
                                        "origin_server_ts": 124,
                                        "content": {
                                            "msgtype": "m.text",
                                            "body": "second from Matrix"
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

        let client = client::MatrixClient::with_timeouts(
            &server.uri(),
            "token-123",
            Duration::from_secs(1),
            Duration::from_millis(50),
        )
        .expect("client");
        let since: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        // WHY: capacity-1 bounded channel with two events and a non-polling receiver:
        // the first tx.send fills the buffer, the second tx.send suspends (buffer full),
        // and handle.abort() cancels the task at that await point.
        let (tx, _rx) = mpsc::channel::<InboundMessage>(1);

        let client_ref = client.clone();
        let since_ref = Arc::clone(&since);
        let tx_ref = tx.clone();
        let own_user_id = "@bot:example.org".to_owned();
        let raw_payload = taxis::config::RawPayloadPolicy::default();
        let handle = tokio::spawn(async move {
            sync_once(
                &client_ref,
                &tx_ref,
                &since_ref,
                Some(&own_user_id),
                &raw_payload,
                None,
            )
            .await
        });

        // WHY: wait until sync_once has advanced the cursor; that happens
        // synchronously before the first tx.send await point, so reaching it
        // means the task is suspended at (or past) the send where cancellation
        // would previously have left a stale token.
        tokio::time::timeout(Duration::from_secs(5), async {
            while since.lock().await.is_none() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("sync_once should advance since before sending");

        handle.abort();
        assert!(
            handle.await.expect_err("aborted task").is_cancelled(),
            "task should have been cancelled while tx.send was suspended"
        );

        let guard = since.lock().await;
        assert_eq!(
            guard.as_deref(),
            Some("s1"),
            "next_batch should be persisted even when the future is cancelled mid-send"
        );
    }

    #[tokio::test]
    async fn listen_resumes_from_persisted_cursor_after_restart() {
        install_crypto_provider();
        let store = Arc::new(MemoryCursorStore::default());

        // First run: a fresh account with no persisted cursor accepts the
        // batch the homeserver hands out with `next_batch: s1`.
        let first_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .and(header("authorization", "Bearer token-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sync_body("s1", "first")))
            .mount(&first_server)
            .await;

        let provider = provider_with_store(&first_server.uri(), Arc::clone(&store));
        let (msg, token, rx, handles) = recv_one(&provider).await;
        assert_eq!(msg.text, "first");
        shut_down(token, rx, handles).await;

        assert_eq!(
            store.load("matrix", "primary").expect("load").as_deref(),
            Some("s1"),
            "the accepted batch's next_batch token must be checkpointed to the store"
        );

        // Restart: a new provider instance sharing the store must sync with
        // `since=s1`. The second server only answers that request — a
        // cursorless restart would send no `since`, match nothing, and
        // receive no message.
        let second_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .and(header("authorization", "Bearer token-123"))
            .and(query_param("since", "s1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sync_body("s2", "second")))
            .mount(&second_server)
            .await;

        let provider = provider_with_store(&second_server.uri(), Arc::clone(&store));
        let (msg, token, rx, handles) = recv_one(&provider).await;
        assert_eq!(
            msg.text, "second",
            "a restart must resume after the accepted batch, not replay it"
        );
        shut_down(token, rx, handles).await;
    }

    #[tokio::test]
    async fn listen_does_not_start_sync_when_cursor_load_fails() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sync_body("s1", "replayed")))
            .expect(0)
            .mount(&server)
            .await;

        let provider = provider_with_store(&server.uri(), Arc::new(FailingCursorStore));
        let token = CancellationToken::new();
        let (mut rx, handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());

        // WHY fail closed: an unreadable cursor means the resumption point is
        // unknown; syncing anyway would replay already-accepted batches, the
        // exact defect the cursor exists to prevent (#7104).
        assert!(
            handles.is_empty(),
            "no sync loop may start for an account whose cursor cannot be read"
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("recv resolves immediately once all senders are dropped")
                .is_none(),
            "no message may be forwarded for an account whose cursor cannot be read"
        );
    }
}
