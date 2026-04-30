//! Toast container positioned at the top-right of the viewport.
//!
//! Renders the visible toast stack from `Signal<ToastStore>`. Placed in the
//! root layout so toasts appear above all content.

use dioxus::prelude::*;
use skeue::{ToastAction, ToastId, ToastItem};

use crate::state::navigation::{self, NavAction};
use crate::state::toasts::ToastStore;

const CONTAINER_STYLE: &str = "\
    position: fixed; \
    top: var(--space-4); \
    right: var(--space-4); \
    z-index: 9999; \
    display: flex; \
    flex-direction: column; \
    gap: var(--space-2); \
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
    let mut store = use_context::<Signal<ToastStore>>();
    let toasts = store.read();

    if toasts.is_empty() {
        return rsx! {};
    }

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            role: "status",
            aria_live: "polite",
            aria_label: "Notifications",
            for toast in toasts.toasts().iter().cloned() {
                div {
                    key: "{toast.id.0}",
                    style: "{ITEM_WRAPPER_STYLE}",
                    ToastItem {
                        toast,
                        on_dismiss: move |id: ToastId| { store.write().dismiss(id); },
                        on_action: move |action: ToastAction| {
                            if let Some(nav) = navigation::parse_action_id(&action.action_id) {
                                if let Some(mut sig) = try_consume_context::<Signal<Option<NavAction>>>() {
                                    sig.set(Some(nav));
                                }
                            }
                        },
                    }
                }
            }
        }
    }
}
