//! Notification preferences, history, and Do Not Disturb state.
//!
//! [`NotificationPreferences`] is persisted to the user config file.
//! [`DndState`] is ephemeral and resets on app restart.
//! [`NotificationHistory`] holds the last 50 sent notifications for a
//! future notification-center view.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Maximum entries retained in [`NotificationHistory`].
const MAX_HISTORY: usize = 50;

/// Category of notification, used for per-event toggles and rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum NotificationCategory {
    /// Agent turn completed (triggered by `TurnAfter` SSE event).
    AgentCompletion,
    /// Tool approval required.
    ///
    /// NOTE: Awaiting `ToolApprovalNeeded` global SSE event type. Currently
    /// not fired from the dispatch layer — placeholder for future wiring.
    ToolApproval,
    /// Agent error or tool failure (triggered by `ToolFailed` SSE event).
    Error,
    /// SSE connection lost or restored.
    ConnectionStatus,
}

/// Duration preset for activating Do Not Disturb mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DndDuration {
    /// Suppress notifications for 15 minutes.
    FifteenMinutes,
    /// Suppress notifications for 1 hour.
    OneHour,
    /// Suppress notifications for approximately 24 hours from activation.
    ///
    /// NOTE: Uses 24h from activation rather than true midnight for simplicity.
    UntilTomorrow,
}

impl DndDuration {
    /// Convert to a concrete [`Duration`] for computing an expiry [`Instant`].
    #[must_use]
    pub(crate) fn as_duration(self) -> Duration {
        match self {
            Self::FifteenMinutes => Duration::from_secs(15 * 60),
            Self::OneHour => Duration::from_secs(60 * 60),
            Self::UntilTomorrow => Duration::from_secs(24 * 60 * 60),
        }
    }

    /// Human-readable label for UI display.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::FifteenMinutes => "15 minutes",
            Self::OneHour => "1 hour",
            Self::UntilTomorrow => "Until tomorrow",
        }
    }
}

/// Ephemeral Do Not Disturb state — resets on app restart (not persisted).
///
/// Activation time and expiry are [`Instant`]-based so they cannot be
/// serialized. Store this in a separate signal from [`NotificationPreferences`].
#[derive(Debug, Clone)]
pub(crate) struct DndState {
    /// Whether DND mode is currently active.
    pub active: bool,
    /// When DND mode expires. `None` means DND is indefinite until deactivated.
    pub expires_at: Option<Instant>,
}

impl Default for DndState {
    fn default() -> Self {
        Self {
            active: false,
            expires_at: None,
        }
    }
}

impl DndState {
    /// Whether DND is currently suppressing notifications.
    ///
    /// Returns `false` if DND is inactive or its expiry has passed.
    #[must_use]
    pub(crate) fn is_suppressing(&self) -> bool {
        if !self.active {
            return false;
        }
        match self.expires_at {
            Some(expires) => Instant::now() < expires,
            None => true,
        }
    }

    /// Activate DND for a preset duration.
    pub(crate) fn activate(&mut self, duration: DndDuration) {
        self.active = true;
        self.expires_at = Some(Instant::now() + duration.as_duration());
    }

    /// Deactivate DND immediately.
    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.expires_at = None;
    }
}

/// Per-event notification toggles and global configuration.
///
/// Persisted to `~/.config/aletheia/desktop.toml` under `[notifications]`.
/// DND state is in a separate ephemeral signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NotificationPreferences {
    /// Master on/off switch for all desktop notifications.
    pub enabled: bool,
    /// Agent turn-completion notifications.
    pub agent_completion: bool,
    /// Tool approval request notifications (always fires regardless of DND).
    pub tool_approval: bool,
    /// Error and tool-failure notifications.
    pub errors: bool,
    /// SSE connection lost/restored notifications.
    pub connection_status: bool,
    /// Play the system default notification sound.
    pub sound_enabled: bool,
    /// Only fire notifications when the window is hidden or unfocused.
    ///
    /// When `false`, notifications fire regardless of focus. When `true`,
    /// focused-window events are suppressed (except tool approval, which
    /// always fires because it is time-sensitive).
    pub only_when_backgrounded: bool,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_completion: true,
            tool_approval: true,
            errors: true,
            connection_status: true,
            sound_enabled: false,
            only_when_backgrounded: false,
        }
    }
}

