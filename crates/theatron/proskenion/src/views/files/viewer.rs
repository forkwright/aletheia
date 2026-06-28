//! File viewer: markdown preview/edit for vault notes, gramma-highlighted source for code.

use dioxus::prelude::*;

use gramma::{HighlightedSpan, highlight_code};

use crate::api::client::{
    SaveOutcome, authenticated_client, open_workspace_file, save_workspace_file,
};
use crate::components::markdown::Markdown;
use crate::state::connection::ConnectionConfig;
use crate::state::files::{is_binary_content, is_markdown_path};
use crate::state::toasts::{ToastSeverity, ToastStore};
use crate::views::files::toolbar::{ViewMode, ViewerToolbar};

const VIEWER_CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    flex: 1; \
    overflow: hidden; \
    background: var(--bg-surface, #1a1816); \
    border: 1px solid var(--border, #2e2b27); \
    border-radius: var(--radius-md, 6px);\
";

const CODE_AREA_STYLE: &str = "\
    display: flex; \
    flex: 1; \
    overflow: auto; \
    font-family: var(--font-mono, monospace); \
    font-size: var(--text-sm); \
    line-height: var(--leading-normal);\
";

const GUTTER_STYLE: &str = "\
    padding: var(--space-3) var(--space-2) var(--space-3) var(--space-3); \
    text-align: right; \
    color: var(--text-muted, #706c66); \
    user-select: none; \
    flex-shrink: 0; \
    min-width: 40px; \
    border-right: 1px solid var(--border-separator, #221f1c);\
";

const CODE_STYLE_WRAP: &str = "\
    padding: var(--space-3); \
    flex: 1; \
    white-space: pre-wrap; \
    word-wrap: break-word; \
    color: var(--code-fg, #d4d0ca);\
";

const CODE_STYLE_NOWRAP: &str = "\
    padding: var(--space-3); \
    flex: 1; \
    white-space: pre; \
    overflow-x: auto; \
    color: var(--code-fg, #d4d0ca);\
";

const MARKDOWN_PREVIEW_STYLE: &str = "\
    flex: 1; \
    overflow: auto; \
    padding: var(--space-5) var(--space-6); \
    max-width: 80ch; \
    color: var(--text-primary, #d4d0ca);\
";

const EDIT_SPLIT_STYLE: &str = "\
    display: flex; \
    flex: 1; \
    overflow: hidden;\
";

const EDIT_PANE_STYLE: &str = "\
    flex: 1; \
    display: flex; \
    flex-direction: column; \
    overflow: hidden; \
    border-right: 1px solid var(--border-separator, #221f1c);\
";

const PREVIEW_PANE_STYLE: &str = "\
    flex: 1; \
    overflow: auto; \
    padding: var(--space-5) var(--space-6); \
    color: var(--text-primary, #d4d0ca);\
";

const TEXTAREA_STYLE: &str = "\
    flex: 1; \
    width: 100%; \
    resize: none; \
    border: none; \
    outline: none; \
    padding: var(--space-3); \
    background: var(--bg-surface, #1a1816); \
    color: var(--code-fg, #d4d0ca); \
    font-family: var(--font-mono, monospace); \
    font-size: var(--text-sm); \
    line-height: var(--leading-normal); \
    tab-size: 4;\
";

const EMPTY_STATE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: var(--text-muted, #706c66); \
    font-size: var(--text-base);\
";

const BINARY_PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    gap: var(--space-3); \
    flex: 1; \
    color: var(--text-muted, #706c66); \
    font-size: var(--text-base);\
";

const OPEN_EXTERNAL_BTN_STYLE: &str = "\
    background: var(--accent); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-sm); \
    color: var(--text-inverse); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const CONFLICT_BANNER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    background: var(--accent-muted); \
    border-bottom: 1px solid var(--status-warning); \
    color: var(--text-primary); \
    font-size: var(--text-xs); \
    flex-shrink: 0;\
";

#[derive(Debug, Clone)]
enum ViewerState {
    Empty,
    Loading,
    Binary {
        path: String,
    },
    Loaded {
        path: String,
        content: String,
        line_count: usize,
        byte_size: usize,
    },
    Error(String),
}

#[component]
pub(crate) fn FileViewer(mut selected_path: Signal<Option<String>>) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut state = use_signal(|| ViewerState::Empty);
    let word_wrap = use_signal(|| true);
    let mode = use_signal(|| ViewMode::Preview);
    let mut last_loaded_path = use_signal(String::new);

    // Edit-buffer state: the working draft, the dirty flag, the in-flight save
    // guard, and a conflict marker that survives a failed save.
    let draft = use_signal(String::new);
    let dirty = use_signal(|| false);
    let saving = use_signal(|| false);
    let conflict = use_signal(|| false);
    // WHY: the external-open guard lives at component scope so its hook order
    // stays stable across viewer-state transitions (hooks must not be called
    // from a conditionally-rendered branch).
    let opening = use_signal(|| false);

    // NOTE: Fetch file content when selected_path changes.
    use_effect(move || {
        let path = selected_path.read().clone();
        if let Some(path) = path {
            let previous = last_loaded_path.read().clone();
            if previous == path {
                return;
            }
            // WHY: unsaved-changes guard. If the operator navigates to another
            // file with a dirty edit buffer, confirm before discarding it;
            // declining restores the prior selection so no edits are lost.
            if *dirty.read() && !previous.is_empty() {
                spawn(async move {
                    if confirm_discard().await {
                        load_file(
                            config,
                            last_loaded_path,
                            state,
                            draft,
                            dirty,
                            conflict,
                            mode,
                            path,
                        );
                    } else {
                        // NOTE: revert selection; setting back to the loaded path
                        // re-runs this effect as a no-op (previous == path).
                        selected_path.set(Some(previous));
                    }
                });
                return;
            }
            load_file(
                config,
                last_loaded_path,
                state,
                draft,
                dirty,
                conflict,
                mode,
                path,
            );
        } else {
            state.set(ViewerState::Empty);
            last_loaded_path.set(String::new());
        }
    });

    match &*state.read() {
        ViewerState::Empty => rsx! {
            div {
                style: "{VIEWER_CONTAINER_STYLE}",
                div {
                    style: "{EMPTY_STATE_STYLE}",
                    "Select a file to view"
                }
            }
        },
        ViewerState::Loading => rsx! {
            div {
                style: "{VIEWER_CONTAINER_STYLE}",
                div {
                    style: "{EMPTY_STATE_STYLE}",
                    "Loading..."
                }
            }
        },
        ViewerState::Binary { path } => {
            let path = path.clone();
            rsx! {
                div {
                    style: "{VIEWER_CONTAINER_STYLE}",
                    ViewerToolbar {
                        path: path.clone(),
                        line_count: 0,
                        byte_size: 0,
                        word_wrap,
                        is_markdown: false,
                        mode,
                        dirty,
                        saving,
                        on_save: move |_| {},
                    }
                    {render_binary_panel(config, path, opening)}
                }
            }
        }
        ViewerState::Loaded {
            path,
            content,
            line_count,
            byte_size,
        } => {
            let path = path.clone();
            let content = content.clone();
            let lc = *line_count;
            let bs = *byte_size;
            let is_md = is_markdown_path(&path);
            let current_mode = *mode.read();

            let save_path = path.clone();
            let on_save = move |_| {
                trigger_save(
                    config,
                    save_path.clone(),
                    draft,
                    dirty,
                    saving,
                    conflict,
                    state,
                );
            };

            rsx! {
                div {
                    style: "{VIEWER_CONTAINER_STYLE}",
                    ViewerToolbar {
                        path: path.clone(),
                        line_count: lc,
                        byte_size: bs,
                        word_wrap,
                        is_markdown: is_md,
                        mode,
                        dirty,
                        saving,
                        on_save,
                    }
                    if *conflict.read() {
                        div {
                            style: "{CONFLICT_BANNER_STYLE}",
                            span { "\u{26A0}\u{FE0F}" }
                            span {
                                "This file changed on disk since you opened it. \
                                 Saving will overwrite those changes -- reselect the file to reload first."
                            }
                        }
                    }
                    {render_body(current_mode, &path, &content, word_wrap, draft, dirty)}
                }
            }
        }
        ViewerState::Error(err) => rsx! {
            div {
                style: "{VIEWER_CONTAINER_STYLE}",
                div {
                    style: "{EMPTY_STATE_STYLE} color: var(--status-error, #A04040);",
                    "Error: {err}"
                }
            }
        },
    }
}

/// Prompt the operator to confirm discarding unsaved edits.
///
/// WHY: the desktop is a webview, so a native `window.confirm` is the
/// blocking yes/no the operator already expects. Returns `true` to discard.
/// A failed eval defaults to `true` (proceed) rather than wedging navigation.
async fn confirm_discard() -> bool {
    let js = "window.confirm('Discard unsaved changes to this note?')";
    match document::eval(js).await {
        Ok(val) => val.as_bool().unwrap_or(true),
        Err(_) => true,
    }
}

/// Fetch a workspace file and populate the viewer state.
///
/// Resets edit state, picks the default mode (Preview for markdown, Source
/// otherwise), then loads content asynchronously. Binary content routes to
/// the binary panel; UTF-8 content seeds the draft buffer for editing.
#[expect(
    clippy::too_many_arguments,
    reason = "load threads the viewer's signal set shared with the change effect"
)]
fn load_file(
    config: Signal<ConnectionConfig>,
    mut last_loaded_path: Signal<String>,
    mut state: Signal<ViewerState>,
    mut draft: Signal<String>,
    mut dirty: Signal<bool>,
    mut conflict: Signal<bool>,
    mut mode: Signal<ViewMode>,
    path: String,
) {
    last_loaded_path.set(path.clone());
    state.set(ViewerState::Loading);
    dirty.set(false);
    conflict.set(false);
    mode.set(if is_markdown_path(&path) {
        ViewMode::Preview
    } else {
        ViewMode::Source
    });

    let cfg = config.read().clone();
    spawn(async move {
        let client = match authenticated_client(&cfg) {
            Ok(client) => client,
            Err(err) => {
                state.set(ViewerState::Error(err.to_string()));
                return;
            }
        };
        let base = cfg.server_url.trim_end_matches('/');
        let encoded: String = keryx::url::encode_path_segment(&path);
        let url = format!("{base}/api/v1/workspace/files/content?path={encoded}");

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    if is_binary_content(&bytes) {
                        state.set(ViewerState::Binary { path });
                    } else {
                        let content = String::from_utf8_lossy(&bytes).into_owned();
                        let line_count = content.lines().count();
                        let byte_size = bytes.len();
                        draft.set(content.clone());
                        state.set(ViewerState::Loaded {
                            path,
                            content,
                            line_count,
                            byte_size,
                        });
                    }
                }
                Err(e) => {
                    state.set(ViewerState::Error(format!("read: {e}")));
                }
            },
            Ok(resp) => {
                state.set(ViewerState::Error(format!("status: {}", resp.status())));
            }
            Err(e) => {
                state.set(ViewerState::Error(format!("connection: {e}")));
            }
        }
    });
}

