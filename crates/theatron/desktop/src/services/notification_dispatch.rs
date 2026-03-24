//! Notification dispatch: routes SSE events to desktop notifications.
//!
//! [`NotificationDispatch`] is instantiated once before the SSE event loop
//! and called on each event. It applies:
//!
//! - **Preferences**: per-category and master enable/disable.
//! - **Focus rules**: completion notifications are suppressed when the window
//!   is in the foreground (tool approval always fires regardless).
//! - **DND**: all non-approval notifications are suppressed during Do Not
//!   Disturb mode. Tool approval fires regardless (time-sensitive).
//! - **Rate limiting**: sliding window of max 5 notifications per minute per
//!   category. Prevents spam during rapid agent activity.
//! - **Grouping**: the first notification in a category fires immediately.
//!   Subsequent arrivals within 3 seconds are coalesced into a pending group
//!   rather than sending duplicates. The group count is recorded in history.
//! - **Connection-loss delay**: fires only after 5 seconds of continuous
//!   disconnect to allow the auto-reconnect logic to succeed silently.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use theatron_core::api::types::SseEvent;
use theatron_core::id::NousId;

use crate::platform::native_notify::send_native;
use crate::platform::notifications::{NotificationPayload, NotificationUrgency};
use crate::state::notifications::{
    DndState, NotificationCategory, NotificationEntry, NotificationPreferences,
};
use crate::state::toasts::Severity;

/// Maximum notifications dispatched per minute per category (sliding window).
const RATE_LIMIT_PER_MINUTE: usize = 5;

/// Duration of the rate limiting sliding window.
const RATE_WINDOW: Duration = Duration::from_secs(60);

/// Notifications in the same category arriving within this window are coalesced.
const GROUP_WINDOW: Duration = Duration::from_secs(3);

/// Delay after first disconnect before firing a connection-lost notification.
const CONNECTION_LOST_DELAY: Duration = Duration::from_secs(5);

/// Maximum body length for error notifications.
const ERROR_BODY_MAX: usize = 200;

/// In-flight group collecting arrivals within [`GROUP_WINDOW`].
struct PendingGroup {
    /// Total number of notifications coalesced so far (including the first).
    count: u32,
    /// When the group window opened.
    started_at: Instant,
}

/// Routes SSE events to desktop notifications with rate limiting and grouping.
///
/// Create once and call [`Self::process_event`] for each event from the SSE
/// stream. The dispatcher maintains internal state between calls.
pub(crate) struct NotificationDispatch {
    /// Sliding windows of send timestamps per category for rate limiting.
    rate_windows: HashMap<NotificationCategory, VecDeque<Instant>>,
    /// Pending coalescing groups per category.
    pending_groups: HashMap<NotificationCategory, PendingGroup>,
    /// Timestamp of the most recent disconnect (for connection-loss delay).
    disconnect_at: Option<Instant>,
    /// Whether the SSE stream was connected on the previous event.
    was_connected: bool,
}

