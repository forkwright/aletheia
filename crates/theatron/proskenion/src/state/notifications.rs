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

/// Global event topic emitted when a tool approval is required.
pub(crate) const TOOL_APPROVAL_REQUIRED_TOPIC: &str = "tool.approval.required";

/// Global event topic emitted when a tool approval decision is resolved.
pub(crate) const TOOL_APPROVAL_RESOLVED_TOPIC: &str = "tool.approval.resolved";

/// Category of notification, used for per-event toggles and rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum NotificationCategory {
    /// Agent turn completed (triggered by `TurnAfter` SSE event).
    AgentCompletion,
    /// Tool approval required.
    ToolApproval,
    /// Agent error or tool failure (triggered by `ToolFailed` SSE event).
    Error,
    /// SSE connection lost or restored.
    ConnectionStatus,
}

/// Notification event sources advertised by the connected server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct NotificationCapabilities {
    /// Whether the server emits global tool-approval-required events.
    pub tool_approval_events: bool,
    /// Whether the server emits global tool-approval-resolved events.
    pub tool_approval_resolved_events: bool,
}

impl NotificationCapabilities {
    /// Build capabilities from `/api/v1/events/discovery` topic names.
    #[must_use]
    pub(crate) fn from_event_topics(topics: &[String]) -> Self {
        Self {
            tool_approval_events: topics
                .iter()
                .any(|topic| topic == TOOL_APPROVAL_REQUIRED_TOPIC),
            tool_approval_resolved_events: topics
                .iter()
                .any(|topic| topic == TOOL_APPROVAL_RESOLVED_TOPIC),
        }
    }

    /// Whether the tool approval preference can be presented as active.
    #[must_use]
    pub(crate) fn tool_approval_available(&self) -> bool {
        self.tool_approval_events && self.tool_approval_resolved_events
    }
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
}

/// Ephemeral Do Not Disturb state -- resets on app restart (not persisted).
///
/// Activation time and expiry are [`Instant`]-based so they cannot be
/// serialized. Store this in a separate signal from [`NotificationPreferences`].
#[derive(Debug, Clone, Default)]
pub(crate) struct DndState {
    /// Whether DND mode is currently active.
    pub active: bool,
    /// When DND mode expires. `None` means DND is indefinite until deactivated.
    pub expires_at: Option<Instant>,
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
    /// Global on/off switch for all desktop notifications.
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
    /// Whether a given category is enabled, respecting the global toggle.
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "part of public notification API")
    )]
    pub category: NotificationCategory,
    /// Title displayed in the notification.
    #[cfg_attr(not(test), expect(dead_code, reason = "public API"))]
    pub title: String,
    /// Body text.
    #[cfg_attr(not(test), expect(dead_code, reason = "public API"))]
    pub body: String,
    /// When this notification was sent.
    #[expect(dead_code, reason = "public API")]
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

    /// Whether the history is empty.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries in the history.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Slice of all entries in chronological order.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn entries(&self) -> &VecDeque<NotificationEntry> {
        &self.entries
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
    fn notification_capabilities_default_to_unavailable() {
        let caps = NotificationCapabilities::default();
        assert!(!caps.tool_approval_available());
    }

    #[test]
    fn notification_capabilities_detect_approval_topic() {
        let topics = vec![
            "fact.created".to_string(),
            TOOL_APPROVAL_REQUIRED_TOPIC.to_string(),
            TOOL_APPROVAL_RESOLVED_TOPIC.to_string(),
        ];
        let caps = NotificationCapabilities::from_event_topics(&topics);
        assert!(caps.tool_approval_available());
        assert!(caps.tool_approval_resolved_events);
    }

    #[test]
    fn notification_capabilities_reject_resolved_without_required_topic() {
        let topics = vec![TOOL_APPROVAL_RESOLVED_TOPIC.to_string()];
        let caps = NotificationCapabilities::from_event_topics(&topics);
        assert!(caps.tool_approval_resolved_events);
        assert!(
            !caps.tool_approval_available(),
            "the preference is unavailable without the approval-needed source"
        );
    }

    #[test]
    fn notification_capabilities_reject_required_without_resolved_topic() {
        let topics = vec![TOOL_APPROVAL_REQUIRED_TOPIC.to_string()];
        let caps = NotificationCapabilities::from_event_topics(&topics);
        assert!(caps.tool_approval_events);
        assert!(
            !caps.tool_approval_available(),
            "the preference is unavailable without the full approval event contract"
        );
    }
}