/// Render the viewer body for the current mode.
fn render_body(
    mode: ViewMode,
    path: &str,
    content: &str,
    word_wrap: Signal<bool>,
    draft: Signal<String>,
    dirty: Signal<bool>,
) -> Element {
    match mode {
        ViewMode::Preview => rsx! {
            div {
                style: "{MARKDOWN_PREVIEW_STYLE}",
                Markdown { content: content.to_string() }
            }
        },
        ViewMode::Source => render_source(content, path, word_wrap),
        ViewMode::Edit => render_editor(draft, dirty),
    }
}

/// Render the edit/preview split for markdown editing.
fn render_editor(mut draft: Signal<String>, mut dirty: Signal<bool>) -> Element {
    let draft_text = draft.read().clone();
    let preview_text = draft_text.clone();
    rsx! {
        div {
            style: "{EDIT_SPLIT_STYLE}",
            div {
                style: "{EDIT_PANE_STYLE}",
                textarea {
                    style: "{TEXTAREA_STYLE}",
                    "aria-label": "Markdown editor",
                    spellcheck: "false",
                    value: "{draft_text}",
                    oninput: move |evt| {
                        draft.set(evt.value());
                        dirty.set(true);
                    },
                }
            }
            div {
                style: "{PREVIEW_PANE_STYLE}",
                Markdown { content: preview_text }
            }
        }
    }
}