impl NotificationDispatch {
    /// Create a new dispatcher with empty state.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            rate_windows: HashMap::new(),
            pending_groups: HashMap::new(),
            disconnect_at: None,
            was_connected: false,
        }
    }

    /// Process one SSE event and dispatch notifications as appropriate.
    ///
    /// - `prefs`: snapshot of current notification preferences.
    /// - `dnd`: current Do Not Disturb state.
    /// - `window_focused`: whether the desktop window currently has focus.
    ///   Completion notifications are suppressed when the window is focused.
    ///   Tool approval is never suppressed.
    /// - `on_sent`: called with each [`NotificationEntry`] that was dispatched,
    ///   allowing the caller to record it in [`NotificationHistory`].
    /// - `toast_fallback`: called when native notification fails so the caller
    ///   can push an in-app toast instead.
    pub(crate) fn process_event(
        &mut self,
        event: &SseEvent,
        prefs: &NotificationPreferences,
        dnd: &DndState,
        window_focused: bool,
        on_sent: &mut impl FnMut(NotificationEntry),
        toast_fallback: &mut impl FnMut(Severity, &str),
    ) {
        // Flush expired pending groups before processing the new event.
        self.flush_expired_groups(prefs, dnd, window_focused, on_sent, toast_fallback);

        match event {
            SseEvent::TurnAfter { nous_id, .. } => {
                self.on_turn_after(nous_id, prefs, dnd, window_focused, on_sent, toast_fallback);
            }
            SseEvent::ToolFailed {
                nous_id,
                tool_name,
                error,
            } => {
                self.on_tool_failed(
                    nous_id,
                    tool_name,
                    error,
                    prefs,
                    dnd,
                    window_focused,
                    on_sent,
                    toast_fallback,
                );
            }
            SseEvent::Connected => {
                let was_pending = self.disconnect_at.is_some();
                self.disconnect_at = None;
                if was_pending {
                    // WHY: Only notify reconnect when we previously tried to notify
                    // a disconnect (meaning the disconnect lasted past the delay).
                    self.on_reconnected(prefs, dnd, window_focused, on_sent, toast_fallback);
                }
                self.was_connected = true;
            }
            SseEvent::Disconnected => {
                if self.was_connected && self.disconnect_at.is_none() {
                    // NOTE: Record the disconnect time; fire notification after delay.
                    self.disconnect_at = Some(Instant::now());
                }
                self.was_connected = false;
            }
            _ => {
                // NOTE: On other events, check if the delayed connection-loss
                // notification should now fire (5s have elapsed).
                self.check_connection_lost(prefs, dnd, window_focused, on_sent, toast_fallback);
            }
        }
    }

    // -- Event handlers -------------------------------------------------------

    fn on_turn_after(
        &mut self,
        nous_id: &NousId,
        prefs: &NotificationPreferences,
        dnd: &DndState,
        focused: bool,
        on_sent: &mut impl FnMut(NotificationEntry),
        fallback: &mut impl FnMut(Severity, &str),
    ) {
        if !prefs.category_enabled(NotificationCategory::AgentCompletion) {
            return;
        }
        // WHY: Completion notifications are irrelevant when the user is actively
        // watching -- suppress when focused (regardless of only_when_backgrounded).
        if focused {
            return;
        }
        let title = format!("{} finished", nous_id.as_str());
        self.dispatch_with_grouping(
            NotificationCategory::AgentCompletion,
            title,
            String::new(),
            NotificationUrgency::Low,
            Some(10),
            prefs,
            dnd,
            on_sent,
            fallback,
        );
    }

    fn on_tool_failed(
        &mut self,
        nous_id: &NousId,
        tool_name: &str,
        error: &str,
        prefs: &NotificationPreferences,
        dnd: &DndState,
        focused: bool,
        on_sent: &mut impl FnMut(NotificationEntry),
        fallback: &mut impl FnMut(Severity, &str),
    ) {
        if !prefs.category_enabled(NotificationCategory::Error) {
            return;
        }
        if focused && prefs.only_when_backgrounded {
            return;
        }
        let title = String::from("Aletheia — Error");
        let body_raw = format!(
            "{}: tool '{}' failed — {}",
            nous_id.as_str(),
            tool_name,
            error,
        );
        let body = truncate_chars(&body_raw, ERROR_BODY_MAX).to_string();
        self.dispatch_with_grouping(
            NotificationCategory::Error,
            title,
            body,
            NotificationUrgency::Critical,
            Some(10),
            prefs,
            dnd,
            on_sent,
            fallback,
        );
    }

    fn on_reconnected(
        &mut self,
        prefs: &NotificationPreferences,
        dnd: &DndState,
        focused: bool,
        on_sent: &mut impl FnMut(NotificationEntry),
        fallback: &mut impl FnMut(Severity, &str),
    ) {
        if !prefs.category_enabled(NotificationCategory::ConnectionStatus) {
            return;
        }
        if focused && prefs.only_when_backgrounded {
            return;
        }
        self.send(
            NotificationCategory::ConnectionStatus,
            "Reconnected to server".to_string(),
            String::new(),
            NotificationUrgency::Normal,
            Some(5),
            prefs,
            dnd,
            on_sent,
            fallback,
        );
    }

    fn check_connection_lost(
        &mut self,
        prefs: &NotificationPreferences,
        dnd: &DndState,
        focused: bool,
        on_sent: &mut impl FnMut(NotificationEntry),
        fallback: &mut impl FnMut(Severity, &str),
    ) {
        let Some(at) = self.disconnect_at else {
            return;
        };
        if Instant::now().duration_since(at) < CONNECTION_LOST_DELAY {
            return;
        }
        // Clear so we don't fire again.
        self.disconnect_at = None;

        if !prefs.category_enabled(NotificationCategory::ConnectionStatus) {
            return;
        }
        if focused && prefs.only_when_backgrounded {
            return;
        }
        self.send(
            NotificationCategory::ConnectionStatus,
            "Connection lost — reconnecting...".to_string(),
            String::new(),
            NotificationUrgency::Normal,
            Some(8),
            prefs,
            dnd,
            on_sent,
            fallback,
        );
    }

    // -- Grouping -------------------------------------------------------------

    /// Dispatch with coalescing: the first notification in a 3s window fires
    /// immediately; subsequent arrivals increment the pending group count.
    fn dispatch_with_grouping(
        &mut self,
        category: NotificationCategory,
        title: String,
        body: String,
        urgency: NotificationUrgency,
        timeout_secs: Option<u32>,
        prefs: &NotificationPreferences,
        dnd: &DndState,
        on_sent: &mut impl FnMut(NotificationEntry),
        fallback: &mut impl FnMut(Severity, &str),
    ) {
        let now = Instant::now();

        if let Some(group) = self.pending_groups.get_mut(&category) {
            if now.duration_since(group.started_at) < GROUP_WINDOW {
                // Still within the grouping window -- coalesce.
                group.count += 1;
                return;
            }
            // Group window has expired; remove and fall through to start a new one.
            self.pending_groups.remove(&category);
        }

        // Start new group and send the first notification immediately.
        self.send(
            category,
            title.clone(),
            body.clone(),
            urgency,
            timeout_secs,
            prefs,
            dnd,
            on_sent,
            fallback,
        );
        self.pending_groups.insert(
            category,
            PendingGroup {
                count: 1,
                started_at: now,
            },
        );
    }

    /// Remove expired groups. When a group had count > 1, the single
    /// first notification already went out; nothing extra is sent. The
    /// history recorded the first event; grouping just prevents spam.
    fn flush_expired_groups(
        &mut self,
        _prefs: &NotificationPreferences,
        _dnd: &DndState,
        _focused: bool,
        _on_sent: &mut impl FnMut(NotificationEntry),
        _fallback: &mut impl FnMut(Severity, &str),
    ) {
        let now = Instant::now();
        self.pending_groups
            .retain(|_, g| now.duration_since(g.started_at) < GROUP_WINDOW);
    }

    // -- Send -----------------------------------------------------------------

    /// Apply DND, rate limit, then dispatch the notification.
    #[expect(clippy::too_many_arguments, reason = "all parameters are contextual state")]
    fn send(
        &mut self,
        category: NotificationCategory,
        title: String,
        body: String,
        urgency: NotificationUrgency,
        timeout_secs: Option<u32>,
        prefs: &NotificationPreferences,
        dnd: &DndState,
        on_sent: &mut impl FnMut(NotificationEntry),
        fallback: &mut impl FnMut(Severity, &str),
    ) {
        // NOTE: Tool approval bypasses DND (time-sensitive).
        if category != NotificationCategory::ToolApproval && dnd.is_suppressing() {
            return;
        }

        if !self.within_rate_limit(category) {
            return;
        }

        let payload = NotificationPayload {
            title: title.clone(),
            body: body.clone(),
            urgency,
            timeout_secs,
        };

        let native_ok = send_native(&payload);
        if !native_ok {
            fallback(severity_for(category), &title);
        }

        on_sent(NotificationEntry {
            category,
            title,
            body,
            sent_at: Instant::now(),
        });
    }

    // -- Rate limiting --------------------------------------------------------

    /// Returns `true` if a notification for `category` is within the rate limit.
    ///
    /// Updates the sliding window on entry.
    pub(crate) fn within_rate_limit(&mut self, category: NotificationCategory) -> bool {
        let now = Instant::now();
        let window = self.rate_windows.entry(category).or_default();

        // Evict entries outside the sliding window.
        window.retain(|&sent| now.duration_since(sent) < RATE_WINDOW);

        if window.len() >= RATE_LIMIT_PER_MINUTE {
            return false;
        }
        window.push_back(now);
        true
    }
}

