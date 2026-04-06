//! Archive, restore, and bulk action components for session management.

use dioxus::prelude::*;
use theatron_core::id::SessionId;

use crate::state::sessions::SessionSelectionStore;

const BULK_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    padding: 8px 12px; \
    background: #1a1a2e; \
    border-top: 1px solid #333;\
";

const BULK_BTN_STYLE: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 6px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const BULK_COUNT_STYLE: &str = "\
    font-size: 12px; \
    color: #888;\
";

const CLEAR_BTN_STYLE: &str = "\
    background: none; \
    border: none; \
    color: #888; \
    font-size: 12px; \
    cursor: pointer; \
    text-decoration: underline;\
";

const DIALOG_OVERLAY_STYLE: &str = "\
    position: fixed; \
    top: 0; \
    left: 0; \
    right: 0; \
    bottom: 0; \
    background: rgba(0, 0, 0, 0.6); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: 100;\
";

const DIALOG_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 12px; \
    padding: 24px; \
    max-width: 400px; \
    width: 100%;\
";

const DIALOG_TITLE_STYLE: &str = "\
    font-size: 16px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin-bottom: 12px;\
";

const DIALOG_TEXT_STYLE: &str = "\
    font-size: 14px; \
    color: #aaa; \
    margin-bottom: 20px; \
    line-height: 1.4;\
";

const DIALOG_ACTIONS_STYLE: &str = "\
    display: flex; \
    gap: 8px; \
    justify-content: flex-end;\
";

const DIALOG_CANCEL_BTN: &str = "\
    background: #2a2a3a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 8px 16px; \
    font-size: 13px; \
    cursor: pointer;\
";

const DIALOG_CONFIRM_BTN: &str = "\
    background: #4a4aff; \
    color: white; \
    border: none; \
    border-radius: 6px; \
    padding: 8px 16px; \
    font-size: 13px; \
    cursor: pointer;\
";

/// Bulk action bar shown when sessions are selected.
#[component]
pub(crate) fn BulkActionBar(
    mut selection_store: Signal<SessionSelectionStore>,
    on_bulk_archive: EventHandler<Vec<SessionId>>,
    on_bulk_restore: EventHandler<Vec<SessionId>>,
) -> Element {
    let count = selection_store.read().count();
    let mut show_archive_confirm = use_signal(|| false);

    if count == 0 {
        return rsx! {};
    }

    rsx! {
        div {
            style: "{BULK_BAR_STYLE}",
            span { style: "{BULK_COUNT_STYLE}", "{count} selected" }
            button {
                style: "{BULK_BTN_STYLE}",
                onclick: move |_| {
                    show_archive_confirm.set(true);
                },
                "Archive selected"
            }
            button {
                style: "{BULK_BTN_STYLE}",
                onclick: move |_| {
                    let ids = selection_store.write().take_selected();
                    on_bulk_restore.call(ids);
                },
                "Restore selected"
            }
            button {
                style: "{CLEAR_BTN_STYLE}",
                onclick: move |_| {
                    selection_store.write().clear();
                },
                "Clear selection"
            }
        }
        // Archive confirmation dialog
        if *show_archive_confirm.read() {
            ConfirmDialog {
                title: "Archive sessions?".to_string(),
                message: format!(
                    "Archive {count} session{}? They will be hidden from the active list but can be restored.",
                    if count == 1 { "" } else { "s" }
                ),
                confirm_label: "Archive".to_string(),
                on_confirm: move |_| {
                    show_archive_confirm.set(false);
                    let ids = selection_store.write().take_selected();
                    on_bulk_archive.call(ids);
                },
                on_cancel: move |_| {
                    show_archive_confirm.set(false);
                },
            }
        }
    }
}

/// Confirmation dialog for archive/restore.
#[component]
pub(crate) fn ConfirmDialog(
    title: String,
    message: String,
    confirm_label: String,
    on_confirm: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            style: "{DIALOG_OVERLAY_STYLE}",
            onclick: move |_| on_cancel.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{DIALOG_TITLE_STYLE}", "{title}" }
                div { style: "{DIALOG_TEXT_STYLE}", "{message}" }
                div {
                    style: "{DIALOG_ACTIONS_STYLE}",
                    button {
                        style: "{DIALOG_CANCEL_BTN}",
                        onclick: move |_| on_cancel.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{DIALOG_CONFIRM_BTN}",
                        onclick: move |_| on_confirm.call(()),
                        "{confirm_label}"
                    }
                }
            }
        }
    }
}
