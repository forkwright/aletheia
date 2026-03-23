//! Toast notification state model.
//!
//! Provides non-blocking notifications for connection events, SSE
//! lifecycle changes, and user-facing feedback. Components read
//! `Signal<ToastStore>` to render the toast stack.

use std::time::Duration;

/// Auto-dismiss duration for informational toasts.
const DEFAULT_DISMISS_MS: u64 = 5_000;

/// Auto-dismiss duration for error toasts (longer to allow reading).
const ERROR_DISMISS_MS: u64 = 10_000;

/// Maximum visible toasts before oldest are evicted.
const MAX_VISIBLE: usize = 5;

/// Severity level controlling visual styling and auto-dismiss behavior.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Neutral information (dye: natural).
    Info,
    /// Positive outcome (dye: green).
    Success,
    /// Non-critical warning (dye: aporia).
    Warning,
    /// Critical failure (dye: aima).
    Error,
}

impl Severity {
    /// CSS-compatible color for this severity.
    #[must_use]
    pub(crate) fn css_color(self) -> &'static str {
        match self {
            Self::Info => "#94a3b8",
            Self::Success => "#22c55e",
            Self::Warning => "#eab308",
            Self::Error => "#ef4444",
        }
    }

    /// CSS-compatible background color for this severity.
    #[must_use]
    pub(crate) fn css_bg(self) -> &'static str {
        match self {
            Self::Info => "#1e293b",
            Self::Success => "#14532d",
            Self::Warning => "#422006",
            Self::Error => "#450a0a",
        }
    }

    /// Default auto-dismiss duration. Returns `None` for toasts with actions
    /// (caller handles that logic).
    #[must_use]
    pub(crate) fn auto_dismiss_duration(self) -> Duration {
        match self {
            Self::Error => Duration::from_millis(ERROR_DISMISS_MS),
            _ => Duration::from_millis(DEFAULT_DISMISS_MS),
        }
    }
}

/// Unique identifier for a toast, monotonically increasing.
pub type ToastId = u64;

/// Optional action button on a toast.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastAction {
    /// Button label text.
    pub label: String,
    /// Action identifier dispatched when clicked.
    pub action_id: String,
}

/// A single toast notification.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    /// Unique identifier.
    pub id: ToastId,
    /// Visual severity.
    pub severity: Severity,
    /// Short title text.
    pub title: String,
    /// Optional extended body text.
    pub body: Option<String>,
    /// Optional action button.
    pub action: Option<ToastAction>,
    /// Auto-dismiss duration. `None` means manual dismiss only (used for
    /// action toasts).
    pub auto_dismiss: Option<Duration>,
}

/// Reactive store holding the visible toast stack.
#[derive(Debug, Clone, Default)]
pub struct ToastStore {
    toasts: Vec<Toast>,
    next_id: ToastId,
}

impl ToastStore {
    /// Create an empty toast store.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
        }
    }

    /// Push a new toast, evicting the oldest if over capacity.
    /// Returns the assigned toast ID.
    pub(crate) fn push(&mut self, severity: Severity, title: impl Into<String>) -> ToastId {
        self.push_full(severity, title.into(), None, None)
    }

    /// Push a toast with optional body and action.
    pub(crate) fn push_full(
        &mut self,
        severity: Severity,
        title: String,
        body: Option<String>,
        action: Option<ToastAction>,
    ) -> ToastId {
        let id = self.next_id;
        self.next_id += 1;

        // WHY: Action toasts should not auto-dismiss since the user needs
        // time to interact with the action button.
        let auto_dismiss = if action.is_some() {
            None
        } else {
            Some(severity.auto_dismiss_duration())
        };

        self.toasts.push(Toast {
            id,
            severity,
            title,
            body,
            action,
            auto_dismiss,
        });

        // NOTE: Evict oldest when over capacity.
        while self.toasts.len() > MAX_VISIBLE {
            self.toasts.remove(0);
        }

        id
    }

    /// Dismiss a toast by ID.
    pub(crate) fn dismiss(&mut self, id: ToastId) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Remove all toasts.
    pub(crate) fn clear_all(&mut self) {
        self.toasts.clear();
    }

    /// Current visible toasts (oldest first).
    #[must_use]
    pub(crate) fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    /// Number of visible toasts.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.toasts.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn push_and_dismiss() {
        let mut store = ToastStore::new();
        let id = store.push(Severity::Info, "hello");
        assert_eq!(store.len(), 1);
        assert_eq!(store.toasts()[0].title, "hello");

        store.dismiss(id);
        assert!(store.is_empty());
    }

    #[test]
    fn dismiss_nonexistent_is_noop() {
        let mut store = ToastStore::new();
        store.push(Severity::Info, "a");
        store.dismiss(999);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn max_visible_evicts_oldest() {
        let mut store = ToastStore::new();
        for i in 0..7 {
            store.push(Severity::Info, format!("toast {i}"));
        }
        assert_eq!(store.len(), MAX_VISIBLE);
        assert_eq!(store.toasts()[0].title, "toast 2");
        assert_eq!(store.toasts()[4].title, "toast 6");
    }

    #[test]
    fn clear_all() {
        let mut store = ToastStore::new();
        store.push(Severity::Info, "a");
        store.push(Severity::Error, "b");
        store.clear_all();
        assert!(store.is_empty());
    }

    #[test]
    fn ids_are_monotonic() {
        let mut store = ToastStore::new();
        let id1 = store.push(Severity::Info, "a");
        let id2 = store.push(Severity::Info, "b");
        let id3 = store.push(Severity::Info, "c");
        assert!(id1 < id2);
        assert!(id2 < id3);
    }

    #[test]
    fn action_toast_no_auto_dismiss() {
        let mut store = ToastStore::new();
        store.push_full(
            Severity::Info,
            "open file".to_string(),
            None,
            Some(ToastAction {
                label: "Open".to_string(),
                action_id: "open_file".to_string(),
            }),
        );
        assert!(store.toasts()[0].auto_dismiss.is_none());
        assert!(store.toasts()[0].action.is_some());
    }

    #[test]
    fn error_auto_dismiss_is_longer() {
        let info_dur = Severity::Info.auto_dismiss_duration();
        let error_dur = Severity::Error.auto_dismiss_duration();
        assert!(error_dur > info_dur);
    }

    #[test]
    fn severity_colors_are_valid() {
        for severity in [Severity::Info, Severity::Success, Severity::Warning, Severity::Error] {
            assert!(severity.css_color().starts_with('#'));
            assert!(severity.css_bg().starts_with('#'));
        }
    }

    #[test]
    fn push_full_with_body() {
        let mut store = ToastStore::new();
        store.push_full(
            Severity::Warning,
            "disk space".to_string(),
            Some("Only 2GB remaining".to_string()),
            None,
        );
        let toast = &store.toasts()[0];
        assert_eq!(toast.severity, Severity::Warning);
        assert_eq!(toast.body.as_deref(), Some("Only 2GB remaining"));
        assert!(toast.auto_dismiss.is_some());
    }

    #[test]
    fn default_store_is_empty() {
        let store = ToastStore::default();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }
}
