//! Notification payload types and urgency levels.
//!
//! Defines the data shape for desktop notifications before they are handed
//! to the platform-specific sender in [`super::native_notify`].

/// Visual and behavioral urgency of a desktop notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationUrgency {
    /// Low urgency: informational, auto-dismisses quickly.
    Low,
    /// Normal urgency: connection status changes.
    Normal,
    /// Critical urgency: tool approval requests and errors — persistent.
    Critical,
}

/// A desktop notification ready to be dispatched.
#[derive(Debug, Clone)]
pub(crate) struct NotificationPayload {
    /// Title displayed in the notification header.
    pub title: String,
    /// Body text shown below the title.
    pub body: String,
    /// Urgency level controlling visual prominence and persistence.
    pub urgency: NotificationUrgency,
    /// Auto-dismiss timeout in seconds. `None` means the notification persists
    /// until the user dismisses it (used for tool approval).
    pub timeout_secs: Option<u32>,
}
