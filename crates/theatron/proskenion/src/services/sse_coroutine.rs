//! SSE coroutine service for Dioxus signal wiring.
//!
//! Bridges the global SSE connection to Dioxus reactive state. Runs as a
//! Dioxus coroutine (via `spawn`) that reads events from
//! [`SseConnection`](crate::api::sse::SseConnection) and updates signals
//! through [`SseEventRouter`](super::sse::SseEventRouter).

use dioxus::prelude::*;
use skene::api::types::SseEvent;
use tokio_util::sync::CancellationToken;

use crate::api::sse::SseConnection;
use crate::services::notification_dispatch::NotificationDispatch;
use crate::services::sse::SseEventRouter;
use crate::state::connection::ConnectionConfig;
use crate::state::events::{EventState, SseConnectionState};
use crate::state::notifications::{DndState, NotificationHistory, NotificationPreferences};
use crate::state::toasts::{ToastSeverity, ToastStore};

/// Provide SSE-derived state signals and start the SSE coroutine.
///
/// Call from the app root after connection is established. Provides
/// `Signal<EventState>` and `Signal<SseConnectionState>` as context.
///
/// Requires `Signal<NotificationPreferences>`, `Signal<NotificationHistory>`,
/// and `Signal<DndState>` to already be in context (provided by `ConnectedApp`).
///
/// The coroutine automatically reconnects when the SSE stream drops
/// (handled internally by `SseConnection`, which reports only confirmed
/// losses). Toast notifications fire only on sustained loss/recovery
/// transitions — transient blips and clean reconnects stay silent.
/// Desktop notifications are dispatched via [`NotificationDispatch`].
pub(crate) fn start_sse_coroutine(config: &ConnectionConfig) {
    let mut event_state = use_context_provider(|| Signal::new(EventState::new()));
    let mut sse_connection_state =
        use_context_provider(|| Signal::new(SseConnectionState::Disconnected));

    // Read the notification signals provided by ConnectedApp.
    let prefs_signal = use_context::<Signal<NotificationPreferences>>();
    let mut history_signal = use_context::<Signal<NotificationHistory>>();
    let dnd_signal = use_context::<Signal<DndState>>();

    let base_url = config.server_url.trim_end_matches('/').to_string();
    let cancel = CancellationToken::new();

    let client = match crate::api::client::authenticated_streaming_client(config) {
        Ok(client) => client,
        Err(err) => {
            crate::api::client::log_authenticated_client_error(&err);
            sse_connection_state.set(SseConnectionState::Disconnected);
            return;
        }
    };

    spawn(async move {
        let mut sse = SseConnection::connect(client, &base_url, cancel);
        let mut router = SseEventRouter::new();
        let mut dispatch = NotificationDispatch::new();
        let mut loss_announced = false;

        while let Some(event) = sse.next().await {
            if let SseEvent::StreamLagged { dropped } = &event {
                let (severity, message) = stream_lagged_toast(*dropped);
                if let Some(mut store) = try_consume_context::<Signal<ToastStore>>() {
                    store.write().push(severity, message);
                }
                continue;
            }

            let prev_connected = router.state().connection.is_connected();

            if router.apply(&event) {
                let new_state = router.state();

                // NOTE: Update the event state signal for components.
                event_state.set(new_state.clone());

                // NOTE: Update the SSE connection state signal separately
                // so the connection indicator can subscribe to just this.
                sse_connection_state.set(new_state.connection.clone());

                // WHY: Toast only on sustained transitions: the connection
                // task already debounces losses, and `connection_toast`
                // pairs them — one lost toast per episode, one restored
                // toast after, nothing on startup or silent reconnects.
                let now_connected = new_state.connection.is_connected();
                let toast = connection_toast(prev_connected, now_connected, &mut loss_announced);
                if let Some((severity, message)) = toast
                    && let Some(mut store) = try_consume_context::<Signal<ToastStore>>()
                {
                    store.write().push(severity, message);
                }
            }

            // NOTE: Window focus state defaults to false (always notify).
            // Full focus integration requires wiring Dioxus desktop window
            // events -- tracked separately.
            let prefs = prefs_signal.peek().clone();
            let dnd = dnd_signal.peek().clone();
            dispatch.process_event(
                &event,
                &prefs,
                &dnd,
                false,
                &mut |entry| history_signal.write().push(entry),
                &mut |sev, title| {
                    if let Some(mut store) = try_consume_context::<Signal<ToastStore>>() {
                        store.write().push(sev, title.to_string());
                    }
                },
            );
        }
    });
}

/// Decide whether a connection transition warrants a toast.
///
/// `loss_announced` pairs the toasts across calls: a restored toast fires
/// only after a lost toast, so the initial connect and silently-recovered
/// blips never produce notifications.
fn connection_toast(
    was_connected: bool,
    is_connected: bool,
    loss_announced: &mut bool,
) -> Option<(ToastSeverity, &'static str)> {
    match (was_connected, is_connected) {
        (true, false) => {
            *loss_announced = true;
            Some((ToastSeverity::Warning, "Server connection lost"))
        }
        (false, true) if *loss_announced => {
            *loss_announced = false;
            Some((ToastSeverity::Success, "Server connection restored"))
        }
        _ => None, // NOTE: steady states and the initial connect stay quiet
    }
}

fn stream_lagged_toast(dropped: u64) -> (ToastSeverity, String) {
    (
        ToastSeverity::Warning,
        format!("Stream lagged; {dropped} events dropped - resync required"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_connect_is_silent() {
        let mut announced = false;
        assert!(connection_toast(false, true, &mut announced).is_none());
        assert!(!announced);
    }

    #[test]
    fn stream_lagged_toast_warns_with_drop_count() {
        assert_eq!(
            stream_lagged_toast(7),
            (
                ToastSeverity::Warning,
                "Stream lagged; 7 events dropped - resync required".to_string()
            )
        );
    }

    #[test]
    fn sustained_loss_then_recovery_toasts_once_each() {
        let mut announced = false;

        let lost = connection_toast(true, false, &mut announced);
        assert_eq!(
            lost,
            Some((ToastSeverity::Warning, "Server connection lost"))
        );
        assert!(announced);

        // Repeated reconnect attempts while down: no further toasts.
        assert!(connection_toast(false, false, &mut announced).is_none());

        let restored = connection_toast(false, true, &mut announced);
        assert_eq!(
            restored,
            Some((ToastSeverity::Success, "Server connection restored"))
        );
        assert!(!announced);
    }

    #[test]
    fn steady_connected_is_silent() {
        let mut announced = false;
        assert!(connection_toast(true, true, &mut announced).is_none());
    }
}
