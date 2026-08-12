//! Backend subsystem health poller for Dioxus signal wiring (#5315).
//!
//! Runs as a Dioxus coroutine (via `spawn`), mirroring
//! [`crate::services::sse_coroutine::start_sse_coroutine`]'s idiom: it
//! periodically fetches `GET /api/v1/system/status` and writes the reduced
//! result into `Signal<BackendHealthState>`. Kept as an independent loop
//! from the SSE coroutine -- the SSE stream and backend subsystem health are
//! separate failure domains that the global status indicator
//! ([`crate::components::connection_indicator`]) merges by worst severity
//! rather than conflating into one signal.

use std::time::Duration;

use dioxus::prelude::*;

use crate::api::system_status::fetch_system_status;
use crate::state::backend_health::BackendHealthState;
use crate::state::connection::ConnectionConfig;

/// Interval between backend health polls.
const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Provide `Signal<BackendHealthState>` and start the polling coroutine.
///
/// Call from `ConnectedApp` alongside `start_sse_coroutine`, after the
/// connection config is finalized. The signal starts at
/// [`BackendHealthState::Unknown`] and updates after each poll, including
/// on failure -- a stalled or unreachable status endpoint must not leave
/// the indicator on a stale "healthy" reading.
pub(crate) fn start_backend_health_coroutine(config: &ConnectionConfig) {
    let mut backend_health = use_context_provider(|| Signal::new(BackendHealthState::default()));
    let config = config.clone();

    spawn(async move {
        loop {
            let state = match fetch_system_status(&config).await {
                Ok(response) => response.to_backend_health(),
                Err(err) => {
                    tracing::warn!(error = %err, "backend health poll failed");
                    err.to_backend_health()
                }
            };
            backend_health.set(state);

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}
