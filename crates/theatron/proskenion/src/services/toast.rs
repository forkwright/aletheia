//! Toast notification service — provides `provide_toast_context` for the app root.

use dioxus::prelude::*;

use crate::state::toasts::ToastStore;

/// Provide the toast store as a context signal.
///
/// Call once in the app root. Child components read it via
/// `use_context::<Signal<ToastStore>>()`.
pub(crate) fn provide_toast_context() -> Signal<ToastStore> {
    use_context_provider(|| Signal::new(ToastStore::new()))
}
