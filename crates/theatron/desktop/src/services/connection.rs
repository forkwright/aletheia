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
//!                              │                   failure detected
//!                              │                        │
//!                              │                  Reconnecting(1)
//!                              │                        │
//!                              │                  backoff → retry
//!                              │                        │
//!                              └─► Failed (auth error, max retries, etc.)
//! ```
//!
//! # Minimal API client
//!
//! This module includes a minimal HTTP client for server communication.
//! Once `theatron-core` exposes `ApiClient` (after P601 merges), this
//! should be replaced by `theatron_core::client::ApiClient`.

use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use snafu::{ResultExt, Snafu};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::state::connection::{
    ConnectionConfig, ConnectionState, HEALTH_CHECK_INTERVAL, backoff_duration,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum ConnectionError {
    #[snafu(display("health check failed: {source}"))]
    HealthCheck { source: reqwest::Error },

    #[snafu(display("server returned unhealthy status: {status}"))]
    Unhealthy { status: u16 },

    #[snafu(display("invalid auth token: contains non-ASCII characters"))]
    InvalidToken,

    #[snafu(display("failed to build HTTP client: {source}"))]
    ClientBuild { source: reqwest::Error },
}

// ---------------------------------------------------------------------------
// Minimal API client (replace with theatron-core::ApiClient after P601)
// ---------------------------------------------------------------------------

/// Minimal HTTP client for pylon communication.
///
/// Wraps `reqwest::Client` with pre-configured auth headers and base URL.
/// This is a temporary implementation: once theatron-core exposes `ApiClient`,
/// this should be replaced.
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
    pub fn new(config: &ConnectionConfig) -> Result<Self, ConnectionError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        // CSRF mitigation: pylon rejects requests without this header.
        headers.insert("x-requested-with", HeaderValue::from_static("aletheia"));

        if let Some(ref token) = config.auth_token {
            let value = format!("Bearer {token}");
            let header_value = HeaderValue::from_str(&value).map_err(|e| {
                tracing::debug!(error = %e, "auth token contains invalid header characters");
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

    /// The underlying reqwest client, for use by SSE and streaming layers.
    #[must_use]
    pub fn raw_client(&self) -> &reqwest::Client {
        &self.client
    }

    /// The base URL this client is configured for.
    #[must_use]
    pub fn base_url(&self) -> &str {
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

// ---------------------------------------------------------------------------
// Connection service
// ---------------------------------------------------------------------------

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
pub struct ConnectionService {
    config: ConnectionConfig,
    cancel: CancellationToken,
    tx: mpsc::UnboundedSender<ConnectionState>,
}

impl ConnectionService {
    #[must_use]
    pub fn new(
        config: ConnectionConfig,
        cancel: CancellationToken,
        tx: mpsc::UnboundedSender<ConnectionState>,
    ) -> Self {
        Self { config, cancel, tx }
    }

    /// Send a state update. Silently drops if the receiver has been closed.
    fn emit(&self, state: ConnectionState) {
        let _ = self.tx.send(state);
    }

    /// Run the connection loop until cancelled.
    ///
    /// This is designed to be spawned as a background task:
    /// ```ignore
    /// let (tx, rx) = mpsc::unbounded_channel();
    /// let svc = ConnectionService::new(config, cancel.clone(), tx);
    /// tokio::spawn(svc.run());
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

        // Initial connection attempt.
        self.emit(ConnectionState::Connecting);
        let mut attempt: u32 = 0;

        loop {
            if self.cancel.is_cancelled() {
                return;
            }

            attempt = attempt.saturating_add(1);

            match client.health().await {
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
    /// On failure, attempts reconnection with exponential backoff.
    async fn health_check_loop(&self, client: &PylonClient) {
        let mut consecutive_failures: u32 = 0;

        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return,
                // NOTE: interval elapsed, proceed to health check
                _ = tokio::time::sleep(HEALTH_CHECK_INTERVAL) => {}
            }

            match client.health().await {
                Ok(()) => {
                    if consecutive_failures > 0 {
                        tracing::info!(
                            "connection restored after {} failures",
                            consecutive_failures
                        );
                        self.emit(ConnectionState::Connected);
                        consecutive_failures = 0;
                    }
                }
                Err(e) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    tracing::warn!(
                        attempt = consecutive_failures,
                        error = %e,
                        "health check failed"
                    );

                    if !self.config.auto_reconnect {
                        self.emit(ConnectionState::Failed {
                            reason: e.to_string(),
                        });
                        return;
                    }

                    self.emit(ConnectionState::Reconnecting {
                        attempt: consecutive_failures,
                    });

                    // Attempt reconnection with backoff.
                    if !self.try_reconnect(client, &mut consecutive_failures).await {
                        return;
                    }
                }
            }
        }
    }

    /// Attempt reconnection with exponential backoff.
    ///
    /// Returns `true` if reconnected or should keep trying, `false` if cancelled.
    async fn try_reconnect(&self, client: &PylonClient, consecutive_failures: &mut u32) -> bool {
        for _ in 0..5 {
            let delay = backoff_duration(*consecutive_failures);
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return false,
                // NOTE: backoff elapsed, retry connection
                _ = tokio::time::sleep(delay) => {}
            }

            match client.health().await {
                Ok(()) => {
                    tracing::info!("reconnected to pylon");
                    self.emit(ConnectionState::Connected);
                    *consecutive_failures = 0;
                    return true;
                }
                Err(e) => {
                    *consecutive_failures = consecutive_failures.saturating_add(1);
                    tracing::warn!(
                        attempt = *consecutive_failures,
                        error = %e,
                        "reconnection attempt failed"
                    );
                    self.emit(ConnectionState::Reconnecting {
                        attempt: *consecutive_failures,
                    });
                }
            }
        }

        true
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
        assert_eq!(client.base_url(), "http://localhost:3000");
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
}
