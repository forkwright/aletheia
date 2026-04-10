//! Native desktop notification dispatch via notify-rust.
//!
//! Uses the freedesktop.org D-Bus notification spec on Linux.
//! If the notification daemon is unavailable or the send fails, logs a warning
//! and returns `false` so the caller can fall back to in-app toasts.
//!
//! NOTE: Action buttons (e.g. "Show") are not wired to window navigation in
//! this release. The notification daemon will display the system default
//! close button only. Full action-click integration requires a thread waiting
//! on `NotificationHandle::wait_for_action` and a channel to the Dioxus event
//! loop -- tracked separately.

use tracing::warn;

use super::notifications::{NotificationPayload, NotificationUrgency};

/// Send a native desktop notification.
///
/// Returns `true` if the notification was accepted by the daemon, `false` on
/// any failure. Never panics. The caller should fall back to an in-app toast
/// when this returns `false`.
pub(crate) fn send_native(payload: &NotificationPayload) -> bool {
    match try_send(payload) {
        Ok(()) => true,
        Err(e) => {
            warn!(
                error = %e,
                title = %payload.title,
                "native notification failed — falling back to in-app toast"
            );
            false
        }
    }
}

fn try_send(payload: &NotificationPayload) -> Result<(), notify_rust::error::Error> {
    use notify_rust::{Notification, Timeout, Urgency};

    let urgency = match payload.urgency {
        NotificationUrgency::Low => Urgency::Low,
        NotificationUrgency::Normal => Urgency::Normal,
        NotificationUrgency::Critical => Urgency::Critical,
    };

    let timeout = match payload.timeout_secs {
        Some(secs) => Timeout::Milliseconds(secs * 1_000),
        None => Timeout::Never,
    };

    Notification::new()
        .summary(&payload.title)
        .body(&payload.body)
        .urgency(urgency)
        .timeout(timeout)
        .show()?;

    Ok(())
}
