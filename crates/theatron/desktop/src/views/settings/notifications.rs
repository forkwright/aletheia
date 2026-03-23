//! Notification preferences settings panel.
//!
//! Rendered as a section within the Settings view. Reads and writes
//! `Signal<NotificationPreferences>` and `Signal<DndState>` from context,
//! and persists preference changes to the config file via `services::config`.

use dioxus::prelude::*;

use crate::services::config;
use crate::state::notifications::{DndDuration, DndState, NotificationPreferences};

const SECTION_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px;\
";

const SECTION_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin-bottom: 12px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: center; \
    padding: 8px 0; \
    border-bottom: 1px solid #222;\
";

const LABEL_STYLE: &str = "\
    color: #888; \
    font-size: 13px;\
";

const TOGGLE_ON: &str = "\
    background: #4f46e5; \
    color: #fff; \
    border: 1px solid #6366f1; \
    border-radius: 6px; \
    padding: 6px 14px; \
    font-size: 13px; \
    cursor: pointer;\
";

const TOGGLE_OFF: &str = "\
    background: #2a2a4a; \
    color: #888; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 6px 14px; \
    font-size: 13px; \
    cursor: pointer;\
";

const DND_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 5px 10px; \
    font-size: 12px; \
    cursor: pointer; \
    margin-left: 6px;\
";

const WARNING_STYLE: &str = "\
    color: #eab308; \
    font-size: 12px; \
    margin-top: 4px;\
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

            // Master toggle
            div {
                style: "{ROW_STYLE}",
                span { style: "{LABEL_STYLE}", "Desktop notifications" }
                button {
                    style: if enabled { TOGGLE_ON } else { TOGGLE_OFF },
                    onclick: move |_| {
                        prefs.write().enabled = !enabled;
                        config::save_notification_prefs(&prefs.read()).ok();
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
                        config::save_notification_prefs(&prefs.read()).ok();
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
                        config::save_notification_prefs(&prefs.read()).ok();
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
                        config::save_notification_prefs(&prefs.read()).ok();
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
                        config::save_notification_prefs(&prefs.read()).ok();
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
                        config::save_notification_prefs(&prefs.read()).ok();
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
                        config::save_notification_prefs(&prefs.read()).ok();
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
                        div { style: "color: #4f46e5; font-size: 12px; margin-top: 2px;", "Active" }
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
