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

use serde::Deserialize;
use snafu::ResultExt;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, instrument};

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
struct MatrixSyncResponse {
    /// Batch token to pass as `since` on the next sync.
    next_batch: String,
    /// Joined rooms returned by sync.
    rooms: MatrixRooms,
}

/// Matrix `/sync` rooms container.
#[derive(Default, Deserialize)]
struct MatrixRooms {
    /// Joined rooms keyed by room ID.
    #[serde(default)]
    join: HashMap<String, MatrixJoinedRoom>,
}

/// Matrix joined-room sync payload.
#[derive(Default, Deserialize)]
struct MatrixJoinedRoom {
    /// Timeline events returned for the room.
    #[serde(default)]
    timeline: MatrixTimeline,
}

/// Matrix room timeline payload.
#[derive(Default, Deserialize)]
struct MatrixTimeline {
    /// Timeline events.
    #[serde(default)]
    events: Vec<MatrixEvent>,
    /// Whether earlier timeline events were omitted from this response.
    #[serde(default)]
    limited: bool,
}

/// Matrix event subset used by message extraction.
#[derive(Default, Deserialize)]
struct MatrixEvent {
    /// Event type, e.g. `m.room.message`.
    #[serde(rename = "type")]
    event_type: String,
    /// Matrix user ID of the sender.
    sender: Option<String>,
    /// Event ID.
    event_id: Option<String>,
    /// Server timestamp in milliseconds.
    origin_server_ts: Option<u64>,
    /// Event content.
    #[serde(default)]
    content: MatrixEventContent,
    /// Raw unsigned metadata.
    #[serde(default)]
    unsigned: Option<serde_json::Value>,
    /// Additional top-level event fields retained only for explicit raw-payload opt-in.
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

/// Matrix room-message content subset.
#[derive(Default, Deserialize)]
struct MatrixEventContent {
    /// Matrix message type, e.g. `m.text`.
    msgtype: Option<String>,
    /// Plain-text body.
    body: Option<String>,
    /// Additional content fields retained for attachments and raw diagnostics.
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Clone)]
struct MatrixAccount {
    client: client::MatrixClient,
    user_id: Option<String>,
    auto_start: bool,
    since: Arc<Mutex<Option<String>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
    ingress_state: Arc<Mutex<MatrixIngressState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: probe and the account task share one async state cell
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MatrixIngressState {
    Disabled,
    Starting,
    Active,
    BackingOff,
    Halted,
    CursorUnavailable,
    TimelineGap,
    ProtocolError,
    Stopped,
}

impl MatrixIngressState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Starting => "starting",
            Self::Active => "active",
            Self::BackingOff => "backing_off",
            Self::Halted => "halted",
            Self::CursorUnavailable => "cursor_unavailable",
            Self::TimelineGap => "timeline_gap",
            Self::ProtocolError => "protocol_error",
            Self::Stopped => "stopped",
        }
    }

    const fn is_healthy(self) -> bool {
        matches!(self, Self::Disabled | Self::Active)
    }
}

