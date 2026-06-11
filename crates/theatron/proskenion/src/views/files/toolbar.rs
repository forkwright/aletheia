//! Viewer toolbar: breadcrumb, view-mode toggle, file stats, edit/save, copy path.

use dioxus::prelude::*;

/// How the viewer renders the current file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewMode {
    /// Rendered markdown reading view (markdown files only).
    Preview,
    /// Raw syntect-highlighted source.
    Source,
    /// Editable markdown source with a live preview pane (markdown files only).
    Edit,
}

const TOOLBAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    border-bottom: 1px solid var(--border-separator, #221f1c); \
    font-size: var(--text-xs); \
    color: var(--text-secondary, #a8a49e); \
    flex-shrink: 0;\
";

const BREADCRUMB_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-1); \
    flex: 1; \
    overflow: hidden;\
";

const CRUMB_STYLE: &str = "\
    color: var(--text-secondary, #a8a49e); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    white-space: nowrap;\
";

const CRUMB_SEPARATOR: &str = "\
    color: var(--text-muted, #706c66); \
    margin: 0 var(--space-1);\
";

const TOGGLE_BTN_STYLE: &str = "\
    background: none; \
    border: 1px solid var(--border, #2e2b27); \
    border-radius: var(--radius-sm, 4px); \
    color: var(--text-secondary, #a8a49e); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TOGGLE_BTN_ACTIVE_STYLE: &str = "\
    background: var(--accent-muted); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-sm); \
    color: var(--text-primary); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const PRIMARY_BTN_STYLE: &str = "\
    background: var(--accent); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-sm); \
    color: var(--text-inverse); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const STAT_STYLE: &str = "\
    color: var(--text-muted, #706c66); \
    font-size: var(--text-xs); \
    white-space: nowrap;\
";

const DIRTY_DOT_STYLE: &str = "\
    color: var(--status-warning); \
    font-size: var(--text-sm); \
    line-height: 1;\
";

/// Toolbar for the file viewer.
///
/// `mode` carries the current view mode. `is_markdown` gates the
/// Preview/Source/Edit affordances (only meaningful for markdown). `dirty`
/// and `saving` drive the editor save controls; `on_save` is invoked when the
/// operator commits an edit. `word_wrap` only applies in Source mode.
#[component]
pub(crate) fn ViewerToolbar(
    path: String,
    line_count: usize,
    byte_size: usize,
    word_wrap: Signal<bool>,
    is_markdown: bool,
    mode: Signal<ViewMode>,
    dirty: Signal<bool>,
    saving: Signal<bool>,
    on_save: EventHandler<()>,
) -> Element {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let is_wrap = *word_wrap.read();
    let wrap_label = if is_wrap { "Wrap: On" } else { "Wrap: Off" };
    let mut copied = use_signal(|| false);

    let current_mode = *mode.read();
    let is_dirty = *dirty.read();
    let is_saving = *saving.read();

    rsx! {
        div {
            style: "{TOOLBAR_STYLE}",
            // Breadcrumb
            div {
                style: "{BREADCRUMB_STYLE}",
                for (i, segment) in segments.iter().enumerate() {
                    if i > 0 {
                        span { style: "{CRUMB_SEPARATOR}", "/" }
                    }
                    span {
                        style: "{CRUMB_STYLE}",
                        "{segment}"
                    }
                }
                if is_dirty {
                    span {
                        style: "{DIRTY_DOT_STYLE}",
                        title: "Unsaved changes",
                        "\u{25CF}"
                    }
                }
            }
            // Stats (hidden while editing to keep the bar focused)
            if line_count > 0 && current_mode != ViewMode::Edit {
                span {
                    style: "{STAT_STYLE}",
                    "{line_count} lines"
                }
                span {
                    style: "{STAT_STYLE}",
                    "{format_byte_size(byte_size)}"
                }
            }
            // Markdown view-mode controls.
            if is_markdown {
                button {
                    style: if current_mode == ViewMode::Preview { TOGGLE_BTN_ACTIVE_STYLE } else { TOGGLE_BTN_STYLE },
                    "aria-pressed": if current_mode == ViewMode::Preview { "true" } else { "false" },
                    onclick: move |_| mode.set(ViewMode::Preview),
                    "Preview"
                }
                button {
                    style: if current_mode == ViewMode::Source { TOGGLE_BTN_ACTIVE_STYLE } else { TOGGLE_BTN_STYLE },
                    "aria-pressed": if current_mode == ViewMode::Source { "true" } else { "false" },
                    onclick: move |_| mode.set(ViewMode::Source),
                    "Source"
                }
                button {
                    style: if current_mode == ViewMode::Edit { TOGGLE_BTN_ACTIVE_STYLE } else { TOGGLE_BTN_STYLE },
                    "aria-pressed": if current_mode == ViewMode::Edit { "true" } else { "false" },
                    onclick: move |_| mode.set(ViewMode::Edit),
                    "Edit"
                }
            }
            // Save (edit mode only).
            if current_mode == ViewMode::Edit {
                button {
                    style: "{PRIMARY_BTN_STYLE}",
                    disabled: !is_dirty || is_saving,
                    onclick: move |_| on_save.call(()),
                    if is_saving { "Saving\u{2026}" } else { "Save" }
                }
            }
            // Word wrap toggle (source views only -- preview/edit manage their own flow).
            if current_mode == ViewMode::Source {
                button {
                    style: "{TOGGLE_BTN_STYLE}",
                    onclick: move |_| {
                        let current = *word_wrap.read();
                        word_wrap.set(!current);
                    },
                    "{wrap_label}"
                }
            }
            // Copy path
            button {
                style: "{TOGGLE_BTN_STYLE}",
                onclick: {
                    let path = path.clone();
                    move |_| {
                        let path = path.clone();
                        copied.set(true);
                        spawn(async move {
                            // WHY: Use eval to copy to clipboard via browser API since
                            // Dioxus desktop uses webview.
                            let js = format!(
                                "navigator.clipboard.writeText('{}')",
                                path.replace('\'', "\\'")
                            );
                            if document::eval(&js).await.is_err() {
                                tracing::warn!("failed to copy file path to clipboard");
                                copied.set(false);
                                return;
                            }
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                            copied.set(false);
                        });
                    }
                },
                if *copied.read() { "Copied!" } else { "Copy Path" }
            }
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "display-only: sub-byte precision irrelevant"
)]
fn format_byte_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64) // kanon:ignore RUST/as-cast
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64) // kanon:ignore RUST/as-cast
    }
}