/// Render gramma-highlighted source with a line-number gutter.
fn render_source(content: &str, path: &str, word_wrap: Signal<bool>) -> Element {
    let highlighted = highlight_code(content, gramma::syntax::language_from_path(path));
    let code_style = if *word_wrap.read() {
        CODE_STYLE_WRAP
    } else {
        CODE_STYLE_NOWRAP
    };
    let line_count = content.lines().count();

    rsx! {
        div {
            style: "{CODE_AREA_STYLE}",
            pre {
                style: "{GUTTER_STYLE}",
                for i in 1..=line_count {
                    "{i}\n"
                }
            }
            pre {
                style: "{code_style}",
                for line in highlighted {
                    {render_highlighted_line(&line)}
                }
            }
        }
    }
}

/// Render the binary-file panel with an "Open externally" action.
fn render_binary_panel(
    config: Signal<ConnectionConfig>,
    path: String,
    mut opening: Signal<bool>,
) -> Element {
    let is_opening = *opening.read();

    rsx! {
        div {
            style: "{BINARY_PANEL_STYLE}",
            span { "Binary file \u{2014} cannot display inline" }
            button {
                style: "{OPEN_EXTERNAL_BTN_STYLE}",
                disabled: is_opening,
                onclick: move |_| {
                    let cfg = config.read().clone();
                    let p = path.clone();
                    opening.set(true);
                    spawn(async move {
                        let result = open_workspace_file(&cfg, &p).await;
                        opening.set(false);
                        if let Some(mut toasts) = try_consume_context::<Signal<ToastStore>>() {
                            match result {
                                Ok(()) => {
                                    toasts.write().push(
                                        ToastSeverity::Info,
                                        "Opened in default app",
                                    );
                                }
                                Err(e) => {
                                    toasts.write().push_full(
                                        ToastSeverity::Error,
                                        "Could not open file".to_string(),
                                        Some(e),
                                        None,
                                    );
                                }
                            }
                        }
                    });
                },
                if is_opening { "Opening\u{2026}" } else { "Open externally" }
            }
        }
    }
}

