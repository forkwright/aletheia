//! Archive, restore, and bulk action components for session management.

use dioxus::prelude::*;
use skene::id::SessionId;

use crate::state::sessions::{
    SessionListStore, SessionSelectionStore, session_can_archive, session_can_restore,
};

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
    background: var(--bg-overlay); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: var(--z-modal);\
";

const DIALOG_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    box-shadow: var(--shadow-modal); \
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
    color: var(--text-inverse); \
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
    list_store: Signal<SessionListStore>,
    mut selection_store: Signal<SessionSelectionStore>,
    on_bulk_archive: EventHandler<Vec<SessionId>>,
    on_bulk_restore: EventHandler<Vec<SessionId>>,
) -> Element {
    let count = selection_store.read().count();
    let mut show_archive_confirm = use_signal(|| false);
    let (archive_ids, restore_ids) = {
        let selection = selection_store.read();
        let list = list_store.read();
        let mut archive_ids = Vec::new();
        let mut restore_ids = Vec::new();

        for session in list
            .sessions
            .iter()
            .filter(|session| selection.is_selected(&session.id))
        {
            if session_can_archive(session) {
                archive_ids.push(session.id.clone());
            }
            if session_can_restore(session) {
                restore_ids.push(session.id.clone());
            }
        }

        (archive_ids, restore_ids)
    };
    let archive_count = archive_ids.len();
    let restore_count = restore_ids.len();

    if count == 0 {
        return rsx! {};
    }

    rsx! {
        div {
            style: "{BULK_BAR_STYLE}",
            span { style: "{BULK_COUNT_STYLE}", "{count} selected" }
            if archive_count > 0 {
                button {
                    style: "{BULK_BTN_STYLE}",
                    onclick: move |_| {
                        show_archive_confirm.set(true);
                    },
                    "Archive selected"
                }
            }
            if restore_count > 0 {
                button {
                    style: "{BULK_BTN_STYLE}",
                    onclick: {
                        let restore_ids = restore_ids.clone();
                        move |_| {
                            selection_store.write().clear();
                            on_bulk_restore.call(restore_ids.clone());
                        }
                    },
                    "Restore selected"
                }
            }
            button {
                style: "{CLEAR_BTN_STYLE}",
                onclick: move |_| {
                    selection_store.write().clear();
                },
                "Clear selection"
            }
        }
        if archive_count > 0 && *show_archive_confirm.read() {
            ConfirmDialog {
                title: "Archive sessions?".to_string(),
                message: format!(
                    "Archive {archive_count} session{}? They will be hidden from the active list but can be restored.",
                    if archive_count == 1 { "" } else { "s" }
                ),
                confirm_label: "Archive".to_string(),
                on_confirm: {
                    let archive_ids = archive_ids.clone();
                    move |_| {
                        show_archive_confirm.set(false);
                        selection_store.write().clear();
                        on_bulk_archive.call(archive_ids.clone());
                    }
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
