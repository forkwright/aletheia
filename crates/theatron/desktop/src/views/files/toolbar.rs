//! Viewer toolbar: breadcrumb, word wrap toggle, file stats, copy path.

use dioxus::prelude::*;

const TOOLBAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 6px 12px; \
    border-bottom: 1px solid var(--border-separator, #221f1c); \
    font-size: 12px; \
    color: var(--text-secondary, #a8a49e); \
    flex-shrink: 0;\
";

const BREADCRUMB_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 2px; \
    flex: 1; \
    overflow: hidden;\
";

const CRUMB_STYLE: &str = "\
    color: var(--text-secondary, #a8a49e); \
    cursor: pointer; \
    white-space: nowrap;\
";

const CRUMB_SEPARATOR: &str = "\
    color: var(--text-muted, #706c66); \
    margin: 0 2px;\
";

const TOGGLE_BTN_STYLE: &str = "\
    background: none; \
    border: 1px solid var(--border, #2e2b27); \
    border-radius: var(--radius-sm, 4px); \
    color: var(--text-secondary, #a8a49e); \
    padding: 2px 8px; \
    font-size: 11px; \
    cursor: pointer;\
";

const STAT_STYLE: &str = "\
    color: var(--text-muted, #706c66); \
    font-size: 11px; \
    white-space: nowrap;\
";

#[component]
pub(crate) fn ViewerToolbar(
    path: String,
    line_count: usize,
    byte_size: usize,
    word_wrap: Signal<bool>,
) -> Element {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let is_wrap = *word_wrap.read();
    let wrap_label = if is_wrap { "Wrap: On" } else { "Wrap: Off" };
    let mut copied = use_signal(|| false);

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
            }
            // Stats
            if line_count > 0 {
                span {
                    style: "{STAT_STYLE}",
                    "{line_count} lines"
                }
                span {
                    style: "{STAT_STYLE}",
                    "{format_byte_size(byte_size)}"
                }
            }
            // Word wrap toggle
            button {
                style: "{TOGGLE_BTN_STYLE}",
                onclick: move |_| {
                    let current = *word_wrap.read();
                    word_wrap.set(!current);
                },
                "{wrap_label}"
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
                            let _ = document::eval(&js);
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