/// Matrix channel provider implementing `ChannelProvider`.
pub struct MatrixProvider {
    accounts: HashMap<String, MatrixAccount>,
    default_account: Option<String>,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
    /// Durable `/sync` cursor store shared by all accounts. When set, each
    /// account resumes from its persisted cursor on startup and checkpoints
    /// after every sync batch.
    cursor_store: Option<Arc<dyn CursorStore>>,
    /// Whether raw Matrix events are attached to inbound messages
    /// (`MessagingConfig::retain_raw_payloads`); off by default because the
    /// raw event carries personal identifiers.
    retain_raw_payloads: bool,
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
            cursor_store: None,
            retain_raw_payloads: false,
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
            cursor_store: None,
            retain_raw_payloads: config.retain_raw_payloads,
        }
    }

    /// Attach a durable cursor store. A persisted cursor overrides the
    /// account's configured `initial_since` on startup.
    #[must_use]
    pub fn with_cursor_store(mut self, store: Arc<dyn CursorStore>) -> Self {
        self.cursor_store = Some(store);
        self
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
                ingress_state: Arc::new(Mutex::new(if auto_start {
                    MatrixIngressState::Starting
                } else {
                    MatrixIngressState::Disabled
                })), // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: probe and the account task share one async state cell
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
                    account = %crate::redact::identifier(account_id),
                    "skipping Matrix sync loop (auto_start=false)"
                );
                continue;
            }

            let tx = tx.clone();
            let token = cancel.clone();
            let client = account.client.clone();
            let since = Arc::clone(&account.since);
            let user_id = account.user_id.clone();
            let account_label = account_id.clone();
            let cursor_store = self.cursor_store.clone();
            let ingress_state = Arc::clone(&account.ingress_state);
            let circuit_breaker_threshold = self.circuit_breaker_threshold;
            let halted_health_check_interval = self.halted_health_check_interval;
            let retain_raw_payloads = self.retain_raw_payloads;
            let span = tracing::info_span!(
                "matrix_sync",
                account = %crate::redact::identifier(account_id)
            );

            handles.spawn(
                async move {
                    let _subscription = crate::metrics::ActiveSubscriptionGuard::new();
                    sync_loop(
                        client,
                        account_label,
                        tx,
                        interval,
                        since,
                        user_id,
                        token,
                        circuit_breaker_threshold,
                        halted_health_check_interval,
                        cursor_store,
                        retain_raw_payloads,
                        ingress_state,
                    )
                    .await;
                }
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
                .send_text(
                    &params.to,
                    &params.text,
                    params.thread_id.as_deref(),
                    params.idempotency_key.as_deref(),
                )
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
            let mut all_ok = true;
            let mut accounts = self.accounts.iter().collect::<Vec<_>>();
            accounts.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            for (index, (account_id, account)) in accounts.into_iter().enumerate() {
                let reachable = account.client.health().await;
                let ingress_state = *account.ingress_state.lock().await;
                let ok = reachable && ingress_state.is_healthy();
                all_ok &= ok;
                details.insert(
                    format!("account_{index}"),
                    serde_json::json!({
                        "account_ref": crate::redact::identifier(account_id),
                        "reachable": reachable,
                        "auto_start": account.auto_start,
                        "ingress_state": ingress_state.as_str(),
                        "ingress_ok": ingress_state.is_healthy(),
                    }),
                );
            }

            ProbeResult {
                ok: all_ok,
                latency_ms: None,
                error: if all_ok {
                    None
                } else {
                    Some("one or more Matrix accounts unreachable or ingress-unhealthy".to_owned())
                },
                details: Some(details),
            }
        })
    }
}

