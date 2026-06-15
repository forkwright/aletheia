//! Connection service: manages the lifecycle of a connection to a pylon instance.
//!
//! Handles initial connection, health checking, and reconnection with
//! exponential backoff. Communicates state changes via an mpsc channel so
//! that Dioxus coroutines can read and write to signals on the UI thread.
//!
//! # Architecture
//!
//! ```text
//!   ConnectionConfig ──► ConnectionService::run()
//!                              │
//!                              ├─► health check (GET /api/health)
//!                              │       │
//!                              │   Connected ──► periodic health check (30s)
//!                              │                        │
//!                              │              sustained loss confirmed
//!                              │              (2+ failures spanning 15s;
//!                              │               single blips stay silent)
//!                              │                        │
//!                              │                  Reconnecting(n)
//!                              │                        │
//!                              │                  backoff → retry
//!                              │                        │
//!                              └─► Failed (auth error, max retries, etc.)
//! ```
//!
//! # Minimal API client
//!
//! This module includes a minimal HTTP client for server communication.

use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use snafu::{ResultExt, Snafu};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::state::connection::{
    ConnectionConfig, ConnectionState, HEALTH_CHECK_INTERVAL, backoff_duration,
};

/// Consecutive health-check failures required before the loss is reported.
const LOSS_CONFIRM_FAILURES: u32 = 2;

/// Minimum elapsed time since the first failure before the loss is reported.
const LOSS_CONFIRM_WINDOW: Duration = Duration::from_secs(15);

#[derive(Debug, Snafu)]
#[non_exhaustive]
/// Errors from connection attempts to a pylon server.
pub enum ConnectionError {
    /// Health check request failed.
    #[snafu(display("health check failed: {source}"))]
    HealthCheck {
        /// Underlying HTTP error.
        source: reqwest::Error,
    },

    /// Server responded but reported unhealthy status.
    #[snafu(display("server returned unhealthy status: {status}"))]
    Unhealthy {
        /// HTTP status code returned.
        status: u16,
    },

    /// Connection attempt exceeded the configured timeout.
    #[snafu(display("connection timed out after {timeout_secs}s"))]
    Timeout {
        /// Configured timeout in seconds.
        timeout_secs: u64,
    },

    /// Auth token contains non-ASCII characters.
    #[snafu(display("invalid auth token: contains non-ASCII characters"))]
    InvalidToken,

    /// Failed to construct the reqwest client.
    #[snafu(display("failed to build HTTP client: {source}"))]
    ClientBuild {
        /// Underlying HTTP error.
        source: reqwest::Error,
    },
}

/// Minimal HTTP client for pylon communication.
///
/// Wraps `reqwest::Client` with pre-configured auth headers and base URL.
#[derive(Clone)]
pub struct PylonClient {
    client: reqwest::Client,
    base_url: String,
}

impl PylonClient {
    /// Build a new client for the given config.
    ///
    /// # Errors
    ///
    /// Returns `InvalidToken` if the auth token contains non-ASCII characters.
    /// Returns `ClientBuild` if the reqwest client cannot be constructed.
    pub(crate) fn new(config: &ConnectionConfig) -> Result<Self, ConnectionError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        // CSRF mitigation: documented default bootstrap header for pylon.
        headers.insert("x-requested-with", HeaderValue::from_static("aletheia"));

        if let Some(ref token) = config.auth_token {
            let value = format!("Bearer {token}");
            // SAFETY: log only the error kind, not the token value.
            let header_value = HeaderValue::from_str(&value).map_err(|e| {
                tracing::debug!(kind = %e, "auth token contains invalid header characters"); // kanon:ignore SECURITY/credential-logging -- logs only the error kind, not the token
                ConnectionError::InvalidToken
            })?;
            headers.insert(AUTHORIZATION, header_value);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .cookie_store(true)
            .build()
            .context(ClientBuildSnafu)?;

        Ok(Self {
            client,
            base_url: config.server_url.trim_end_matches('/').to_string(),
        })
    }

