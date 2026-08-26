//! Signal channel provider: wraps signal-cli JSON-RPC.

/// JSON-RPC client for the signal-cli HTTP daemon.
pub mod client;
/// Connection state machine and outbound message buffering during disconnection.
pub mod connection;
/// Signal envelope deserialization and inbound message extraction.
pub mod envelope;
/// Signal-specific error types.
pub mod error;

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, instrument};

use crate::types::{
    ChannelCapabilities, ChannelProvider, InboundMessage, ProbeResult,
    SendParams as ChannelSendParams, SendResult,
};
use client::SendDisposition;
use connection::{
    AccountState, ConnectionHealthReport, ConnectionState, EnqueueDisposition, reconnect_delay,
};

/// Fallback default; runtime reads `MessagingConfig::poll_interval_ms`.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Fallback default; runtime reads `MessagingConfig::buffer_capacity`.
pub const DEFAULT_BUFFER_CAPACITY: usize = 100;

/// Consecutive poll failures before the circuit breaker trips and polling halts.
/// WHY: lowered from 20 to 5 because signal-cli being down is the common case
/// (`auto_start=false`, user hasn't started it), and 20 retries at exponential
/// backoff = several minutes of warn-level log spam before halting (#3104).
/// Fallback default; runtime reads `MessagingConfig::circuit_breaker_threshold`.
pub const CIRCUIT_BREAKER_THRESHOLD: u32 = 5;

/// Interval between health checks while the circuit breaker is open.
/// Fallback default; runtime reads `MessagingConfig::halted_health_check_interval_secs`.
pub const HALTED_HEALTH_CHECK_INTERVAL: Duration = Duration::from_mins(1);

/// Parsed Signal message target.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignalTarget {
    /// Direct message to a phone number (e.g., `"+1234567890"`).
    Phone(String),
    /// Group message identified by base64 group ID.
    Group(String),
}

impl std::fmt::Debug for SignalTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Phone(_) => f.debug_tuple("Phone").field(&"<redacted>").finish(),
            Self::Group(_) => f.debug_tuple("Group").field(&"<redacted>").finish(),
        }
    }
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
    reactions: false,
    typing: false,
    // Inbound attachment handles are not yet materialized or carried through
    // dispatch, so advertising general media support would be false.
    media: false,
    streaming: false,
    rich_formatting: false,
    max_text_length: 2000,
};

/// One configured Signal account and its private wire identity.
#[derive(Clone)]
struct SignalAccount {
    client: client::SignalClient,
    /// Private signal-cli wire selector. `None` delegates to the daemon's
    /// default and is never replaced by the public logical label.
    wire_account: Option<String>,
    state: Arc<Mutex<AccountState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: tokio mutex is shared by sends and the polling task
    auto_start: bool,
}

/// Signal channel provider implementing `ChannelProvider`.
///
/// Manages multiple Signal accounts, each backed by a `SignalClient`.
/// Tracks connection state per account with reconnect backoff and
/// outbound message buffering during disconnection.
pub struct SignalProvider {
    /// Stable logical labels used by routing and inbound attribution.
    accounts: HashMap<String, SignalAccount>,
    default_account: Option<String>,
    buffer_capacity: usize,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
}

impl SignalProvider {
    /// Create an empty provider with default config. Add accounts with [`add_account`](Self::add_account).
    #[must_use]
    pub fn new() -> Self {
        Self::with_buffer_capacity(DEFAULT_BUFFER_CAPACITY)
    }

    /// Create a provider with a custom outbound buffer capacity.
    #[must_use]
    pub fn with_buffer_capacity(capacity: usize) -> Self {
        Self {
            accounts: HashMap::new(),
            default_account: None,
            buffer_capacity: capacity,
            circuit_breaker_threshold: CIRCUIT_BREAKER_THRESHOLD,
            halted_health_check_interval: HALTED_HEALTH_CHECK_INTERVAL,
        }
    }

    /// Create a provider from a `MessagingConfig`.
    #[must_use]
    pub fn from_config(config: &taxis::config::MessagingConfig) -> Self {
        Self {
            accounts: HashMap::new(),
            default_account: None,
            buffer_capacity: config.buffer_capacity,
            circuit_breaker_threshold: config.circuit_breaker_threshold,
            halted_health_check_interval: Duration::from_secs(
                config.halted_health_check_interval_secs,
            ),
        }
    }

    /// Register a Signal account backed by a client.
    ///
    /// The first account added becomes the default. When `auto_start` is
    /// `true`, [`listen`](Self::listen) spawns a receive-poll task for this
    /// account; when `false`, the account is registered for sending but the
    /// receive loop is not started automatically.
    pub fn add_account(
        &mut self,
        logical_label: String,
        wire_account: Option<String>,
        client: client::SignalClient,
        auto_start: bool,
    ) {
        if self.default_account.is_none() {
            self.default_account = Some(logical_label.clone());
        }
        self.accounts.insert(
            logical_label,
            SignalAccount {
                client,
                wire_account,
                state: Arc::new(Mutex::new(AccountState::new(self.buffer_capacity))),
                auto_start,
            },
        );
    }

    /// Start listening for inbound messages on accounts with `auto_start` enabled.
    ///
    /// Spawns a polling task per eligible account with reconnect backoff.
    /// Accounts where `auto_start` was set to `false` in [`add_account`](Self::add_account)
    /// are skipped (they remain available for sending but do not receive).
    /// Messages from all started accounts merge into the returned receiver.
    /// When the `cancel` token is cancelled, polling tasks exit promptly.
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

