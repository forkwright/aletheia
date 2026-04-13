//! Notification preferences settings panel.
//!
//! Rendered as a section within the Settings view. Reads and writes
//! `Signal<NotificationPreferences>` and `Signal<DndState>` from context,
//! and persists preference changes to the config file via `services::config`.

use dioxus::prelude::*;

use crate::services::config;
use crate::state::notifications::{DndDuration, DndState, NotificationPreferences};

const SECTION_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-5);\
";

const SECTION_TITLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin-bottom: var(--space-3);\
";

const ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: center; \
    padding: var(--space-2) 0; \
    border-bottom: 1px solid #222;\
";

const LABEL_STYLE: &str = "\
    color: var(--text-secondary); \
    font-size: var(--text-sm);\
";

const TOGGLE_ON: &str = "\
    background: #4f46e5; \
    color: var(--text-primary); \
    border: 1px solid #6366f1; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TOGGLE_OFF: &str = "\
    background: var(--border); \
    color: var(--text-secondary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const DND_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    margin-left: var(--space-2); \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const WARNING_STYLE: &str = "\
    color: var(--status-warning); \
    font-size: var(--text-xs); \
    margin-top: var(--space-1);\
";

/// Notification settings panel component.
///
/// Requires `Signal<NotificationPreferences>` and `Signal<DndState>` to be
/// provided as context by an ancestor (done in `ConnectedApp`).
#[component]
pub(crate) fn NotificationSettings() -> Element {
    let mut prefs: Signal<NotificationPreferences> = use_context();
    let mut dnd: Signal<DndState> = use_context();

    let enabled = prefs.read().enabled;
    let agent_completion = prefs.read().agent_completion;
    let tool_approval = prefs.read().tool_approval;
    let errors = prefs.read().errors;
    let connection_status = prefs.read().connection_status;
    let sound_enabled = prefs.read().sound_enabled;
    let only_when_backgrounded = prefs.read().only_when_backgrounded;
    let dnd_active = dnd.read().is_suppressing();

    rsx! {
        div {
            style: "{SECTION_STYLE}",
            div { style: "{SECTION_TITLE}", "Notifications" }

            // Global toggle
            div {
                style: "{ROW_STYLE}",
                span { style: "{LABEL_STYLE}", "Desktop notifications" }
                button {
                    style: if enabled { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().enabled = !enabled;
                        if let Err(e) = config::save_notification_prefs(&prefs.read()) {
                            tracing::warn!(error = %e, "failed to save notification preferences");
                        }
                    },
                    if enabled { "On" } else { "Off" }
                }
            }

            // Per-event: agent completion
            div {
                style: "{ROW_STYLE}",
                span { style: "{LABEL_STYLE}", "Agent completion" }
                button {
                    style: if agent_completion { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().agent_completion = !agent_completion;
                        if let Err(e) = config::save_notification_prefs(&prefs.read()) {
                            tracing::warn!(error = %e, "failed to save notification preferences");
                        }
                    },
                    if agent_completion { "On" } else { "Off" }
                }
            }

            // Per-event: tool approval (warn if disabled)
            div {
                style: if !tool_approval { ROW_STYLE } else { ROW_STYLE },
                span {
                    style: "{LABEL_STYLE}",
                    "Tool approval requests"
                    if !tool_approval {
                        div { style: "{WARNING_STYLE}", "Warning: you may miss approval requests" }
                    }
                }
                button {
                    style: if tool_approval { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().tool_approval = !tool_approval;
                        if let Err(e) = config::save_notification_prefs(&prefs.read()) {
                            tracing::warn!(error = %e, "failed to save notification preferences");
                        }
                    },
                    if tool_approval { "On" } else { "Off" }
                }
            }

            // Per-event: errors
            div {
                style: "{ROW_STYLE}",
                span { style: "{LABEL_STYLE}", "Errors" }
                button {
                    style: if errors { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().errors = !errors;
                        if let Err(e) = config::save_notification_prefs(&prefs.read()) {
                            tracing::warn!(error = %e, "failed to save notification preferences");
                        }
                    },
                    if errors { "On" } else { "Off" }
                }
            }

            // Per-event: connection status
            div {
                style: "{ROW_STYLE}",
                span { style: "{LABEL_STYLE}", "Connection status" }
                button {
                    style: if connection_status { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().connection_status = !connection_status;
                        if let Err(e) = config::save_notification_prefs(&prefs.read()) {
                            tracing::warn!(error = %e, "failed to save notification preferences");
                        }
                    },
                    if connection_status { "On" } else { "Off" }
                }
            }

            // Sound
            div {
                style: "{ROW_STYLE}",
                span { style: "{LABEL_STYLE}", "Sound" }
                button {
                    style: if sound_enabled { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().sound_enabled = !sound_enabled;
                        if let Err(e) = config::save_notification_prefs(&prefs.read()) {
                            tracing::warn!(error = %e, "failed to save notification preferences");
                        }
                    },
                    if sound_enabled { "On" } else { "Off" }
                }
            }

            // Only when backgrounded
            div {
                style: "{ROW_STYLE}",
                span { style: "{LABEL_STYLE}", "Only when app is backgrounded" }
                button {
                    style: if only_when_backgrounded { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().only_when_backgrounded = !only_when_backgrounded;
                        if let Err(e) = config::save_notification_prefs(&prefs.read()) {
                            tracing::warn!(error = %e, "failed to save notification preferences");
                        }
                    },
                    if only_when_backgrounded { "On" } else { "Off" }
                }
            }

            // Do Not Disturb
            div {
                style: "border-bottom: none; {ROW_STYLE}",
                span {
                    style: "{LABEL_STYLE}",
                    "Do Not Disturb"
                    if dnd_active {
                        div { style: "color: #4f46e5; font-size: var(--text-xs); margin-top: var(--space-1);", "Active" }
                    }
                }
                div {
                    if dnd_active {
                        button {
                            style: "{DND_BTN}",
                            onclick: move |_| {
                                dnd.write().deactivate();
                            },
                            "Turn off"
                        }
                    } else {
                        button {
                            style: "{DND_BTN}",
                            onclick: move |_| {
                                dnd.write().activate(DndDuration::FifteenMinutes);
                            },
                            "15 min"
                        }
                        button {
                            style: "{DND_BTN}",
                            onclick: move |_| {
                                dnd.write().activate(DndDuration::OneHour);
                            },
                            "1 hour"
                        }
                        button {
                            style: "{DND_BTN}",
                            onclick: move |_| {
                                dnd.write().activate(DndDuration::UntilTomorrow);
                            },
                            "Until tomorrow"
                        }
                    }
                }
            }
        }
    }
}