    /// Check server reachability via `GET /api/health`.
    ///
    /// Returns `Ok(())` if the server responds with a 2xx status.
    pub async fn health(&self) -> Result<(), ConnectionError> {
        let url = format!("{}/api/health", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context(HealthCheckSnafu)?;

        if resp.status().is_success() {
            Ok(())
        } else {
            UnhealthySnafu {
                status: resp.status().as_u16(),
            }
            .fail()
        }
    }

    /// The base URL this client is configured for.
    #[must_use]
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl std::fmt::Debug for PylonClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PylonClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// Manages the connection lifecycle as a background task.
///
/// Call [`ConnectionService::run`] to start the connection loop. It will:
/// 1. Attempt initial connection via health check
/// 2. On success: enter periodic health check loop (every 30s)
/// 3. On failure: retry with exponential backoff (1s to 30s)
/// 4. Report all state transitions via the mpsc sender
///
/// The service respects `CancellationToken` for clean shutdown.
///
/// State changes are sent through an unbounded mpsc channel so a Dioxus
/// coroutine can receive them on the UI thread and write to signals.
pub(crate) struct ConnectionService {
    config: ConnectionConfig,
    cancel: CancellationToken,
    tx: mpsc::UnboundedSender<ConnectionState>,
}

impl ConnectionService {
    /// Create a new connection service.
    #[must_use]
    pub(crate) fn new(
        config: ConnectionConfig,
        cancel: CancellationToken,
        tx: mpsc::UnboundedSender<ConnectionState>,
    ) -> Self {
        Self { config, cancel, tx }
    }

    /// Send a state update. Silently drops if the receiver has been closed.
    fn emit(&self, state: ConnectionState) {
        if self.tx.send(state).is_err() {
            tracing::debug!("connection state receiver closed");
        }
    }

    /// Run the connection loop until cancelled or timed out.
    ///
    /// The overall connection phase (all retries combined) is bounded by
    /// `ConnectionConfig::connect_timeout_secs` (default 30s). If the
    /// deadline elapses without a successful health check, the service
    /// emits `ConnectionState::TimedOut` so the UI can offer a retry button.
    ///
    /// This is designed to be spawned as a background task:
    /// ```ignore
    /// use tracing::Instrument;
    /// let (tx, rx) = mpsc::unbounded_channel();
    /// let svc = ConnectionService::new(config, cancel.clone(), tx);
    /// tokio::spawn(svc.run().instrument(tracing::info_span!("connection_service")));
    /// // rx.recv() in a Dioxus coroutine
    /// ```
    pub async fn run(self) {
        let client = match PylonClient::new(&self.config) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to build pylon client: {e}");
                self.emit(ConnectionState::Failed {
                    reason: e.to_string(),
                });
                return;
            }
        };

        // Initial connection attempt with overall timeout.
        self.emit(ConnectionState::Connecting);
        let deadline = tokio::time::sleep(self.config.connect_timeout());
        tokio::pin!(deadline);
        let mut attempt: u32 = 0;

        loop {
            if self.cancel.is_cancelled() {
                return;
            }

            attempt = attempt.saturating_add(1);

            // Race the health check against cancellation and the overall deadline.
            let health_result = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return,
                _ = &mut deadline => {
                    tracing::warn!(
                        timeout_secs = self.config.connect_timeout_secs,
                        attempts = attempt,
                        "connection timed out"
                    );
                    self.emit(ConnectionState::TimedOut);
                    return;
                }
                result = client.health() => result,
            };

            match health_result {
                Ok(()) => {
                    tracing::info!(base_url = client.base_url(), "connected to pylon");
                    self.emit(ConnectionState::Connected);
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        attempt,
                        error = %e,
                        "connection attempt failed"
                    );
                    self.emit(ConnectionState::Reconnecting { attempt });

                    let delay = backoff_duration(attempt);
                    tokio::select! {
                        biased;
                        _ = self.cancel.cancelled() => return,
                        _ = &mut deadline => {
                            tracing::warn!(
                                timeout_secs = self.config.connect_timeout_secs,
                                attempts = attempt,
                                "connection timed out during backoff"
                            );
                            self.emit(ConnectionState::TimedOut);
                            return;
                        }
                        // NOTE: backoff elapsed, retry connection
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }

        // Connected: enter periodic health check loop.
        self.health_check_loop(&client).await;
    }

    /// Periodically verify the connection is still alive.
    ///
    /// On failure, attempts reconnection with exponential backoff. A loss is
    /// reported to the UI only once confirmed ([`LOSS_CONFIRM_FAILURES`]
    /// consecutive failures spanning [`LOSS_CONFIRM_WINDOW`]); single blips
    /// recover silently without flipping connection state.
    async fn health_check_loop(&self, client: &PylonClient) {
        let mut loss = LossTracker::default();

        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return,
                // NOTE: interval elapsed, proceed to health check
                _ = tokio::time::sleep(HEALTH_CHECK_INTERVAL) => {}
            }

            match client.health().await {
                Ok(()) => {
                    if loss.failures > 0 {
                        tracing::info!("connection restored after {} failures", loss.failures);
                        self.emit_recovery(&mut loss);
                    }
                }
                Err(e) => {
                    loss.record_failure();
                    tracing::warn!(
                        attempt = loss.failures,
                        error = %e,
                        "health check failed"
                    );

                    if !self.config.auto_reconnect {
                        self.emit(ConnectionState::Failed {
                            reason: e.to_string(),
                        });
                        return;
                    }

                    self.report_loss_if_confirmed(&mut loss);

                    // Attempt reconnection with backoff.
                    if !self.try_reconnect(client, &mut loss).await {
                        return;
                    }
                }
            }
        }
    }

    /// Attempt reconnection with exponential backoff.
    ///
    /// Returns `true` if reconnected or should keep trying, `false` if cancelled.
    async fn try_reconnect(&self, client: &PylonClient, loss: &mut LossTracker) -> bool {
        for _ in 0..5 {
            let delay = backoff_duration(loss.failures);
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return false,
                // NOTE: backoff elapsed, retry connection
                _ = tokio::time::sleep(delay) => {}
            }

            match client.health().await {
                Ok(()) => {
                    tracing::info!("reconnected to pylon");
                    self.emit_recovery(loss);
                    return true;
                }
                Err(e) => {
                    loss.record_failure();
                    tracing::warn!(
                        attempt = loss.failures,
                        error = %e,
                        "reconnection attempt failed"
                    );
                    self.report_loss_if_confirmed(loss);
                }
            }
        }

        true
    }

    /// Emit `Reconnecting` only once the loss is confirmed.
    ///
    /// WHY: `Reconnecting` unmounts the connected UI (`needs_connect_view`),
    /// so a single health-check blip must never reach the signal — only a
    /// sustained loss (consecutive failures spanning the confirm window).
    fn report_loss_if_confirmed(&self, loss: &mut LossTracker) {
        if loss.confirmed() {
            loss.reported = true;
            self.emit(ConnectionState::Reconnecting {
                attempt: loss.failures,
            });
        }
    }

    /// Reset loss tracking; emit `Connected` only if a loss was reported.
    ///
    /// A silently-recovered blip leaves the signal untouched (still
    /// `Connected`), so no state flip or notification fires.
    fn emit_recovery(&self, loss: &mut LossTracker) {
        if loss.reported {
            self.emit(ConnectionState::Connected);
        }
        *loss = LossTracker::default();
    }
}