        for (logical_label, account) in &self.accounts {
            // WHY: skip accounts where auto_start is false -- they are registered
            // for outbound sends but should not spawn a receive poll loop.
            if !account.auto_start {
                tracing::info!("skipping receive loop (auto_start=false)");
                continue;
            }
            let tx = tx.clone();
            let logical_label = logical_label.clone();
            let wire_account = account.wire_account.clone();
            let signal_client = account.client.clone();
            let token = cancel.clone();
            let state = Arc::clone(&account.state);
            let circuit_breaker_threshold = self.circuit_breaker_threshold;
            let halted_health_check_interval = self.halted_health_check_interval;
            let span = tracing::info_span!(
                "signal_poll",
                account = %crate::redact::opaque_identifier("signal-account", &logical_label)
            );

            handles.spawn(
                async move {
                    let _subscription = crate::metrics::ActiveSubscriptionGuard::new();
                    poll_loop(
                        signal_client,
                        logical_label,
                        wire_account,
                        tx,
                        interval,
                        state,
                        token,
                        circuit_breaker_threshold,
                        halted_health_check_interval,
                    )
                    .await;
                }
                .instrument(span),
            );
        }

        (rx, handles)
    }

    /// Query connection health for all accounts.
    ///
    /// Report keys are opaque handles derived from public logical labels.
    pub async fn connection_health(&self) -> HashMap<String, ConnectionHealthReport> {
        let mut reports = HashMap::new();
        for (logical_label, account) in &self.accounts {
            let s = account.state.lock().await;
            reports.insert(
                crate::redact::opaque_identifier("signal-account", logical_label),
                ConnectionHealthReport {
                    state: s.state.clone(),
                    buffered_messages: s.buffered_count(),
                    dropped_count: s.dropped_count,
                    ambiguous_delivery_count: s.ambiguous_delivery_count,
                    partial_delivery_count: s.partial_delivery_count,
                    receive_loss_count: s.receive_loss_count,
                },
            );
        }
        reports
    }

    fn resolve_account(&self, account_id: Option<&str>) -> Option<(&str, &SignalAccount)> {
        let key = account_id.or(self.default_account.as_deref())?;
        self.accounts
            .get_key_value(key)
            .map(|(k, v)| (k.as_str(), v))
    }

    fn build_send_params(
        wire_account: Option<&str>,
        params: &ChannelSendParams,
    ) -> client::SendParams {
        let target = parse_target(&params.to);
        let (recipient, group_id) = match target {
            SignalTarget::Phone(phone) => (Some(phone), None),
            SignalTarget::Group(gid) => (None, Some(gid)),
        };
        client::SendParams {
            message: Some(params.text.clone()),
            recipient,
            group_id,
            account: wire_account.map(ToOwned::to_owned),
            attachments: params.attachments.clone(),
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
            if params
                .attachments
                .as_ref()
                .is_some_and(|attachments| !attachments.is_empty())
            {
                return SendResult::err("signal_media_unsupported");
            }

            let Some((logical_label, account)) = self.resolve_account(params.account_id.as_deref())
            else {
                return SendResult::err("no Signal client available");
            };

            let send_params = Self::build_send_params(account.wire_account.as_deref(), params);

            // The state mutex is also the per-account send sequencer. Tokio's
            // mutex queues waiters FIFO, so recovery drain cannot be overtaken
            // by a newly submitted message.
            let mut state = account.state.lock().await;
            if state.state != ConnectionState::Connected {
                if !account.client.health().await {
                    let disposition = state.enqueue(send_params);
                    return enqueue_result(disposition, "signal_connection_unavailable_buffered");
                }
                state.state = ConnectionState::Connected;
            }

            if drain_buffer(&account.client, &mut state).await == DrainOutcome::BlockedOnConnect {
                let disposition = state.enqueue(send_params);
                return enqueue_result(disposition, "signal_connect_failure_buffered");
            }

            let attempt = DirectDeliveryAttempt::new(&mut state);
            let result = account.client.send_message(&send_params).await;
            attempt.resolve();
            match result {
                Ok(SendDisposition::Delivered) => SendResult::ok(),
                Ok(SendDisposition::Partial) => {
                    state.record_partial_delivery();
                    SendResult::err("signal_delivery_partial")
                }
                Ok(SendDisposition::Rejected) => SendResult::err("signal_delivery_rejected"),
                Err(failure) if failure.safe_to_retry_delivery() => {
                    state.state = ConnectionState::Reconnecting { attempt: 1 };
                    let disposition = state.enqueue(send_params);
                    enqueue_result(disposition, "signal_connect_failure_buffered")
                }
                Err(failure) if failure.delivery_outcome_ambiguous() => {
                    state.record_ambiguous_delivery();
                    SendResult::err("signal_delivery_ambiguous")
                }
                Err(_failure) => {
                    tracing::warn!(
                        account = %crate::redact::opaque_identifier("signal-account", logical_label),
                        "Signal send failed before a confirmed delivery"
                    );
                    SendResult::err("signal_provider_failure")
                }
            }
        })
    }

    fn listen(
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
        SignalProvider::listen(self, poll_interval, cancel)
    }

    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
        Box::pin(async move {
            if self.accounts.is_empty() {
                return ProbeResult {
                    ok: false,
                    latency_ms: None,
                    error: Some("no Signal clients configured".to_owned()),
                    details: None,
                };
            }

            let mut account_results = HashMap::new();
            let mut any_ok = false;

            for (logical_label, account) in &self.accounts {
                let ok = account.client.health().await;
                if ok {
                    any_ok = true;
                }
                let mut detail = serde_json::json!({"reachable": ok});

                let s = account.state.lock().await;
                let map = detail.as_object_mut();
                debug_assert!(map.is_some(), "detail is a JSON object");
                if let Some(map) = map {
                    map.insert(
                        "connection_state".to_owned(),
                        serde_json::json!(format!("{:?}", s.state)),
                    );
                    map.insert(
                        "buffered_messages".to_owned(),
                        serde_json::json!(s.buffered_count()),
                    );
                }

                account_results.insert(
                    crate::redact::opaque_identifier("signal-account", logical_label),
                    detail,
                );
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

fn enqueue_result(disposition: EnqueueDisposition, queued_error: &'static str) -> SendResult {
    match disposition {
        EnqueueDisposition::Queued => SendResult::err(queued_error),
        EnqueueDisposition::DroppedDisabled => SendResult::err("signal_buffer_disabled_dropped"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainOutcome {
    Complete,
    BlockedOnConnect,
}

/// Drain the oldest queued messages without letting a new send overtake them.
/// A message leaves the queue before the network await. Only a proven connect
/// failure restores it; cancellation and ambiguous outcomes can never replay.
async fn drain_buffer(
    signal_client: &client::SignalClient,
    state: &mut AccountState,
) -> DrainOutcome {
    while let Some(attempt) = BufferedDeliveryAttempt::take(state) {
        let result = signal_client.send_message(attempt.params()).await;
        match result {
            Ok(SendDisposition::Delivered) => attempt.resolve_delivered(),
            Ok(SendDisposition::Partial) => {
                attempt.resolve_partial();
                tracing::warn!("buffered Signal send reached only some recipients");
            }
            Ok(SendDisposition::Rejected) => {
                attempt.resolve_rejected();
                tracing::warn!("buffered Signal send was rejected");
            }
            Err(failure) if failure.safe_to_retry_delivery() => {
                attempt.restore_connect_failure();
                return DrainOutcome::BlockedOnConnect;
            }
            Err(failure) if failure.delivery_outcome_ambiguous() => {
                attempt.resolve_ambiguous();
                tracing::warn!("buffered Signal delivery outcome was ambiguous");
            }
            Err(_failure) => {
                attempt.resolve_rejected();
                tracing::warn!("buffered Signal send failed permanently");
            }
        }
    }
    DrainOutcome::Complete
}

struct DirectDeliveryAttempt<'a> {
    state: &'a mut AccountState,
    resolved: bool,
}

impl<'a> DirectDeliveryAttempt<'a> {
    fn new(state: &'a mut AccountState) -> Self {
        Self {
            state,
            resolved: false,
        }
    }

    fn resolve(mut self) {
        self.resolved = true;
    }
}

impl Drop for DirectDeliveryAttempt<'_> {
    fn drop(&mut self) {
        if !self.resolved {
            self.state.record_ambiguous_delivery();
        }
    }
}

struct BufferedDeliveryAttempt<'a> {
    state: &'a mut AccountState,
    params: Option<client::SendParams>,
}

impl<'a> BufferedDeliveryAttempt<'a> {
    fn take(state: &'a mut AccountState) -> Option<Self> {
        let params = state.take_front()?;
        Some(Self {
            state,
            params: Some(params),
        })
    }

    fn params(&self) -> &client::SendParams {
        let Some(params) = self.params.as_ref() else {
            unreachable!("buffered attempt owns params until disposition");
        };
        params
    }

    fn resolve_delivered(mut self) {
        drop(self.params.take());
    }

    fn resolve_partial(mut self) {
        drop(self.params.take());
        self.state.record_partial_delivery();
    }

    fn resolve_rejected(mut self) {
        drop(self.params.take());
        self.state.record_failed_delivery();
    }

    fn resolve_ambiguous(mut self) {
        drop(self.params.take());
        self.state.record_ambiguous_delivery();
    }

    fn restore_connect_failure(mut self) {
        if let Some(params) = self.params.take() {
            self.state.push_front(params);
        }
        self.state.state = ConnectionState::Reconnecting { attempt: 1 };
    }
}

impl Drop for BufferedDeliveryAttempt<'_> {
    fn drop(&mut self) {
        if self.params.take().is_some() {
            self.state.record_ambiguous_delivery();
        }
    }
}

impl std::fmt::Debug for SignalProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalProvider")
            .field("account_count", &self.accounts.len())
            .field("has_default_account", &self.default_account.is_some())
            .field(
                "auto_start_count",
                &self
                    .accounts
                    .values()
                    .filter(|account| account.auto_start)
                    .count(),
            )
            .field("buffer_capacity", &self.buffer_capacity)
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
    reason = "poll_loop is one state machine; logical/wire account identity rides with the connection state it drives"
)]
#[expect(
    clippy::too_many_lines,
    reason = "single destructive-receive state machine keeps cancellation shielding, loss accounting, and connection transitions visibly ordered"
)]
async fn poll_loop(
    signal_client: client::SignalClient,
    logical_label: String,
    wire_account: Option<String>,
    tx: mpsc::Sender<InboundMessage>,
    interval: Duration,
    state: Arc<Mutex<AccountState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
    cancel: CancellationToken,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
) {
    tracing::info!("polling started");
    loop {
        if cancel.is_cancelled() || tx.is_closed() {
            tracing::info!("Signal polling stop requested");
            return;
        }

        let halted_failures = {
            let current = state.lock().await;
            match current.state {
                ConnectionState::Halted { total_failures } => Some(total_failures),
                ConnectionState::Connected | ConnectionState::Reconnecting { .. } => None,
            }
        };
        if let Some(total_failures) = halted_failures {
            if wait_or_stop(&cancel, &tx, halted_health_check_interval).await {
                return;
            }
            if signal_client.health().await {
                let mut current = state.lock().await;
                tracing::info!(previous_failures = total_failures, "connection restored");
                current.state = ConnectionState::Connected;
                if drain_buffer(&signal_client, &mut current).await
                    == DrainOutcome::BlockedOnConnect
                {
                    tracing::warn!("Signal health recovery drain blocked on connection");
                }
            }
            continue;
        }

        // `receive` consumes daemon state. Once submitted, cancellation cannot
        // drop its future: finish the bounded request and account for its result.
        let receive_attempt = ReceiveAttemptGuard::new(Arc::clone(&state));
        let (cancelled_during_receive, result) =
            receive_without_cancelling(&signal_client, wire_account.as_deref(), &cancel).await;
        match result {
            Ok(batch) => {
                {
                    let mut current = state.lock().await;
                    current.state = ConnectionState::Connected;
                }

                let (messages, losses) =
                    normalize_batch(batch, &logical_label, wire_account.as_deref());
                if losses > 0 {
                    let mut current = state.lock().await;
                    current.record_receive_loss(losses);
                    tracing::warn!(
                        count = losses,
                        "Signal receive contained data that could not be forwarded"
                    );
                }
                if forward_batch(messages, &tx, &state, &cancel, cancelled_during_receive).await {
                    receive_attempt.resolve();
                    return;
                }
                {
                    let mut current = state.lock().await;
                    if drain_buffer(&signal_client, &mut current).await
                        == DrainOutcome::BlockedOnConnect
                    {
                        tracing::warn!("Signal receive recovery drain blocked on connection");
                    }
                }
                receive_attempt.resolve();
                if wait_or_stop(&cancel, &tx, interval).await {
                    return;
                }
            }
            Err(failure) => {
                if failure.receive_outcome_ambiguous() {
                    let mut current = state.lock().await;
                    current.record_receive_loss(1);
                    tracing::warn!(
                        failure_class = signal_failure_class(&failure),
                        "Signal receive outcome is ambiguous; at least one loss incident recorded"
                    );
                }
                receive_attempt.resolve();
                if cancelled_during_receive {
                    return;
                }

                let Some(attempt) = record_poll_failure(
                    &state,
                    circuit_breaker_threshold,
                    halted_health_check_interval,
                )
                .await
                else {
                    continue;
                };
                let delay = reconnect_delay(attempt);
                tracing::warn!(
                    failure_class = signal_failure_class(&failure),
                    attempt,
                    backoff_secs = delay.as_secs(),
                    "receive poll failed, backing off"
                );
                if wait_or_stop(&cancel, &tx, delay).await {
                    return;
                }
            }
        }
    }
}