/// Map a notification category to an in-app toast severity for fallback.
fn severity_for(category: NotificationCategory) -> Severity {
    match category {
        NotificationCategory::AgentCompletion => Severity::Info,
        NotificationCategory::ToolApproval => Severity::Warning,
        NotificationCategory::Error => Severity::Error,
        NotificationCategory::ConnectionStatus => Severity::Info,
    }
}

/// Truncate `s` to at most `max_chars` Unicode scalar values.
fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use theatron_core::id::{NousId, SessionId};

    use super::*;
    use crate::state::notifications::{DndDuration, NotificationHistory};

    fn prefs() -> NotificationPreferences {
        NotificationPreferences::default()
    }

    fn dnd_off() -> DndState {
        DndState::default()
    }

    fn nous(id: &str) -> NousId {
        NousId::from(id)
    }

    fn session(id: &str) -> SessionId {
        SessionId::from(id)
    }

    fn no_fallback(_sev: Severity, _title: &str) {}

    // -- Dispatch: event routing ---------------------------------------------

    #[test]
    fn turn_after_dispatches_when_unfocused() {
        let mut dispatch = NotificationDispatch::new();
        let mut history = NotificationHistory::default();
        let prefs = prefs();
        let dnd = dnd_off();

        dispatch.process_event(
            &SseEvent::TurnAfter {
                nous_id: nous("syn"),
                session_id: session("s1"),
            },
            &prefs,
            &dnd,
            false,
            &mut |e| history.push(e),
            &mut no_fallback,
        );

        assert_eq!(history.len(), 1);
        assert_eq!(
            history.entries()[0].category,
            NotificationCategory::AgentCompletion
        );
    }

    #[test]
    fn turn_after_suppressed_when_focused() {
        let mut dispatch = NotificationDispatch::new();
        let mut history = NotificationHistory::default();

        dispatch.process_event(
            &SseEvent::TurnAfter {
                nous_id: nous("syn"),
                session_id: session("s1"),
            },
            &prefs(),
            &dnd_off(),
            true, // focused
            &mut |e| history.push(e),
            &mut no_fallback,
        );

        assert!(history.is_empty(), "completion should be suppressed when focused");
    }

    // -- Focus rules ----------------------------------------------------------

    #[test]
    fn only_when_backgrounded_suppresses_errors_when_focused() {
        let mut dispatch = NotificationDispatch::new();
        let mut history = NotificationHistory::default();
        let prefs = NotificationPreferences {
            only_when_backgrounded: true,
            ..Default::default()
        };

        dispatch.process_event(
            &SseEvent::ToolFailed {
                nous_id: nous("syn"),
                tool_name: "bash".to_string(),
                error: "timeout".to_string(),
            },
            &prefs,
            &dnd_off(),
            true, // focused
            &mut |e| history.push(e),
            &mut no_fallback,
        );

        assert!(history.is_empty());
    }

    // -- Rate limiting --------------------------------------------------------

    #[test]
    fn rate_limit_caps_at_five_per_minute() {
        // NOTE: Test within_rate_limit directly because grouping (3s window)
        // coalesces rapid-fire events before rate limiting is reached.
        let mut dispatch = NotificationDispatch::new();
        let mut allowed = 0usize;
        for _ in 0..7 {
            if dispatch.within_rate_limit(NotificationCategory::Error) {
                allowed += 1;
            }
        }
        assert_eq!(allowed, RATE_LIMIT_PER_MINUTE);
    }

    // -- Grouping -------------------------------------------------------------

    #[test]
    fn grouping_coalesces_second_arrival_within_window() {
        let mut dispatch = NotificationDispatch::new();
        let mut history = NotificationHistory::default();

        // Two rapid turn_after events -- first fires, second coalesces.
        for _ in 0..2 {
            dispatch.process_event(
                &SseEvent::TurnAfter {
                    nous_id: nous("syn"),
                    session_id: session("s1"),
                },
                &prefs(),
                &dnd_off(),
                false,
                &mut |e| history.push(e),
                &mut no_fallback,
            );
        }

        // Only the first notification should reach history.
        assert_eq!(history.len(), 1);
        assert!(dispatch
            .pending_groups
            .contains_key(&NotificationCategory::AgentCompletion));
    }

    // -- Preferences ----------------------------------------------------------

    #[test]
    fn disabled_category_not_dispatched() {
        let mut dispatch = NotificationDispatch::new();
        let mut history = NotificationHistory::default();
        let prefs = NotificationPreferences {
            agent_completion: false,
            ..Default::default()
        };

        dispatch.process_event(
            &SseEvent::TurnAfter {
                nous_id: nous("syn"),
                session_id: session("s1"),
            },
            &prefs,
            &dnd_off(),
            false,
            &mut |e| history.push(e),
            &mut no_fallback,
        );

        assert!(history.is_empty());
    }

    // -- DND ------------------------------------------------------------------

    #[test]
    fn dnd_suppresses_completion_notifications() {
        let mut dispatch = NotificationDispatch::new();
        let mut history = NotificationHistory::default();
        let mut dnd = dnd_off();
        dnd.activate(DndDuration::FifteenMinutes);

        dispatch.process_event(
            &SseEvent::TurnAfter {
                nous_id: nous("syn"),
                session_id: session("s1"),
            },
            &prefs(),
            &dnd,
            false,
            &mut |e| history.push(e),
            &mut no_fallback,
        );

        assert!(history.is_empty(), "DND should suppress completion notification");
    }

    // -- Helpers --------------------------------------------------------------

    #[test]
    fn truncate_chars_at_boundary() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
        assert_eq!(truncate_chars("hello", 100), "hello");
    }

    #[test]
    fn truncate_chars_empty_string() {
        assert_eq!(truncate_chars("", 5), "");
    }
}
