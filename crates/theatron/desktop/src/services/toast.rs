//! Toast notification service providing `use_toast()` for any component.
//!
//! Provides the toast signal as a Dioxus context and convenience methods
//! for pushing toasts from anywhere in the component tree.

use dioxus::prelude::*;

use crate::state::toasts::{Severity, ToastAction, ToastId, ToastStore};

/// Provide the toast store as a context signal.
///
/// Call once in the app root. Child components access it via [`use_toast`].
pub(crate) fn provide_toast_context() -> Signal<ToastStore> {
    use_context_provider(|| Signal::new(ToastStore::new()))
}

/// Access the toast store from any component.
///
/// Requires [`provide_toast_context`] to have been called in an ancestor.
#[must_use]
pub(crate) fn use_toast() -> ToastHandle {
    let store = use_context::<Signal<ToastStore>>();
    ToastHandle { store }
}

/// Handle for pushing toasts, returned by [`use_toast`].
#[derive(Clone, Copy)]
pub(crate) struct ToastHandle {
    store: Signal<ToastStore>,
}

impl ToastHandle {
    /// Push an info toast.
    pub(crate) fn info(&mut self, title: impl Into<String>) -> ToastId {
        self.store.write().push(Severity::Info, title)
    }

    /// Push a success toast.
    pub(crate) fn success(&mut self, title: impl Into<String>) -> ToastId {
        self.store.write().push(Severity::Success, title)
    }

    /// Push a warning toast.
    pub(crate) fn warning(&mut self, title: impl Into<String>) -> ToastId {
        self.store.write().push(Severity::Warning, title)
    }

    /// Push an error toast.
    pub(crate) fn error(&mut self, title: impl Into<String>) -> ToastId {
        self.store.write().push(Severity::Error, title)
    }

    /// Push a toast with an action button.
    pub(crate) fn with_action(
        &mut self,
        severity: Severity,
        title: impl Into<String>,
        label: impl Into<String>,
        action_id: impl Into<String>,
    ) -> ToastId {
        self.store.write().push_full(
            severity,
            title.into(),
            None,
            Some(ToastAction {
                label: label.into(),
                action_id: action_id.into(),
            }),
        )
    }

    /// Dismiss a toast by ID.
    pub(crate) fn dismiss(&mut self, id: ToastId) {
        self.store.write().dismiss(id);
    }
}
