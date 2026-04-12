//! Archive, restore, and bulk action components for session management.

use dioxus::prelude::*;
use skene::id::SessionId;

use crate::state::sessions::SessionSelectionStore;

const BULK_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    padding: var(--space-2) var(--space-3); \
    background: var(--bg-surface); \
    border-top: 1px solid var(--border);\
";

const BULK_BTN_STYLE: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const BULK_COUNT_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
";

const CLEAR_BTN_STYLE: &str = "\
    background: none; \
    border: none; \
    color: var(--text-secondary); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    text-decoration: underline; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
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
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    padding: var(--space-6); \
    max-width: 400px; \
    width: 100%;\
";

const DIALOG_TITLE_STYLE: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary); \
    margin-bottom: var(--space-3);\
";

const DIALOG_TEXT_STYLE: &str = "\
    font-size: var(--text-base); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-5); \
    line-height: var(--leading-normal);\
";

const DIALOG_ACTIONS_STYLE: &str = "\
    display: flex; \
    gap: var(--space-2); \
    justify-content: flex-end;\
";

const DIALOG_CANCEL_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const DIALOG_CONFIRM_BTN: &str = "\
    background: var(--accent); \
    color: white; \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
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
