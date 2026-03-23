//! Toast container positioned at the top-right of the viewport.
//!
//! Renders the visible toast stack from `Signal<ToastStore>`. Placed in the
//! root layout so toasts appear above all content.

use dioxus::prelude::*;

use crate::state::toasts::ToastStore;

use super::toast::ToastItem;

const CONTAINER_STYLE: &str = "\
    position: fixed; \
    top: 16px; \
    right: 16px; \
    z-index: 9999; \
    display: flex; \
    flex-direction: column; \
    gap: 8px; \
    pointer-events: none;\
";

/// WHY: Each toast needs pointer-events re-enabled so buttons work,
/// while the container itself is transparent to clicks.
const ITEM_WRAPPER_STYLE: &str = "pointer-events: auto;";

/// Render the global toast stack.
///
/// Reads `Signal<ToastStore>` from context (provided by
/// [`provide_toast_context`](crate::services::toast::provide_toast_context)).
#[component]
pub(crate) fn ToastContainer() -> Element {
    let store = use_context::<Signal<ToastStore>>();
    let toasts = store.read();

    if toasts.is_empty() {
        return rsx! {};
    }

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            for toast in toasts.toasts().iter().cloned() {
                div {
                    key: "{toast.id}",
                    style: "{ITEM_WRAPPER_STYLE}",
                    ToastItem { toast }
                }
            }
        }
    }
}