/// Tracks an in-progress connection loss for confirm-before-report.
#[derive(Default)]
struct LossTracker {
    /// Consecutive health-check failures (0 = healthy).
    failures: u32,
    /// When the first failure of the current run occurred.
    first_failure_at: Option<Instant>,
    /// Whether `Reconnecting` has been emitted for this run.
    reported: bool,
}

impl LossTracker {
    fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
        self.first_failure_at.get_or_insert_with(Instant::now);
    }

    /// Both gates must hold: enough consecutive failures AND enough elapsed
    /// time since the run began.
    fn confirmed(&self) -> bool {
        self.failures >= LOSS_CONFIRM_FAILURES
            && self
                .first_failure_at
                .is_some_and(|t| t.elapsed() >= LOSS_CONFIRM_WINDOW)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn pylon_client_default_config() {
        install_crypto();
        let config = ConnectionConfig::default();
        let client = PylonClient::new(&config).unwrap();
        let port = skene::discovery::DiscoveryConfig::default().port;
        assert_eq!(client.base_url(), format!("http://localhost:{port}"));
    }

    #[test]
    fn pylon_client_trims_trailing_slash() {
        install_crypto();
        let config = ConnectionConfig {
            server_url: "http://localhost:3000/".to_string(),
            ..ConnectionConfig::default()
        };
        let client = PylonClient::new(&config).unwrap();
        assert_eq!(client.base_url(), "http://localhost:3000");
    }

    #[test]
    fn pylon_client_invalid_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let result = PylonClient::new(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid auth token"));
    }

    #[test]
    fn pylon_client_debug_redacts() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("secret".to_string()),
            ..ConnectionConfig::default()
        };
        let client = PylonClient::new(&config).unwrap();
        let debug = format!("{client:?}");
        assert!(!debug.contains("secret"));
        assert!(debug.contains("base_url"));
    }

    #[test]
    fn connection_error_display() {
        let err = ConnectionError::InvalidToken;
        assert_eq!(
            err.to_string(),
            "invalid auth token: contains non-ASCII characters"
        );

        let err = ConnectionError::Unhealthy { status: 503 };
        assert_eq!(err.to_string(), "server returned unhealthy status: 503");
    }

    #[tokio::test]
    async fn service_emits_failed_on_bad_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let cancel = CancellationToken::new();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let svc = ConnectionService::new(config, cancel, tx);
        svc.run().await;

        let state = rx.recv().await.unwrap();
        assert!(matches!(state, ConnectionState::Failed { .. }));
    }

    #[tokio::test]
    async fn connection_error_health_check_display() {
        install_crypto();
        // Build a real reqwest::Error by failing on an unreachable URL.
        let client = reqwest::Client::new();
        let result = client.get("http://127.0.0.1:1").send().await;
        if let Err(e) = result {
            let err = ConnectionError::HealthCheck { source: e };
            assert!(err.to_string().contains("health check failed"));
        }
    }

    #[test]
    fn connection_error_timeout_display() {
        let err = ConnectionError::Timeout { timeout_secs: 30 };
        assert!(err.to_string().contains("30s"));
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn pylon_client_with_token_succeeds() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("good-token".to_string()),
            ..ConnectionConfig::default()
        };
        let client = PylonClient::new(&config).unwrap();
        let port = skene::discovery::DiscoveryConfig::default().port;
        assert_eq!(client.base_url(), format!("http://localhost:{port}"));
    }

    /// Spawns a minimal HTTP server on an ephemeral port that responds with
    /// the configured status code and body for any request.
    ///
    /// Returns (port, server_task_handle). The server handles a single
    /// request, then exits.
    async fn spawn_test_server(status: u16, body: &'static str) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            // Accept multiple sequential connections so health-check loops work.
            for _ in 0..32 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                // Best-effort read of request preamble.
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let reason = match status {
                    200 => "OK",
                    503 => "Service Unavailable",
                    500 => "Internal Server Error",
                    _ => "OK",
                };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        port
    }

    #[tokio::test]
    async fn pylon_client_health_succeeds_against_mock_server() {
        install_crypto();
        let port = spawn_test_server(200, "ok").await;
        let config = ConnectionConfig {
            server_url: format!("http://127.0.0.1:{port}"),
            ..ConnectionConfig::default()
        };
        let client = PylonClient::new(&config).unwrap();
        let result = client.health().await;
        assert!(result.is_ok(), "health check must succeed: {result:?}");
    }

    #[tokio::test]
    async fn pylon_client_health_returns_unhealthy_on_5xx() {
        install_crypto();
        let port = spawn_test_server(503, "down").await;
        let config = ConnectionConfig {
            server_url: format!("http://127.0.0.1:{port}"),
            ..ConnectionConfig::default()
        };
        let client = PylonClient::new(&config).unwrap();
        let result = client.health().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ConnectionError::Unhealthy { status } => assert_eq!(status, 503),
            other => panic!("expected Unhealthy, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn service_emits_connected_against_mock_server() {
        install_crypto();
        let port = spawn_test_server(200, "ok").await;
        let config = ConnectionConfig {
            server_url: format!("http://127.0.0.1:{port}"),
            ..ConnectionConfig::default()
        };
        let cancel = CancellationToken::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let svc_cancel = cancel.clone();
        let svc = ConnectionService::new(config, svc_cancel, tx);
        let handle = tokio::spawn(svc.run());

        // Drain states until we observe Connected or timeout.
        let mut saw_connecting = false;
        let mut saw_connected = false;
        for _ in 0..20 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(ConnectionState::Connecting)) => saw_connecting = true,
                Ok(Some(ConnectionState::Connected)) => {
                    saw_connected = true;
                    break;
                }
                Ok(Some(_other)) => {}
                Ok(None) => break,
                Err(_) => break,
            }
        }
        cancel.cancel();
        let _ = handle.await;
        assert!(saw_connecting, "must transition through Connecting");
        assert!(saw_connected, "must reach Connected against healthy server");
    }

    #[test]
    fn loss_tracker_not_confirmed_on_single_failure() {
        let mut loss = LossTracker::default();
        loss.record_failure();
        assert!(!loss.confirmed(), "one blip must not confirm a loss");
    }

    #[test]
    fn loss_tracker_not_confirmed_within_window() {
        let mut loss = LossTracker::default();
        loss.record_failure();
        loss.record_failure();
        // Two failures, but the confirm window has not elapsed.
        assert!(!loss.confirmed());
    }

    #[test]
    fn loss_tracker_confirmed_past_both_gates() {
        let mut loss = LossTracker::default();
        loss.record_failure();
        loss.record_failure();
        loss.first_failure_at = Instant::now().checked_sub(LOSS_CONFIRM_WINDOW);
        if loss.first_failure_at.is_some() {
            assert!(loss.confirmed());
        }
    }

    #[tokio::test]
    async fn unconfirmed_loss_emits_nothing() {
        install_crypto();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let svc = ConnectionService::new(ConnectionConfig::default(), CancellationToken::new(), tx);

        let mut loss = LossTracker::default();
        loss.record_failure();
        svc.report_loss_if_confirmed(&mut loss);

        assert!(!loss.reported);
        assert!(
            rx.try_recv().is_err(),
            "no state must be emitted for a blip"
        );
    }

    #[tokio::test]
    async fn confirmed_loss_emits_reconnecting_and_recovery_emits_connected() {
        install_crypto();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let svc = ConnectionService::new(ConnectionConfig::default(), CancellationToken::new(), tx);

        let mut loss = LossTracker::default();
        loss.record_failure();
        loss.record_failure();
        loss.first_failure_at = Instant::now().checked_sub(LOSS_CONFIRM_WINDOW);
        if loss.first_failure_at.is_none() {
            return; // NOTE: process younger than the window; skip rather than flake.
        }

        svc.report_loss_if_confirmed(&mut loss);
        assert!(loss.reported);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ConnectionState::Reconnecting { attempt: 2 }
        ));

        svc.emit_recovery(&mut loss);
        assert!(matches!(rx.try_recv().unwrap(), ConnectionState::Connected));
        assert_eq!(loss.failures, 0);
        assert!(!loss.reported);
    }

    #[tokio::test]
    async fn silent_recovery_emits_nothing() {
        install_crypto();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let svc = ConnectionService::new(ConnectionConfig::default(), CancellationToken::new(), tx);

        let mut loss = LossTracker::default();
        loss.record_failure();
        svc.emit_recovery(&mut loss);

        assert!(
            rx.try_recv().is_err(),
            "recovery from an unreported blip must stay silent"
        );
        assert_eq!(loss.failures, 0);
    }

    #[tokio::test]
    async fn service_respects_cancellation_during_connect() {
        install_crypto();
        // No server bound — connection will fail and retry. Cancel before
        // the deadline elapses.
        let config = ConnectionConfig {
            // Use an unreachable port to force connection failures.
            server_url: "http://127.0.0.1:1".to_string(),
            connect_timeout_secs: 60,
            ..ConnectionConfig::default()
        };
        let cancel = CancellationToken::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let svc = ConnectionService::new(config, cancel.clone(), tx);
        let handle = tokio::spawn(svc.run());

        // Let it emit Connecting, then cancel.
        tokio::time::sleep(Duration::from_millis(50)).await; // kanon:ignore TESTING/sleep-in-test -- real retry loop cancellation requires a brief scheduler window (#3988)
        cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("task must exit promptly after cancel");

        // Drain whatever was sent; first should be Connecting.
        if let Ok(Some(state)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
            assert!(matches!(state, ConnectionState::Connecting));
        }
    }
}