impl std::fmt::Debug for MatrixProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixProvider")
            .field("accounts_count", &self.accounts.len())
            .field("default_account_set", &self.default_account.is_some())
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
    reason = "sync_loop is one account state machine; splitting its shared state would obscure transition ownership"
)]
#[expect(
    clippy::too_many_lines,
    reason = "cursor recovery, polling, gap halt, and transport recovery are transitions in one account state machine"
)]
async fn sync_loop(
    client: client::MatrixClient,
    account_id: String,
    tx: mpsc::Sender<InboundMessage>,
    interval: Duration,
    since: Arc<Mutex<Option<String>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
    user_id: Option<String>,
    cancel: CancellationToken,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
    cursor_store: Option<Arc<dyn CursorStore>>,
    retain_raw_payloads: bool,
    ingress_state: Arc<Mutex<MatrixIngressState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: probe and the account task share one async state cell
) {
    tracing::info!("Matrix sync started");

    // WHY: cursor failure must never look like cursor absence. Starting an
    // unscoped sync after a failed load would silently widen the replay set.
    if let Some(store) = &cursor_store {
        let mut load_failures = 0_u32;
        loop {
            match store
                .load("matrix", &account_id)
                .context(error::CursorSnafu { operation: "load" })
            {
                Ok(Some(saved)) => {
                    tracing::info!("resuming Matrix sync from persisted cursor");
                    *since.lock().await = Some(saved);
                    break;
                }
                Ok(None) => break,
                Err(e) => {
                    load_failures = load_failures.saturating_add(1);
                    *ingress_state.lock().await = MatrixIngressState::CursorUnavailable;
                    tracing::warn!(
                        error = %e,
                        load_failures,
                        "Matrix cursor load failed; sync remains stopped"
                    );
                    if wait_or_stop(reconnect_delay(load_failures), &cancel, &tx).await {
                        *ingress_state.lock().await = MatrixIngressState::Stopped;
                        return;
                    }
                }
            }
        }
    }

    let mut consecutive_failures = 0_u32;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                tracing::info!("cancellation received, stopping Matrix sync");
                *ingress_state.lock().await = MatrixIngressState::Stopped;
                return;
            }
            () = tx.closed() => {
                tracing::info!("receiver dropped, stopping Matrix sync");
                *ingress_state.lock().await = MatrixIngressState::Stopped;
                return;
            }
            result = sync_once(&client, &tx, &since, user_id.as_deref(), &account_id, cursor_store.as_ref(), retain_raw_payloads) => {
                match result {
                    Ok(()) => {
                        consecutive_failures = 0;
                        *ingress_state.lock().await = MatrixIngressState::Active;
                        if wait_or_stop(interval, &cancel, &tx).await {
                            *ingress_state.lock().await = MatrixIngressState::Stopped;
                            return;
                        }
                    }
                    Err(error::Error::ReceiverDropped { .. }) => {
                        tracing::info!("receiver dropped, stopping Matrix sync");
                        *ingress_state.lock().await = MatrixIngressState::Stopped;
                        return;
                    }
                    Err(e @ error::Error::TimelineGap { .. }) => {
                        *ingress_state.lock().await = MatrixIngressState::TimelineGap;
                        tracing::error!(
                            error = %e,
                            "Matrix sync halted before checkpoint because timeline history is incomplete"
                        );
                        wait_until_stopped(&cancel, &tx).await;
                        *ingress_state.lock().await = MatrixIngressState::Stopped;
                        return;
                    }
                    Err(e @ error::Error::Protocol { .. }) => {
                        *ingress_state.lock().await = MatrixIngressState::ProtocolError;
                        tracing::error!(error = %e, "Matrix sync halted on invalid protocol response");
                        wait_until_stopped(&cancel, &tx).await;
                        *ingress_state.lock().await = MatrixIngressState::Stopped;
                        return;
                    }
                    Err(e @ error::Error::Json { .. }) => {
                        *ingress_state.lock().await = MatrixIngressState::ProtocolError;
                        tracing::error!(error = %e, "Matrix sync halted on malformed JSON response");
                        wait_until_stopped(&cancel, &tx).await;
                        *ingress_state.lock().await = MatrixIngressState::Stopped;
                        return;
                    }
                    Err(e @ error::Error::Cursor { .. }) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        *ingress_state.lock().await = MatrixIngressState::CursorUnavailable;
                        tracing::warn!(
                            error = %e,
                            consecutive_failures,
                            "Matrix cursor checkpoint failed; batch remains unaccepted"
                        );
                        if wait_or_stop(
                            reconnect_delay(consecutive_failures),
                            &cancel,
                            &tx,
                        )
                        .await
                        {
                            *ingress_state.lock().await = MatrixIngressState::Stopped;
                            return;
                        }
                    }
                    Err(e) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        *ingress_state.lock().await = MatrixIngressState::BackingOff;
                        tracing::warn!(
                            error = %e,
                            consecutive_failures,
                            "Matrix sync failed"
                        );
                        if consecutive_failures >= circuit_breaker_threshold {
                            *ingress_state.lock().await = MatrixIngressState::Halted;
                            tracing::error!(
                                consecutive_failures,
                                "Matrix sync halted after repeated failures; will probe for recovery"
                            );
                            // WHY: mirror Signal provider resilience — enter halted state with
                            // periodic health probes and backoff rather than exiting permanently;
                            // this avoids requiring a full process restart after a transient
                            // Matrix homeserver outage.
                            loop {
                                if wait_or_stop(halted_health_check_interval, &cancel, &tx).await {
                                    *ingress_state.lock().await = MatrixIngressState::Stopped;
                                    return;
                                }
                                let reachable = tokio::select! {
                                    biased;
                                    () = cancel.cancelled() => {
                                        *ingress_state.lock().await = MatrixIngressState::Stopped;
                                        return;
                                    }
                                    () = tx.closed() => {
                                        *ingress_state.lock().await = MatrixIngressState::Stopped;
                                        return;
                                    }
                                    reachable = client.health() => reachable,
                                };
                                if reachable {
                                    tracing::info!(
                                        previous_failures = consecutive_failures,
                                        "Matrix health check passed, resuming sync"
                                    );
                                    consecutive_failures = 0;
                                    *ingress_state.lock().await = MatrixIngressState::Starting;
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
                            if wait_or_stop(delay, &cancel, &tx).await {
                                *ingress_state.lock().await = MatrixIngressState::Stopped;
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn wait_or_stop(
    delay: Duration,
    cancel: &CancellationToken,
    tx: &mpsc::Sender<InboundMessage>,
) -> bool {
    tokio::select! {
        biased;
        () = cancel.cancelled() => true,
        () = tx.closed() => true,
        () = tokio::time::sleep(delay) => false,
    }
}

async fn wait_until_stopped(cancel: &CancellationToken, tx: &mpsc::Sender<InboundMessage>) {
    tokio::select! {
        biased;
        () = cancel.cancelled() => {},
        () = tx.closed() => {},
    }
}

async fn sync_once(
    client: &client::MatrixClient,
    tx: &mpsc::Sender<InboundMessage>,
    since: &Arc<Mutex<Option<String>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
    own_user_id: Option<&str>,
    account_id: &str,
    cursor_store: Option<&Arc<dyn CursorStore>>,
    retain_raw_payloads: bool,
) -> error::Result<()> {
    let since_token = { since.lock().await.clone() };
    let response = client.sync(since_token.as_deref()).await?;

    let limited_rooms = response
        .rooms
        .join
        .values()
        .filter(|room| room.timeline.limited)
        .count();
    if limited_rooms > 0 {
        return error::TimelineGapSnafu { limited_rooms }.fail();
    }

    // WHY: advance the cursor immediately after the HTTP response returns,
    // before any tx.send await points. If cancellation interrupts the sends
    // below, we would rather lose the in-flight events than replay the whole
    // accepted batch on the next sync. This is an explicit at-most-once
    // boundary: loss is preferred over replay, and this does not claim the
    // downstream turn was processed or durable.
    let next_batch = response.next_batch;
    if let Some(store) = cursor_store {
        store
            .save("matrix", account_id, &next_batch)
            .context(error::CursorSnafu { operation: "save" })?;
        crate::metrics::record_cursor_checkpoint("matrix");
    }
    let mut guard = since.lock().await;
    *guard = Some(next_batch);
    drop(guard);

    for (room_id, room) in &response.rooms.join {
        for event in &room.timeline.events {
            if let Some(message) =
                extract_message(room_id, event, own_user_id, account_id, retain_raw_payloads)
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
    account_id: &str,
    retain_raw: bool,
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
        account_id: Some(account_id.to_owned()),
        message_id: event.event_id.clone(),
        text: text.to_owned(),
        timestamp: event.origin_server_ts.unwrap_or_else(|| {
            tracing::warn!("Matrix event has no timestamp, defaulting to 0");
            0
        }),
        attachments,
        raw: if retain_raw {
            Some(retained_raw_event(event))
        } else {
            None
        },
    })
}

fn retained_raw_event(event: &MatrixEvent) -> serde_json::Value {
    let mut content = event.content.extra.clone();
    if let Some(msgtype) = &event.content.msgtype {
        content.insert("msgtype".to_owned(), serde_json::json!(msgtype));
    }
    if let Some(body) = &event.content.body {
        content.insert("body".to_owned(), serde_json::json!(body));
    }

    let mut raw = event.extra.clone();
    raw.insert(
        "type".to_owned(),
        serde_json::json!(event.event_type.as_str()),
    );
    raw.insert(
        "sender".to_owned(),
        serde_json::json!(event.sender.as_deref()),
    );
    raw.insert(
        "event_id".to_owned(),
        serde_json::json!(event.event_id.as_deref()),
    );
    raw.insert(
        "origin_server_ts".to_owned(),
        serde_json::json!(event.origin_server_ts),
    );
    raw.insert("content".to_owned(), serde_json::Value::Object(content));
    raw.insert(
        "unsigned".to_owned(),
        serde_json::json!(event.unsigned.as_ref()),
    );
    serde_json::Value::Object(raw)
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
    use std::io;
    use std::sync::Arc;
    use std::time::Duration;

    use organon::testing::install_crypto_provider;
    use tokio::sync::{Mutex, mpsc};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn assert_join_set_drains(handles: &mut JoinSet<()>) {
        while !handles.is_empty() {
            let result = tokio::time::timeout(Duration::from_secs(5), handles.join_next())
                .await
                .expect("Matrix task did not stop before timeout")
                .expect("Matrix JoinSet ended before all tracked tasks drained");
            result.expect("Matrix task panicked");
        }
    }

    async fn wait_for_ingress_state(
        state: &Arc<Mutex<MatrixIngressState>>,
        expected: MatrixIngressState,
    ) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if *state.lock().await == expected {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Matrix ingress state did not reach expected value");
    }

    struct LoadFailingCursorStore;

    impl CursorStore for LoadFailingCursorStore {
        fn load(&self, _channel: &str, _account: &str) -> io::Result<Option<String>> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "synthetic cursor failure",
            ))
        }

        fn save(&self, _channel: &str, _account: &str, _cursor: &str) -> io::Result<()> {
            Ok(())
        }
    }

    struct SaveFailingCursorStore;

    impl CursorStore for SaveFailingCursorStore {
        fn load(&self, _channel: &str, _account: &str) -> io::Result<Option<String>> {
            Ok(None)
        }

        fn save(&self, _channel: &str, _account: &str, _cursor: &str) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "synthetic cursor failure",
            ))
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

        let msg = extract_message(
            "!room:example.org",
            &event,
            Some("@bot:example.org"),
            "primary",
            true,
        )
        .expect("message");
        assert_eq!(msg.channel, "matrix");
        assert_eq!(msg.sender, "@alice:example.org");
        assert_eq!(msg.group_id.as_deref(), Some("!room:example.org"));
        assert_eq!(msg.account_id.as_deref(), Some("primary"));
        assert_eq!(msg.message_id.as_deref(), Some("$event"));
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.timestamp, 100);
        assert!(msg.raw.is_some(), "retain_raw=true keeps the raw event");
    }

    #[test]
    fn extract_matrix_message_drops_raw_event_unless_opted_in() {
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

        let msg = extract_message(
            "!room:example.org",
            &event,
            Some("@bot:example.org"),
            "primary",
            false,
        )
        .expect("message");
        assert!(
            msg.raw.is_none(),
            "raw event must be absent by default (opt-in retention)"
        );
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

        assert!(
            extract_message(
                "!room:example.org",
                &event,
                Some("@bot:example.org"),
                "primary",
                true
            )
            .is_none()
        );
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
                idempotency_key: None,
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
        assert_eq!(msg.account_id.as_deref(), Some("primary"));
        assert_eq!(msg.text, "hello from Matrix");

        token.cancel();
        drop(rx);
        assert_join_set_drains(&mut handles).await;
    }

    #[tokio::test]
    async fn sync_once_persists_cursor_checkpoint() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .and(header("authorization", "Bearer token-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s9",
                "rooms": { "join": {} }
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
        let dir = tempfile::tempdir().expect("tmpdir");
        let store: Arc<dyn CursorStore> = Arc::new(crate::cursor::FileCursorStore::new(
            dir.path().to_path_buf(),
        ));
        let since: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (tx, _rx) = mpsc::channel::<InboundMessage>(4);

        sync_once(&client, &tx, &since, None, "primary", Some(&store), false)
            .await
            .expect("sync_once");

        assert_eq!(
            store.load("matrix", "primary").expect("load").as_deref(),
            Some("s9"),
            "next_batch must be persisted for restart recovery"
        );
    }

    #[tokio::test]
    async fn sync_loop_resumes_from_persisted_cursor() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .and(header("authorization", "Bearer token-123"))
            .and(query_param("since", "s-saved"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s10",
                "rooms": { "join": {} }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tmpdir");
        let store: Arc<dyn CursorStore> = Arc::new(crate::cursor::FileCursorStore::new(
            dir.path().to_path_buf(),
        ));
        store.save("matrix", "primary", "s-saved").expect("save");

        let mut provider = MatrixProvider::from_config(&taxis::config::MessagingConfig {
            receive_timeout_secs: 1,
            ..taxis::config::MessagingConfig::default()
        })
        .with_cursor_store(store);
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
        let (rx, mut handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());

        let ingress_state = Arc::clone(
            &provider
                .accounts
                .get("primary")
                .expect("account")
                .ingress_state,
        );
        wait_for_ingress_state(&ingress_state, MatrixIngressState::Active).await;

        token.cancel();
        drop(rx);
        assert_join_set_drains(&mut handles).await;
    }

    #[tokio::test]
    async fn limited_timeline_halts_before_checkpoint_or_forward() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .and(query_param("since", "s0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s1",
                "rooms": {
                    "join": {
                        "!room:example.org": {
                            "timeline": {
                                "limited": true,
                                "prev_batch": "backfill-token",
                                "events": [{
                                    "type": "m.room.message",
                                    "sender": "@alice:example.org",
                                    "event_id": "$event",
                                    "origin_server_ts": 123,
                                    "content": {"msgtype": "m.text", "body": "must not forward"}
                                }]
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
        let dir = tempfile::tempdir().expect("tmpdir");
        let store: Arc<dyn CursorStore> = Arc::new(crate::cursor::FileCursorStore::new(
            dir.path().to_path_buf(),
        ));
        store.save("matrix", "primary", "s0").expect("save");
        let since: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some("s0".to_owned())));
        let (tx, mut rx) = mpsc::channel::<InboundMessage>(4);

        let error = sync_once(&client, &tx, &since, None, "primary", Some(&store), false)
            .await
            .expect_err("limited timeline");

        assert!(matches!(error, error::Error::TimelineGap { .. }));
        assert_eq!(since.lock().await.as_deref(), Some("s0"));
        assert_eq!(
            store.load("matrix", "primary").expect("load").as_deref(),
            Some("s0"),
            "limited timeline must leave the durable cursor unchanged"
        );
        assert!(
            matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "limited timeline event must not reach dispatch"
        );
    }

    #[tokio::test]
    async fn cursor_save_failure_leaves_batch_unaccepted_and_unforwarded() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s1",
                "rooms": {
                    "join": {
                        "!room:example.org": {
                            "timeline": {
                                "events": [{
                                    "type": "m.room.message",
                                    "sender": "@alice:example.org",
                                    "event_id": "$event",
                                    "origin_server_ts": 123,
                                    "content": {"msgtype": "m.text", "body": "must not forward"}
                                }]
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
        let store: Arc<dyn CursorStore> = Arc::new(SaveFailingCursorStore);
        let since: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (tx, mut rx) = mpsc::channel::<InboundMessage>(4);

        let error = sync_once(&client, &tx, &since, None, "primary", Some(&store), false)
            .await
            .expect_err("cursor save");

        assert!(matches!(error, error::Error::Cursor { .. }));
        assert!(since.lock().await.is_none());
        assert!(
            matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "uncheckpointed event must not reach dispatch"
        );
    }

    #[tokio::test]
    async fn limited_timeline_is_visible_in_redacted_account_health() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s1",
                "rooms": {
                    "join": {
                        "!room:example.org": {
                            "timeline": {"limited": true, "events": []}
                        }
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/account/whoami"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "user_id": "@bot:example.org"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tmpdir");
        let store: Arc<dyn CursorStore> = Arc::new(crate::cursor::FileCursorStore::new(
            dir.path().to_path_buf(),
        ));
        let mut provider = MatrixProvider::new().with_cursor_store(store);
        provider.add_account(
            "operator@private".to_owned(),
            client::MatrixClient::with_timeouts(
                &server.uri(),
                "token-123",
                Duration::from_secs(1),
                Duration::from_millis(50),
            )
            .expect("client"),
            Some("@bot:example.org".to_owned()),
            true,
            None,
        );
        let ingress_state = Arc::clone(
            &provider
                .accounts
                .get("operator@private")
                .expect("account")
                .ingress_state,
        );

        let token = CancellationToken::new();
        let (rx, mut handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());
        wait_for_ingress_state(&ingress_state, MatrixIngressState::TimelineGap).await;

        let probe = provider.probe().await;
        assert!(!probe.ok, "reachable homeserver cannot hide a timeline gap");
        let details = serde_json::to_string(&probe.details).expect("details");
        assert!(!details.contains("operator@private"));
        assert!(details.contains(MatrixIngressState::TimelineGap.as_str()));

        token.cancel();
        drop(rx);
        assert_join_set_drains(&mut handles).await;
    }

    #[tokio::test]
    async fn cursor_load_failure_never_starts_unscoped_sync_and_cancels_promptly() {
        install_crypto_provider();
        let server = MockServer::start().await;
        let store: Arc<dyn CursorStore> = Arc::new(LoadFailingCursorStore);
        let mut provider = MatrixProvider::new().with_cursor_store(store);
        provider.add_account(
            "primary".to_owned(),
            client::MatrixClient::with_timeouts(
                &server.uri(),
                "token-123",
                Duration::from_secs(1),
                Duration::from_millis(50),
            )
            .expect("client"),
            None,
            true,
            None,
        );
        let ingress_state = Arc::clone(
            &provider
                .accounts
                .get("primary")
                .expect("account")
                .ingress_state,
        );

        let token = CancellationToken::new();
        let (rx, mut handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());
        wait_for_ingress_state(&ingress_state, MatrixIngressState::CursorUnavailable).await;
        let requests = server
            .received_requests()
            .await
            .expect("mock server records requests");
        assert!(
            requests.is_empty(),
            "cursor failure must precede all sync I/O"
        );

        token.cancel();
        drop(rx);
        assert_join_set_drains(&mut handles).await;
    }

    #[tokio::test]
    async fn receiver_close_interrupts_long_poll_interval() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s1",
                "rooms": {"join": {}}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut provider = MatrixProvider::new();
        provider.add_account(
            "primary".to_owned(),
            client::MatrixClient::with_timeouts(
                &server.uri(),
                "token-123",
                Duration::from_secs(1),
                Duration::from_millis(50),
            )
            .expect("client"),
            None,
            true,
            None,
        );
        let ingress_state = Arc::clone(
            &provider
                .accounts
                .get("primary")
                .expect("account")
                .ingress_state,
        );

        let token = CancellationToken::new();
        let (rx, mut handles) = provider.listen(Some(Duration::from_hours(1)), token.clone());
        wait_for_ingress_state(&ingress_state, MatrixIngressState::Active).await;
        drop(rx);

        assert_join_set_drains(&mut handles).await;
        assert!(
            !token.is_cancelled(),
            "receiver close is independent of cancellation"
        );
        assert!(*ingress_state.lock().await == MatrixIngressState::Stopped);
    }

    #[tokio::test]
    async fn sync_once_accepts_batch_before_sends_complete() {
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
        let handle = tokio::spawn(async move {
            sync_once(
                &client_ref,
                &tx_ref,
                &since_ref,
                Some(&own_user_id),
                "primary",
                None,
                false,
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
            "next_batch should be accepted even when the future is cancelled mid-send"
        );
    }
}