impl NotificationPreferences {
    /// Whether a given category is enabled, respecting the master toggle.
    #[must_use]
    pub(crate) fn category_enabled(&self, category: NotificationCategory) -> bool {
        if !self.enabled {
            return false;
        }
        match category {
            NotificationCategory::AgentCompletion => self.agent_completion,
            NotificationCategory::ToolApproval => self.tool_approval,
            NotificationCategory::Error => self.errors,
            NotificationCategory::ConnectionStatus => self.connection_status,
        }
    }
}

/// A single entry in the notification history.
#[derive(Debug, Clone)]
pub(crate) struct NotificationEntry {
    /// Category of the notification.
    pub category: NotificationCategory,
    /// Title displayed in the notification.
    pub title: String,
    /// Body text.
    pub body: String,
    /// When this notification was sent.
    pub sent_at: Instant,
}

/// History of recent desktop notifications, capped at [`MAX_HISTORY`].
///
/// Read by a future notification-center view. Updated by
/// [`crate::services::notification_dispatch::NotificationDispatch`].
#[derive(Debug, Clone, Default)]
pub(crate) struct NotificationHistory {
    entries: VecDeque<NotificationEntry>,
}

impl NotificationHistory {
    /// Push a new entry, evicting the oldest when over capacity.
    pub(crate) fn push(&mut self, entry: NotificationEntry) {
        if self.entries.len() >= MAX_HISTORY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// All retained entries, oldest first.
    #[must_use]
    pub(crate) fn entries(&self) -> &VecDeque<NotificationEntry> {
        &self.entries
    }

    /// Number of retained entries.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnd_inactive_by_default() {
        let dnd = DndState::default();
        assert!(!dnd.is_suppressing());
    }

    #[test]
    fn dnd_active_without_expiry_suppresses() {
        let dnd = DndState {
            active: true,
            expires_at: None,
        };
        assert!(dnd.is_suppressing());
    }

    #[test]
    fn dnd_active_with_past_expiry_not_suppressing() {
        let past = Instant::now() - Duration::from_secs(1);
        let dnd = DndState {
            active: true,
            expires_at: Some(past),
        };
        assert!(!dnd.is_suppressing());
    }

    #[test]
    fn dnd_activate_and_deactivate() {
        let mut dnd = DndState::default();
        dnd.activate(DndDuration::FifteenMinutes);
        assert!(dnd.is_suppressing());
        dnd.deactivate();
        assert!(!dnd.is_suppressing());
    }

    #[test]
    fn preferences_master_toggle_disables_all() {
        let prefs = NotificationPreferences {
            enabled: false,
            ..Default::default()
        };
        for cat in [
            NotificationCategory::AgentCompletion,
            NotificationCategory::ToolApproval,
            NotificationCategory::Error,
            NotificationCategory::ConnectionStatus,
        ] {
            assert!(!prefs.category_enabled(cat));
        }
    }

    #[test]
    fn preferences_per_category_toggle() {
        let prefs = NotificationPreferences {
            agent_completion: false,
            ..Default::default()
        };
        assert!(!prefs.category_enabled(NotificationCategory::AgentCompletion));
        assert!(prefs.category_enabled(NotificationCategory::Error));
    }

    #[test]
    fn notification_history_caps_at_max() {
        let mut history = NotificationHistory::default();
        for i in 0..(MAX_HISTORY + 5) {
            history.push(NotificationEntry {
                category: NotificationCategory::AgentCompletion,
                title: format!("notif {i}"),
                body: String::new(),
                sent_at: Instant::now(),
            });
        }
        assert_eq!(history.len(), MAX_HISTORY);
        // Oldest should have been evicted.
        assert!(history.entries()[0].title.contains('5'));
    }

    #[test]
    fn dnd_duration_labels_are_nonempty() {
        for dur in [
            DndDuration::FifteenMinutes,
            DndDuration::OneHour,
            DndDuration::UntilTomorrow,
        ] {
            assert!(!dur.label().is_empty());
        }
    }
}