struct ReceiveAttemptGuard {
    state: Arc<Mutex<AccountState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: detached loss recorder must outlive an aborted polling task
    resolved: bool,
}

impl ReceiveAttemptGuard {
    fn new(state: Arc<Mutex<AccountState>>) -> Self {
        Self {
            state,
            resolved: false,
        }
    }

    fn resolve(mut self) {
        self.resolved = true;
    }
}

impl Drop for ReceiveAttemptGuard {
    fn drop(&mut self) {
        if self.resolved {
            return;
        }
        let state = Arc::clone(&self.state);
        // The polling task may itself be aborted. Detach the minimal accounting
        // write so destructive receive cancellation is never silent.
        std::mem::drop(tokio::spawn(async move {
            state.lock().await.record_receive_loss(1);
        }));
    }
}

async fn receive_without_cancelling(
    signal_client: &client::SignalClient,
    wire_account: Option<&str>,
    cancel: &CancellationToken,
) -> (bool, error::Result<client::ReceiveBatch>) {
    let receive = signal_client.receive_batch(wire_account);
    tokio::pin!(receive);
    tokio::select! {
        result = &mut receive => (false, result),
        () = cancel.cancelled() => (true, receive.await),
    }
}

fn normalize_batch(
    batch: client::ReceiveBatch,
    logical_label: &str,
    wire_account: Option<&str>,
) -> (VecDeque<InboundMessage>, u64) {
    let mut messages = VecDeque::new();
    let mut losses = 0_u64;
    for entry in batch.entries {
        let client::ReceiveEntry::Envelope(received) = entry else {
            losses = losses.saturating_add(1);
            continue;
        };
        let wrapper_matches = match wire_account {
            Some(expected) => received.account.as_deref() == Some(expected),
            None => received.account.is_some(),
        };
        if !wrapper_matches {
            losses = losses.saturating_add(1);
            continue;
        }
        match envelope::extract_received(&received.envelope, &received.raw, Some(logical_label)) {
            envelope::EnvelopeOutcome::Message {
                message,
                lost_parts,
            } => {
                losses = losses.saturating_add(lost_parts);
                messages.push_back(*message);
            }
            envelope::EnvelopeOutcome::ExpectedControl(_kind) => {}
            envelope::EnvelopeOutcome::UnsupportedContentLost { lost_parts, .. }
            | envelope::EnvelopeOutcome::MalformedLost { lost_parts, .. } => {
                losses = losses.saturating_add(lost_parts);
            }
        }
    }
    (messages, losses)
}

