//! Two-panel workspace file browser: tree explorer (left) + viewer (right).

pub(crate) mod diff;
mod search;
pub(crate) mod toolbar;
mod tree;
mod viewer;

use dioxus::prelude::*;

use crate::state::navigation::NavAction;
use crate::views::files::diff::DiffViewer;
use crate::views::files::search::FileSearch;
use crate::views::files::tree::FileTree;
use crate::views::files::viewer::FileViewer;

/// What the files view is currently showing.
#[derive(Debug, Clone)]
enum FilesView {
    /// Standard file browser.
    Browser,
    /// Diff viewer for a specific file.
    Diff { path: String },
}

const FILES_LAYOUT_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: 12px;\
";

const PANELS_STYLE: &str = "\
    display: flex; \
    flex: 1; \
    overflow: hidden; \
    gap: 0;\
";

const TREE_PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 8px; \
    overflow: hidden; \
    flex-shrink: 0;\
";

const RESIZE_HANDLE_STYLE: &str = "\
    width: 4px; \
    cursor: col-resize; \
    background: transparent; \
    flex-shrink: 0; \
    transition: background var(--transition-quick, 0.15s);\
";

const COLLAPSE_BTN_STYLE: &str = "\
    background: none; \
    border: 1px solid var(--border, #2e2b27); \
    border-radius: var(--radius-sm, 4px); \
    color: var(--text-muted, #706c66); \
    padding: 2px 6px; \
    font-size: 11px; \
    cursor: pointer;\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding-bottom: 8px;\
";

const DEFAULT_TREE_WIDTH: f64 = 280.0;
const MIN_TREE_WIDTH: f64 = 160.0;
const MAX_TREE_WIDTH: f64 = 600.0;

#[component]
pub(crate) fn Files() -> Element {
    let mut selected_path: Signal<Option<String>> = use_signal(|| None);
    let mut tree_collapsed = use_signal(|| false);
    let mut tree_width = use_signal(|| DEFAULT_TREE_WIDTH);
    let is_searching = use_signal(|| false);
    let mut is_resizing = use_signal(|| false);
    let mut resize_start_x = use_signal(|| 0.0f64);
    let mut resize_start_width = use_signal(|| 0.0f64);
    let mut view = use_signal(|| FilesView::Browser);

    // NOTE: Consume navigation actions from toast buttons to open diff viewer.
    if let Some(mut nav_signal) = try_consume_context::<Signal<Option<NavAction>>>() {
        let action = nav_signal.read().clone();
        if let Some(NavAction::OpenDiff(path)) = action {
            nav_signal.set(None);
            view.set(FilesView::Diff { path });
        }
    }

    let on_select_file = move |path: String| {
        selected_path.set(Some(path));
    };

    let current_view = view.read().clone();
    match current_view {
        FilesView::Diff { ref path } => {
            let p = path.clone();
            rsx! {
                div {
                    style: "display: flex; flex-direction: column; height: 100%; padding: 16px;",
                    DiffViewer {
                        path: p,
                        on_back: move |_| view.set(FilesView::Browser),
                    }
                }
            }
        }
        FilesView::Browser => {
            let collapsed = *tree_collapsed.read();
            let width = *tree_width.read();
            let panel_width = if collapsed {
                "0px".to_string()
            } else {
                format!("{width}px")
            };

            rsx! {
                div {
                    style: "{FILES_LAYOUT_STYLE}",
                    // Header
                    div {
                        style: "{HEADER_STYLE}",
                        h2 {
                            style: "font-size: 18px; margin: 0; color: var(--text-primary, #e0e0e0);",
                            "Files"
                        }
                        button {
                            style: "{COLLAPSE_BTN_STYLE}",
                            onclick: move |_| {
                                let current = *tree_collapsed.read();
                                tree_collapsed.set(!current);
                            },
                            if collapsed { "\u{25B6} Show Tree" } else { "\u{25C0} Hide Tree" }
                        }
                    }
                    // Two-panel layout
                    div {
                        style: "{PANELS_STYLE}",
                        // WHY: mousemove on the outer container so dragging past the handle
                        // still updates the width.
                        onmousemove: move |evt: Event<MouseData>| {
                            if *is_resizing.read() {
                                let delta = evt.client_coordinates().x - *resize_start_x.read();
                                let new_width = (*resize_start_width.read() + delta)
                                    .clamp(MIN_TREE_WIDTH, MAX_TREE_WIDTH);
                                tree_width.set(new_width);
                            }
                        },
                        onmouseup: move |_| {
                            is_resizing.set(false);
                        },
                        // Tree panel
                        if !collapsed {
                            div {
                                style: "{TREE_PANEL_STYLE} width: {panel_width};",
                                FileSearch {
                                    on_select_file: on_select_file,
                                    is_searching,
                                }
                                if !*is_searching.read() {
                                    FileTree {
                                        selected_path,
                                        on_select_file: on_select_file,
                                    }
                                }
                            }
                            // Resize handle
                            div {
                                style: "{RESIZE_HANDLE_STYLE}",
                                onmousedown: move |evt: Event<MouseData>| {
                                    is_resizing.set(true);
                                    resize_start_x.set(evt.client_coordinates().x);
                                    resize_start_width.set(*tree_width.read());
                                },
                                onmouseenter: move |_| {},
                            }
                        }
                        // Viewer panel
                        FileViewer {
                            selected_path,
                        }
                    }
                }
            }
        }
    }
}
