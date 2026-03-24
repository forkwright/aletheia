//! SSE coroutine service for Dioxus signal wiring.
//!
//! Bridges the global SSE connection to Dioxus reactive state. Runs as a
//! Dioxus coroutine (via `spawn`) that reads events from
//! [`SseConnection`](crate::api::sse::SseConnection) and updates signals
//! through [`SseEventRouter`](super::sse::SseEventRouter).

use dioxus::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::api::sse::SseConnection;
use crate::services::notification_dispatch::NotificationDispatch;
use crate::services::sse::SseEventRouter;
use crate::state::connection::ConnectionConfig;
use crate::state::events::{EventState, SseConnectionState};
use crate::state::notifications::{DndState, NotificationHistory, NotificationPreferences};
use crate::state::toasts::{Severity, ToastStore};

/// Provide SSE-derived state signals and start the SSE coroutine.
///
/// Call from the app root after connection is established. Provides
/// `Signal<EventState>` and `Signal<SseConnectionState>` as context.
///
/// Requires `Signal<NotificationPreferences>`, `Signal<NotificationHistory>`,
/// and `Signal<DndState>` to already be in context (provided by `ConnectedApp`).
///
/// The coroutine automatically reconnects when the SSE stream drops
/// (handled internally by `SseConnection`). Toast notifications are
/// emitted on disconnect/reconnect transitions. Desktop notifications are
/// dispatched via [`NotificationDispatch`].
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

    // WHY: Build the HTTP client from the shared authenticated client helper
    // so auth headers are included in the SSE request.
    let client = crate::api::client::authenticated_client(config);

    spawn(async move {
        let mut sse = SseConnection::connect(client, &base_url, cancel);
        let mut router = SseEventRouter::new();
        let mut dispatch = NotificationDispatch::new();

        while let Some(event) = sse.next().await {
            let prev_connected = router.state().connection.is_connected();

            if router.apply(&event) {
                let new_state = router.state();

                // NOTE: Update the event state signal for components.
                event_state.set(new_state.clone());

                // NOTE: Update the SSE connection state signal separately
                // so the connection indicator can subscribe to just this.
                sse_connection_state.set(new_state.connection.clone());

                // WHY: Emit toasts on connection transitions so the user
                // has visibility into SSE health without watching the indicator.
                let now_connected = new_state.connection.is_connected();
                emit_connection_toasts(prev_connected, now_connected);
            }

            // Dispatch desktop notifications for relevant events.
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

/// Emit toast notifications on SSE connection state transitions.
fn emit_connection_toasts(was_connected: bool, is_connected: bool) {
    match (was_connected, is_connected) {
        (true, false) => {
            // NOTE: We need a mutable signal write, access via context.
            if let Some(mut store) = try_consume_context::<Signal<ToastStore>>() {
                store
                    .write()
                    .push(Severity::Warning, "Server connection lost");
            }
        }
        (false, true) => {
            if let Some(mut store) = try_consume_context::<Signal<ToastStore>>() {
                store
                    .write()
                    .push(Severity::Success, "Server connection restored");
            }
        }
        _ => {}
    }
}