async fn forward_batch(
    mut messages: VecDeque<InboundMessage>,
    tx: &mpsc::Sender<InboundMessage>,
    state: &Arc<Mutex<AccountState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: shared provider state
    cancel: &CancellationToken,
    mut shutting_down: bool,
) -> bool {
    while let Some(message) = messages.pop_front() {
        if shutting_down || cancel.is_cancelled() {
            shutting_down = true;
            if tx.try_send(message).is_err() {
                let lost = pending_message_count(&messages);
                state.lock().await.record_receive_loss(lost);
                tracing::warn!(
                    count = lost,
                    "Signal shutdown could not forward an accepted receive batch"
                );
                return true;
            }
            continue;
        }

        let permit = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                shutting_down = true;
                messages.push_front(message);
                continue;
            }
            result = tx.reserve() => {
                let Ok(permit) = result else {
                    let lost = pending_message_count(&messages);
                    state.lock().await.record_receive_loss(lost);
                    tracing::warn!(count = lost, "Signal receiver closed with accepted messages pending");
                    return true;
                };
                permit
            }
        };
        permit.send(message);
    }
    shutting_down
}

fn pending_message_count(messages: &VecDeque<InboundMessage>) -> u64 {
    u64::try_from(messages.len())
        .unwrap_or(u64::MAX)
        .saturating_add(1)
}

async fn record_poll_failure(
    state: &Arc<Mutex<AccountState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: shared provider state
    threshold: u32,
    halted_health_check_interval: Duration,
) -> Option<u32> {
    let mut current = state.lock().await;
    let next = match current.state {
        ConnectionState::Connected => 1,
        ConnectionState::Reconnecting { attempt } => attempt.saturating_add(1),
        ConnectionState::Halted { .. } => return None,
    };
    if next >= threshold.max(1) {
        current.state = ConnectionState::Halted {
            total_failures: next,
        };
        tracing::error!(
            consecutive_failures = next,
            threshold = threshold.max(1),
            health_check_secs = halted_health_check_interval.as_secs(),
            "circuit breaker tripped, halting Signal polling"
        );
        None
    } else {
        current.state = ConnectionState::Reconnecting { attempt: next };
        Some(next)
    }
}

