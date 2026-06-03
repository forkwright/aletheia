//! Toast notification state model.
//!
//! Provides non-blocking notifications for connection events, SSE
//! lifecycle changes, and user-facing feedback. Components read
//! `Signal<ToastStore>` to render the toast stack.
//!
//! `Toast`, `ToastAction`, `ToastId`, and `ToastSeverity` are the canonical
//! types from `skeue`. `ToastStore` is proskenion-local state
//! management built on top of those types.

use std::time::Duration;

pub use skeue::{Toast, ToastAction, ToastId, ToastSeverity};

/// Auto-dismiss duration for informational toasts.
const DEFAULT_DISMISS_MS: u64 = 5_000;

/// Auto-dismiss duration for error toasts (longer to allow reading).
const ERROR_DISMISS_MS: u64 = 10_000;

/// Maximum visible toasts before oldest are evicted.
const MAX_VISIBLE: usize = 5;

/// Reactive store holding the visible toast stack.
#[derive(Debug, Clone, Default)]
pub struct ToastStore {
    toasts: Vec<Toast>,
    next_id: u64,
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
    pub(crate) fn push(&mut self, severity: ToastSeverity, title: impl Into<String>) -> ToastId {
        self.push_full(severity, title.into(), None, None)
    }

    /// Push a toast with optional body and action.
    pub(crate) fn push_full(
        &mut self,
        severity: ToastSeverity,
        title: String,
        body: Option<String>,
        action: Option<ToastAction>,
    ) -> ToastId {
        let id = ToastId(self.next_id);
        self.next_id += 1;

        // WHY: Action toasts should not auto-dismiss since the user needs
        // time to interact with the action button.
        let auto_dismiss = if action.is_some() {
            None
        } else {
            let ms = match severity {
                ToastSeverity::Error => ERROR_DISMISS_MS,
                _ => DEFAULT_DISMISS_MS,
            };
            Some(Duration::from_millis(ms))
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

    /// Current visible toasts (oldest first).
    #[must_use]
    pub(crate) fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    /// Whether the store is empty.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_dismiss() {
        let mut store = ToastStore::new();
        let id = store.push(ToastSeverity::Info, "hello");
        assert_eq!(store.toasts().len(), 1);
        assert_eq!(store.toasts()[0].title, "hello");

        store.dismiss(id);
        assert!(store.is_empty());
    }

    #[test]
    fn dismiss_nonexistent_is_noop() {
        let mut store = ToastStore::new();
        store.push(ToastSeverity::Info, "a");
        store.dismiss(ToastId(999));
        assert_eq!(store.toasts().len(), 1);
    }

    #[test]
    fn max_visible_evicts_oldest() {
        let mut store = ToastStore::new();
        for i in 0..7 {
            store.push(ToastSeverity::Info, format!("toast {i}"));
        }
        assert_eq!(store.toasts().len(), MAX_VISIBLE);
        assert_eq!(store.toasts()[0].title, "toast 2");
        assert_eq!(store.toasts()[4].title, "toast 6");
    }

    #[test]
    fn ids_are_monotonic() {
        let mut store = ToastStore::new();
        let id1 = store.push(ToastSeverity::Info, "a");
        let id2 = store.push(ToastSeverity::Info, "b");
        let id3 = store.push(ToastSeverity::Info, "c");
        assert!(id1 < id2);
        assert!(id2 < id3);
    }

    #[test]
    fn action_toast_no_auto_dismiss() {
        let mut store = ToastStore::new();
        store.push_full(
            ToastSeverity::Info,
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
        let mut store = ToastStore::new();
        store.push(ToastSeverity::Info, "info");
        store.push(ToastSeverity::Error, "error");
        let info_dur = store.toasts()[0]
            .auto_dismiss
            .expect("info has auto-dismiss");
        let error_dur = store.toasts()[1]
            .auto_dismiss
            .expect("error has auto-dismiss");
        assert!(error_dur > info_dur);
    }

    #[test]
    fn push_full_with_body() {
        let mut store = ToastStore::new();
        store.push_full(
            ToastSeverity::Warning,
            "disk space".to_string(),
            Some("Only 2GB remaining".to_string()),
            None,
        );
        let toast = &store.toasts()[0];
        assert_eq!(toast.severity, ToastSeverity::Warning);
        assert_eq!(toast.body.as_deref(), Some("Only 2GB remaining"));
        assert!(toast.auto_dismiss.is_some());
    }

    #[test]
    fn default_store_is_empty() {
        let store = ToastStore::default();
        assert!(store.is_empty());
    }
}