/// Persist the current draft, mapping the result to dirty/conflict/toast UX.
fn trigger_save(
    config: Signal<ConnectionConfig>,
    path: String,
    draft: Signal<String>,
    mut dirty: Signal<bool>,
    mut saving: Signal<bool>,
    mut conflict: Signal<bool>,
    mut state: Signal<ViewerState>,
) {
    if *saving.read() {
        return;
    }
    let cfg = config.read().clone();
    let content = draft.read().clone();
    saving.set(true);

    spawn(async move {
        let outcome = save_workspace_file(&cfg, &path, &content).await;
        saving.set(false);

        match outcome {
            SaveOutcome::Saved => {
                dirty.set(false);
                conflict.set(false);
                // WHY: refresh the canonical Loaded snapshot so a later
                // conflict check compares against what we just wrote.
                let line_count = content.lines().count();
                let byte_size = content.len();
                state.set(ViewerState::Loaded {
                    path: path.clone(),
                    content,
                    line_count,
                    byte_size,
                });
                if let Some(mut toasts) = try_consume_context::<Signal<ToastStore>>() {
                    toasts.write().push(ToastSeverity::Info, "Saved");
                }
            }
            SaveOutcome::Conflict => {
                conflict.set(true);
                if let Some(mut toasts) = try_consume_context::<Signal<ToastStore>>() {
                    toasts.write().push_full(
                        ToastSeverity::Warning,
                        "Save conflict".to_string(),
                        Some("The file changed on disk. Reload before saving.".to_string()),
                        None,
                    );
                }
            }
            SaveOutcome::TooLarge => {
                if let Some(mut toasts) = try_consume_context::<Signal<ToastStore>>() {
                    toasts.write().push_full(
                        ToastSeverity::Error,
                        "File too large to save".to_string(),
                        Some("The note exceeds the server's size limit.".to_string()),
                        None,
                    );
                }
            }
            SaveOutcome::Failed(msg) => {
                if let Some(mut toasts) = try_consume_context::<Signal<ToastStore>>() {
                    toasts.write().push_full(
                        ToastSeverity::Error,
                        "Save failed".to_string(),
                        Some(msg),
                        None,
                    );
                }
            }
        }
    });
}

fn render_highlighted_line(line: &[HighlightedSpan]) -> Element {
    rsx! {
        div {
            style: "min-height: 1.5em;",
            for span in line {
                span {
                    style: "color: {span.color};{bold_style(span.bold)}{italic_style(span.italic)}",
                    "{span.text}"
                }
            }
        }
    }
}

fn bold_style(bold: bool) -> &'static str {
    if bold {
        " font-weight: var(--weight-bold);"
    } else {
        ""
    }
}

fn italic_style(italic: bool) -> &'static str {
    if italic { " font-style: italic;" } else { "" }
}