async fn wait_or_stop(
    cancel: &CancellationToken,
    tx: &mpsc::Sender<InboundMessage>,
    duration: Duration,
) -> bool {
    tokio::select! {
        biased;
        () = cancel.cancelled() => true,
        () = tx.closed() => true,
        () = tokio::time::sleep(duration) => false,
    }
}

fn signal_failure_class(failure: &error::Error) -> &'static str {
    match failure {
        error::Error::Rpc { .. } => "rpc",
        error::Error::Http { .. } => "http",
        error::Error::HttpStatus { .. } => "http_status",
        error::Error::InvalidUrl { .. } => "invalid_url",
        error::Error::InsecureTransport { .. } => "insecure_transport",
        error::Error::NoAccount { .. } => "no_account",
        error::Error::Json { .. } => "json",
        error::Error::Protocol { .. } => "protocol",
        error::Error::ReceiveOutcomeUnknown { .. } => "receive_outcome_unknown",
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use organon::testing::install_crypto_provider;
    use tracing::Instrument;

    use super::*;

    fn echo_rpc_id(response_body: serde_json::Value) -> impl wiremock::Respond + 'static {
        move |request: &wiremock::Request| {
            let request_body: serde_json::Value = request
                .body_json()
                .expect("test request must contain JSON-RPC JSON");
            let mut body = response_body.clone();
            body.as_object_mut()
                .expect("test response must be an object")
                .insert(
                    "id".to_owned(),
                    request_body
                        .get("id")
                        .cloned()
                        .expect("test request must contain an id"),
                );
            wiremock::ResponseTemplate::new(200).set_body_json(body)
        }
    }

    fn send_response(
        request: &wiremock::Request,
        result_types: &[&str],
    ) -> wiremock::ResponseTemplate {
        let request_body: serde_json::Value =
            request.body_json().expect("test request must contain JSON");
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_body.get("id").cloned().expect("request id"),
            "result": {
                "timestamp": 100,
                "results": result_types
                    .iter()
                    .map(|kind| serde_json::json!({"type": kind}))
                    .collect::<Vec<_>>()
            }
        }))
    }

    async fn assert_join_set_drains(mut handles: JoinSet<()>) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(result) = handles.join_next().await {
                result.expect("Signal poll task must not panic");
            }
        })
        .await
        .expect("Signal poll tasks must stop promptly");
    }

    fn channel_params(text: &str) -> ChannelSendParams {
        ChannelSendParams {
            to: "+15550999".to_owned(),
            text: text.to_owned(),
            account_id: None,
            sender_id: None,
            idempotency_key: None,
            thread_id: None,
            attachments: None,
        }
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
        assert!(!SIGNAL_CAPABILITIES.reactions);
        assert!(!SIGNAL_CAPABILITIES.typing);
        assert!(!SIGNAL_CAPABILITIES.media);
        assert!(!SIGNAL_CAPABILITIES.streaming);
        assert!(!SIGNAL_CAPABILITIES.rich_formatting);
        assert_eq!(SIGNAL_CAPABILITIES.max_text_length, 2000);
    }

    #[test]
    fn receive_wrapper_account_mismatch_is_lost_not_misattributed() {
        let raw = serde_json::json!({
            "sourceUuid": "uuid-account",
            "timestamp": 100,
            "dataMessage": {"timestamp": 100, "message": "private"}
        });
        let envelope = serde_json::from_value(raw.clone()).expect("envelope");
        let batch = client::ReceiveBatch {
            entries: vec![client::ReceiveEntry::Envelope(Box::new(
                client::ReceivedEnvelope {
                    account: Some("+1222222222".to_owned()),
                    envelope,
                    raw,
                },
            ))],
        };
        let (messages, losses) = normalize_batch(batch, "logical", Some("+1111111111"));
        assert!(messages.is_empty());
        assert_eq!(losses, 1);
    }

    #[test]
    fn wire_wrapper_is_attributed_to_logical_label_and_raw_is_always_removed() {
        let raw = serde_json::json!({
            "sourceUuid": "uuid-account",
            "timestamp": 100,
            "dataMessage": {"timestamp": 100, "message": "private"}
        });
        let envelope = serde_json::from_value(raw.clone()).expect("envelope");
        let batch = client::ReceiveBatch {
            entries: vec![client::ReceiveEntry::Envelope(Box::new(
                client::ReceivedEnvelope {
                    account: Some("+1111111111".to_owned()),
                    envelope,
                    raw,
                },
            ))],
        };
        let (mut messages, losses) = normalize_batch(batch, "primary", Some("+1111111111"));
        let message = messages.pop_front().expect("normalized message");
        assert_eq!(message.account_id.as_deref(), Some("primary"));
        assert!(message.raw.is_none());
        assert_eq!(losses, 0);
    }

    #[test]
    fn logical_label_is_never_used_as_an_implicit_wire_account() {
        let params = SignalProvider::build_send_params(None, &channel_params("hello"));
        assert!(params.account.is_none());
        let params =
            SignalProvider::build_send_params(Some("+1111111111"), &channel_params("hello"));
        assert_eq!(params.account.as_deref(), Some("+1111111111"));
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
        let (rx, handles) = provider.listen(None, CancellationToken::new());
        assert!(handles.is_empty());
        drop(rx);
    }

    #[tokio::test]
    async fn listen_returns_receiver_and_handles() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": []
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(rpc_response))
            .mount(&server)
            .await;

        let mut provider = SignalProvider::new();
        let signal_client = client::SignalClient::new(&server.uri()).expect("client");
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            signal_client,
            true,
        );

        let token = CancellationToken::new();
        let (rx, handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());
        assert_eq!(handles.len(), 1);

        token.cancel();
        drop(rx);
        assert_join_set_drains(handles).await;
    }

    #[tokio::test]
    async fn listen_skips_auto_start_false() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": []
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(rpc_response))
            .mount(&server)
            .await;

        let mut provider = SignalProvider::new();
        let client_a = client::SignalClient::new(&server.uri()).expect("client");
        let client_b = client::SignalClient::new(&server.uri()).expect("client");
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client_a,
            true,
        );
        provider.add_account(
            "secondary".to_owned(),
            Some("+2222222222".to_owned()),
            client_b,
            false,
        );

        let token = CancellationToken::new();
        let (rx, handles) = provider.listen(Some(Duration::from_mins(1)), token.clone());
        // WHY: only the auto_start=true account should have a poll task.
        assert_eq!(handles.len(), 1);

        token.cancel();
        drop(rx);
        assert_join_set_drains(handles).await;
    }

    #[tokio::test]
    async fn poll_loop_stops_on_receiver_drop() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": [
                {
                    "account": "+1111111111",
                    "envelope": {
                        "sourceNumber": "+9999999999",
                        "timestamp": 100,
                        "dataMessage": {
                            "timestamp": 100,
                            "message": "test msg"
                        }
                    }
                }
            ]
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(rpc_response))
            .mount(&server)
            .await;

        let signal_client = client::SignalClient::new(&server.uri()).expect("client");
        let (tx, mut rx) = mpsc::channel(16);
        let account_state = Arc::new(Mutex::new(AccountState::new(100)));
        let token = CancellationToken::new();

        let handle = tokio::spawn(
            super::poll_loop(
                signal_client,
                "primary".to_owned(),
                Some("+1111111111".to_owned()),
                tx,
                Duration::from_millis(50),
                account_state,
                token,
                CIRCUIT_BREAKER_THRESHOLD,
                HALTED_HEALTH_CHECK_INTERVAL,
            )
            .instrument(tracing::info_span!("test_poll_loop")),
        );

        let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("message");
        assert_eq!(msg.text, "test msg");
        assert_eq!(msg.account_id.as_deref(), Some("primary"));

        drop(rx);
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("poll loop should stop when receiver is dropped")
            .expect("poll loop must not panic");
    }

    #[tokio::test]
    async fn cancellation_finishes_inflight_receive_and_forwards_accepted_message() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        let request_seen = Arc::new(tokio::sync::Notify::new());
        let notify = Arc::clone(&request_seen);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(move |request: &wiremock::Request| {
                notify.notify_one();
                let request_body: serde_json::Value =
                    request.body_json().expect("request must be JSON");
                wiremock::ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(100))
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request_body
                            .get("id")
                            .cloned()
                            .expect("request must contain id"),
                        "result": [{
                            "account": "+1111111111",
                            "envelope": {
                                "sourceUuid": "uuid-shutdown",
                                "timestamp": 100,
                                "dataMessage": {"timestamp": 100, "message": "accepted"}
                            }
                        }]
                    }))
            })
            .expect(1)
            .mount(&server)
            .await;

        let signal_client = client::SignalClient::new(&server.uri()).expect("client");
        let (tx, mut rx) = mpsc::channel(16);
        let account_state = Arc::new(Mutex::new(AccountState::new(100)));
        let token = CancellationToken::new();
        let handle = tokio::spawn(super::poll_loop(
            signal_client,
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            tx,
            Duration::from_mins(1),
            Arc::clone(&account_state),
            token.clone(),
            CIRCUIT_BREAKER_THRESHOLD,
            HALTED_HEALTH_CHECK_INTERVAL,
        ));

        request_seen.notified().await;
        token.cancel();
        let message = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("shielded receive should finish")
            .expect("accepted message should be forwarded");
        assert_eq!(message.text, "accepted");
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("poll task should stop after draining accepted batch")
            .expect("poll task must not panic");
        assert_eq!(account_state.lock().await.receive_loss_count, 0);
    }

    #[tokio::test]
    async fn recovery_drains_buffer_fifo_before_new_send() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v1/check"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(serde_json::json!({
                "jsonrpc": "2.0",
                "result": {"timestamp": 100, "results": [{"type": "SUCCESS"}]}
            })))
            .expect(3)
            .mount(&server)
            .await;

        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let state = Arc::clone(&provider.accounts.get("primary").expect("account").state);
        {
            let mut current = state.lock().await;
            current.state = ConnectionState::Reconnecting { attempt: 1 };
            assert_eq!(
                current.enqueue(SignalProvider::build_send_params(
                    Some("+1111111111"),
                    &channel_params("old-1"),
                )),
                EnqueueDisposition::Queued
            );
            assert_eq!(
                current.enqueue(SignalProvider::build_send_params(
                    Some("+1111111111"),
                    &channel_params("old-2"),
                )),
                EnqueueDisposition::Queued
            );
        }

        let result = provider.send(&channel_params("new")).await;
        assert!(
            result.sent,
            "recovered send should succeed: {:?}",
            result.error
        );

        let requests = server
            .received_requests()
            .await
            .expect("request recording enabled");
        let messages: Vec<String> = requests
            .iter()
            .filter_map(|request| request.body_json::<serde_json::Value>().ok())
            .filter_map(|body| {
                body.get("params")?
                    .get("message")?
                    .as_str()
                    .map(ToOwned::to_owned)
            })
            .collect();
        assert_eq!(messages, ["old-1", "old-2", "new"]);
        assert!(requests.iter().all(|request| {
            request
                .body_json::<serde_json::Value>()
                .ok()
                .and_then(|body| {
                    body.get("params")?
                        .get("account")?
                        .as_str()
                        .map(str::to_owned)
                })
                .as_deref()
                == Some("+1111111111")
        }));
        let current = state.lock().await;
        assert_eq!(current.state, ConnectionState::Connected);
        assert_eq!(current.buffered_count(), 0);
    }

    #[tokio::test]
    async fn ambiguous_poison_does_not_block_fifo_or_taint_new_result() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let response_calls = Arc::clone(&calls);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(move |request: &wiremock::Request| {
                if response_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    wiremock::ResponseTemplate::new(204)
                } else {
                    send_response(request, &["SUCCESS"])
                }
            })
            .expect(3)
            .mount(&server)
            .await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let state = Arc::clone(&provider.accounts.get("primary").expect("account").state);
        {
            let mut current = state.lock().await;
            for text in ["old-1", "old-2"] {
                assert_eq!(
                    current.enqueue(SignalProvider::build_send_params(
                        Some("+1111111111"),
                        &channel_params(text),
                    )),
                    EnqueueDisposition::Queued
                );
            }
        }

        let result = provider.send(&channel_params("new")).await;
        assert!(result.sent, "old ambiguity must not taint the new send");
        let current = state.lock().await;
        assert_eq!(current.buffered_count(), 0);
        assert_eq!(current.ambiguous_delivery_count, 1);
        drop(current);
        let requests = server.received_requests().await.expect("requests");
        let messages: Vec<_> = requests
            .iter()
            .filter_map(|request| request.body_json::<serde_json::Value>().ok())
            .filter_map(|body| {
                body.get("params")?
                    .get("message")?
                    .as_str()
                    .map(ToOwned::to_owned)
            })
            .collect();
        assert_eq!(messages, ["old-1", "old-2", "new"]);
    }

    #[tokio::test]
    async fn rejected_poison_is_removed_and_later_fifo_work_continues() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let response_calls = Arc::clone(&calls);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(move |request: &wiremock::Request| {
                if response_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    send_response(request, &["UNREGISTERED_FAILURE"])
                } else {
                    send_response(request, &["SUCCESS"])
                }
            })
            .expect(3)
            .mount(&server)
            .await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let state = Arc::clone(&provider.accounts.get("primary").expect("account").state);
        {
            let mut current = state.lock().await;
            for text in ["old-1", "old-2"] {
                assert_eq!(
                    current.enqueue(SignalProvider::build_send_params(
                        Some("+1111111111"),
                        &channel_params(text),
                    )),
                    EnqueueDisposition::Queued
                );
            }
        }
        let result = provider.send(&channel_params("new")).await;
        assert!(result.sent, "old rejection must not taint the new send");
        let current = state.lock().await;
        assert_eq!(current.buffered_count(), 0);
        assert_eq!(current.dropped_count, 1);
        drop(current);
        let requests = server.received_requests().await.expect("requests");
        let messages: Vec<_> = requests
            .iter()
            .filter_map(|request| request.body_json::<serde_json::Value>().ok())
            .filter_map(|body| {
                body.get("params")?
                    .get("message")?
                    .as_str()
                    .map(ToOwned::to_owned)
            })
            .collect();
        assert_eq!(messages, ["old-1", "old-2", "new"]);
    }

    #[tokio::test]
    async fn probe_is_observational_and_never_drains_user_content() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v1/check"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let state = Arc::clone(&provider.accounts.get("primary").expect("account").state);
        {
            let mut current = state.lock().await;
            current.state = ConnectionState::Reconnecting { attempt: 2 };
            assert_eq!(
                current.enqueue(SignalProvider::build_send_params(
                    Some("+1111111111"),
                    &channel_params("queued"),
                )),
                EnqueueDisposition::Queued
            );
        }
        assert!(provider.probe().await.ok);
        let current = state.lock().await;
        assert_eq!(current.state, ConnectionState::Reconnecting { attempt: 2 });
        assert_eq!(current.buffered_count(), 1);
        drop(current);
        let requests = server.received_requests().await.expect("requests");
        assert!(
            requests
                .iter()
                .all(|request| request.method.as_str() == "GET")
        );
    }

    #[tokio::test]
    async fn zero_capacity_never_claims_a_failed_send_was_buffered() {
        install_crypto_provider();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        drop(listener);
        let mut provider = SignalProvider::with_buffer_capacity(0);
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&format!("http://{address}")).expect("client"),
            false,
        );
        let state = Arc::clone(&provider.accounts.get("primary").expect("account").state);
        state.lock().await.state = ConnectionState::Reconnecting { attempt: 1 };
        let result = provider.send(&channel_params("drop me")).await;
        assert_eq!(
            result.error.as_deref(),
            Some("signal_buffer_disabled_dropped")
        );
        let current = state.lock().await;
        assert_eq!(current.buffered_count(), 0);
        assert_eq!(current.dropped_count, 1);
    }

    #[tokio::test]
    async fn ambiguous_send_is_not_requeued() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );

        let result = provider.send(&channel_params("uncertain")).await;
        assert!(!result.sent);
        assert_eq!(result.error.as_deref(), Some("signal_delivery_ambiguous"));
        let state = provider
            .accounts
            .get("primary")
            .expect("account")
            .state
            .lock()
            .await;
        assert_eq!(state.buffered_count(), 0);
        assert_eq!(state.ambiguous_delivery_count, 1);
    }

    #[tokio::test]
    async fn partial_delivery_is_never_retried_and_has_a_dedicated_counter() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(|request: &wiremock::Request| {
                send_response(request, &["SUCCESS", "IDENTITY_FAILURE"])
            })
            .expect(1)
            .mount(&server)
            .await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let result = provider.send(&channel_params("partial")).await;
        assert_eq!(result.error.as_deref(), Some("signal_delivery_partial"));
        let state = provider
            .accounts
            .get("primary")
            .expect("account")
            .state
            .lock()
            .await;
        assert_eq!(state.partial_delivery_count, 1);
        assert_eq!(state.ambiguous_delivery_count, 0);
        assert_eq!(state.buffered_count(), 0);
    }

    #[tokio::test]
    async fn cancelling_direct_send_records_ambiguity_without_replay() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        let request_seen = Arc::new(tokio::sync::Notify::new());
        let notify = Arc::clone(&request_seen);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(move |request: &wiremock::Request| {
                notify.notify_one();
                send_response(request, &["SUCCESS"]).set_delay(Duration::from_millis(200))
            })
            .expect(1)
            .mount(&server)
            .await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let provider = Arc::new(provider);
        let state = Arc::clone(&provider.accounts.get("primary").expect("account").state);
        let task_provider = Arc::clone(&provider);
        let handle = tokio::spawn(async move {
            task_provider
                .send(&channel_params("cancelled-direct"))
                .await
        });
        request_seen.notified().await;
        handle.abort();
        assert!(handle.await.expect_err("send task aborted").is_cancelled());
        let current = state.lock().await;
        assert_eq!(current.ambiguous_delivery_count, 1);
        assert_eq!(current.buffered_count(), 0);
    }

    #[tokio::test]
    async fn cancelling_buffer_drain_removes_inflight_item_before_later_send() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let response_calls = Arc::clone(&calls);
        let request_seen = Arc::new(tokio::sync::Notify::new());
        let notify = Arc::clone(&request_seen);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(move |request: &wiremock::Request| {
                if response_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    notify.notify_one();
                    send_response(request, &["SUCCESS"]).set_delay(Duration::from_millis(200))
                } else {
                    send_response(request, &["SUCCESS"])
                }
            })
            .expect(2)
            .mount(&server)
            .await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let provider = Arc::new(provider);
        let state = Arc::clone(&provider.accounts.get("primary").expect("account").state);
        assert_eq!(
            state
                .lock()
                .await
                .enqueue(SignalProvider::build_send_params(
                    Some("+1111111111"),
                    &channel_params("old-inflight"),
                )),
            EnqueueDisposition::Queued
        );
        let task_provider = Arc::clone(&provider);
        let handle =
            tokio::spawn(async move { task_provider.send(&channel_params("cancelled-new")).await });
        request_seen.notified().await;
        handle.abort();
        assert!(handle.await.expect_err("drain task aborted").is_cancelled());
        assert!(provider.send(&channel_params("later")).await.sent);

        let current = state.lock().await;
        assert_eq!(current.ambiguous_delivery_count, 1);
        assert_eq!(current.buffered_count(), 0);
        drop(current);
        let requests = server.received_requests().await.expect("requests");
        let messages: Vec<_> = requests
            .iter()
            .filter_map(|request| request.body_json::<serde_json::Value>().ok())
            .filter_map(|body| {
                body.get("params")?
                    .get("message")?
                    .as_str()
                    .map(ToOwned::to_owned)
            })
            .collect();
        assert_eq!(messages, ["old-inflight", "later"]);
    }

    #[tokio::test]
    async fn aborting_poll_during_destructive_receive_records_explicit_loss() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        let request_seen = Arc::new(tokio::sync::Notify::new());
        let notify = Arc::clone(&request_seen);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(move |request: &wiremock::Request| {
                notify.notify_one();
                let request_body: serde_json::Value = request.body_json().expect("JSON");
                wiremock::ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(200))
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request_body.get("id").cloned().expect("id"),
                        "result": []
                    }))
            })
            .expect(1)
            .mount(&server)
            .await;
        let state = Arc::new(Mutex::new(AccountState::new(10)));
        let (tx, _rx) = mpsc::channel(1);
        let handle = tokio::spawn(poll_loop(
            client::SignalClient::new(&server.uri()).expect("client"),
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            tx,
            Duration::from_mins(1),
            Arc::clone(&state),
            CancellationToken::new(),
            CIRCUIT_BREAKER_THRESHOLD,
            HALTED_HEALTH_CHECK_INTERVAL,
        ));
        request_seen.notified().await;
        handle.abort();
        assert!(handle.await.expect_err("poll task aborted").is_cancelled());
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if state.lock().await.receive_loss_count == 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached loss accounting");
    }

    #[tokio::test]
    async fn outbound_media_is_rejected_before_network() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&server.uri()).expect("client"),
            false,
        );
        let mut params = channel_params("media");
        params.attachments = Some(vec!["/private/file".to_owned()]);
        let result = provider.send(&params).await;
        assert_eq!(result.error.as_deref(), Some("signal_media_unsupported"));
        assert!(
            server
                .received_requests()
                .await
                .expect("requests")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn connect_failure_enters_reconnecting_state_and_buffers_once() {
        install_crypto_provider();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let address = listener.local_addr().expect("local address");
        drop(listener);

        let mut provider = SignalProvider::new();
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            client::SignalClient::new(&format!("http://{address}")).expect("client"),
            false,
        );
        let result = provider.send(&channel_params("buffer me")).await;
        assert!(!result.sent);
        let state = provider
            .accounts
            .get("primary")
            .expect("account")
            .state
            .lock()
            .await;
        assert_eq!(state.state, ConnectionState::Reconnecting { attempt: 1 });
        assert_eq!(state.buffered_count(), 1);
    }

    #[tokio::test]
    async fn connection_health_reports_state() {
        install_crypto_provider();
        let mut provider = SignalProvider::with_buffer_capacity(50);
        let server = wiremock::MockServer::start().await;
        let signal_client = client::SignalClient::new(&server.uri()).expect("client");
        provider.add_account(
            "primary".to_owned(),
            Some("+1111111111".to_owned()),
            signal_client,
            true,
        );

        let health = provider.connection_health().await;
        let key = crate::redact::opaque_identifier("signal-account", "primary");
        let report = health.get(&key).expect("logical account present (opaque)");
        assert_eq!(report.state, ConnectionState::Connected);
        assert_eq!(report.buffered_messages, 0);
        assert_eq!(report.dropped_count, 0);
        assert_eq!(report.ambiguous_delivery_count, 0);
        assert_eq!(report.partial_delivery_count, 0);
        assert_eq!(report.receive_loss_count, 0);
        assert!(
            !health.contains_key("+1111111111"),
            "health payload must not carry raw phone numbers"
        );
    }
}
