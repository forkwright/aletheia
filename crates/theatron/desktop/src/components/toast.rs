//! Single toast notification component.

use dioxus::prelude::*;

use crate::services::toast::use_toast;
use crate::state::navigation::{self, NavAction};
use crate::state::toasts::{Toast, ToastId};

const TOAST_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 4px; \
    padding: 12px 16px; \
    border-radius: 8px; \
    border-left: 4px solid; \
    min-width: 300px; \
    max-width: 400px; \
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3); \
    position: relative;\
";

const TITLE_STYLE: &str = "\
    font-size: 14px; \
    font-weight: 600;\
";

const BODY_STYLE: &str = "\
    font-size: 13px; \
    opacity: 0.85;\
";

const DISMISS_STYLE: &str = "\
    position: absolute; \
    top: 8px; \
    right: 8px; \
    background: none; \
    border: none; \
    color: inherit; \
    opacity: 0.6; \
    cursor: pointer; \
    font-size: 14px; \
    padding: 2px 4px;\
";

const ACTION_STYLE: &str = "\
    background: rgba(255, 255, 255, 0.15); \
    border: 1px solid rgba(255, 255, 255, 0.2); \
    border-radius: 4px; \
    color: inherit; \
    cursor: pointer; \
    font-size: 13px; \
    padding: 4px 12px; \
    align-self: flex-start; \
    margin-top: 4px;\
";

/// Render a single toast notification.
#[component]
pub(crate) fn ToastItem(toast: Toast) -> Element {
    let mut toasts = use_toast();
    let toast_id: ToastId = toast.id;
    let color = toast.severity.css_color();
    let bg = toast.severity.css_bg();

    // WHY: Auto-dismiss timer. Spawn a task that sleeps then dismisses.
    // Runs once per toast mount.
    if let Some(duration) = toast.auto_dismiss {
        let ms = duration.as_millis() as u64;
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            toasts.dismiss(toast_id);
        });
    }

    rsx! {
        div {
            style: "{TOAST_STYLE} background: {bg}; border-color: {color}; color: #e0e0e0;",
            button {
                style: "{DISMISS_STYLE}",
                onclick: move |_| toasts.dismiss(toast_id),
                "x"
            }
            div { style: "{TITLE_STYLE}", "{toast.title}" }
            if let Some(ref body) = toast.body {
                div { style: "{BODY_STYLE}", "{body}" }
            }
            if let Some(ref action) = toast.action {
                {
                    let action_id = action.action_id.clone();
                    rsx! {
                        button {
                            style: "{ACTION_STYLE}",
                            onclick: move |_| {
                                // NOTE: Dispatch navigation actions before dismissing.
                                if let Some(nav) = navigation::parse_action_id(&action_id) {
                                    if let Some(mut signal) = try_consume_context::<Signal<Option<NavAction>>>() {
                                        signal.set(Some(nav));
                                    }
                                }
                                toasts.dismiss(toast_id);
                            },
                            "{action.label}"
                        }
                    }
                }
            }
        }
    }
}
