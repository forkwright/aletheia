//! Single toast notification component.

use dioxus::prelude::*;

use crate::services::toast::use_toast;
use crate::state::navigation::{self, NavAction};
use crate::state::toasts::{Toast, ToastId};

const TOAST_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-1); \
    padding: var(--space-3) var(--space-4); \
    border-radius: var(--radius-lg); \
    border-left: 4px solid; \
    min-width: 300px; \
    max-width: 400px; \
    box-shadow: var(--shadow-float, 0 4px 16px rgb(18 17 15 / 0.16)); \
    animation: toast-enter 350ms cubic-bezier(0.16, 1, 0.3, 1); \
    position: relative;\
";

const TITLE_STYLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-semibold);\
";

const BODY_STYLE: &str = "\
    font-size: var(--text-sm); \
    opacity: 0.85;\
";

const DISMISS_STYLE: &str = "\
    position: absolute; \
    top: var(--space-2); \
    right: var(--space-2); \
    background: none; \
    border: none; \
    color: inherit; \
    opacity: 0.6; \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    font-size: var(--text-base); \
    padding: var(--space-1) var(--space-2); \
    min-width: 24px; \
    min-height: 24px; \
    display: flex; \
    align-items: center; \
    justify-content: center;\
";

const ACTION_STYLE: &str = "\
    background: var(--bg-surface-bright); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    color: inherit; \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    font-size: var(--text-sm); \
    padding: var(--space-1) var(--space-3); \
    align-self: flex-start; \
    margin-top: var(--space-1);\
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
        #[expect(clippy::as_conversions, reason = "toast duration under u64::MAX milliseconds")]
        let ms = duration.as_millis() as u64;
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            toasts.dismiss(toast_id);
        });
    }

    rsx! {
        div {
            style: "{TOAST_STYLE} background: {bg}; border-color: {color}; color: var(--text-primary);",
            button {
                style: "{DISMISS_STYLE}",
                onclick: move |_| toasts.dismiss(toast_id),
                "\u{2715}"
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
