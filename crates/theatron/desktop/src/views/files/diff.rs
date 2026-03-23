//! Diff viewer: fetches and displays file diffs with unified and side-by-side modes.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::diff_hunk::DiffHunkView;
use crate::state::connection::ConnectionConfig;
use crate::state::diff::{DiffFile, DiffViewMode, parse_unified_diff};
use crate::state::fetch::FetchState;

const TOOLBAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: 8px 0;\
";

const STATS_STYLE: &str = "\
    display: flex; \
    gap: 12px; \
    font-size: 13px;\
";

const TOGGLE_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const BACK_BTN: &str = "\
    background: none; \
    color: #7a7aff; \
    border: none; \
    font-size: 13px; \
    cursor: pointer; \
    padding: 0;\
";

const PATH_STYLE: &str = "\
    font-size: 16px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    color: #888; \
    font-size: 14px; \
    padding: 32px;\
";

const DIFF_CONTAINER_STYLE: &str = "\
    flex: 1; \
    overflow: auto; \
    border: 1px solid #333; \
    border-radius: 8px; \
    background: #1a1a2e;\
";

/// Diff viewer component.
///
/// Fetches the unified diff for `path` from the workspace API and renders
/// it with mode toggle, stats, and syntax-highlighted hunks.
#[component]
pub(crate) fn DiffViewer(path: String, on_back: EventHandler<()>) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut diff_state = use_signal(|| FetchState::<DiffFile>::Loading);
    let mut view_mode = use_signal(DiffViewMode::default);

    let path_clone = path.clone();
    use_effect(move || {
        let cfg = config.read().clone();
        let p = path_clone.clone();
        diff_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');
            let encoded: String = form_urlencoded::byte_serialize(p.as_bytes()).collect();
            let url = format!("{base}/api/v1/workspace/diff?path={encoded}");

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.text().await {
                    Ok(text) => {
                        let parsed = parse_unified_diff(&p, &text);
                        diff_state.set(FetchState::Loaded(parsed));
                    }
                    Err(e) => {
                        diff_state.set(FetchState::Error(format!("read error: {e}")));
                    }
                },
                Ok(resp) => {
                    let status = resp.status();
                    diff_state.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    diff_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    let language = detect_language_from_path(&path);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%; gap: 8px;",
            // Navigation
            button {
                style: "{BACK_BTN}",
                onclick: move |_| on_back.call(()),
                "← Back to Files"
            }
            // Path header
            div { style: "{PATH_STYLE}", "{path}" }
            // Toolbar: stats + mode toggle
            div {
                style: "{TOOLBAR_STYLE}",
                match &*diff_state.read() {
                    FetchState::Loaded(diff) => rsx! {
                        div {
                            style: "{STATS_STYLE}",
                            span { style: "color: #22c55e;", "+{diff.additions}" }
                            span { style: "color: #ef4444;", "-{diff.deletions}" }
                        }
                    },
                    _ => rsx! { div {} },
                }
                button {
                    style: "{TOGGLE_BTN}",
                    onclick: move |_| {
                        let current = *view_mode.read();
                        view_mode.set(current.toggle());
                    },
                    {
                        let mode = *view_mode.read();
                        match mode {
                            DiffViewMode::Unified => "Switch to Side-by-Side",
                            DiffViewMode::SideBySide => "Switch to Unified",
                        }
                    }
                }
            }
            // Diff content
            match &*diff_state.read() {
                FetchState::Loading => rsx! {
                    div { style: "{STATUS_STYLE}", "Loading diff..." }
                },
                FetchState::Error(err) => rsx! {
                    div { style: "{STATUS_STYLE} color: #ef4444;", "Error: {err}" }
                },
                FetchState::Loaded(diff) => {
                    if diff.hunks.is_empty() {
                        rsx! {
                            div { style: "{STATUS_STYLE}", "No changes found" }
                        }
                    } else {
                        let mode = *view_mode.read();
                        rsx! {
                            div {
                                style: "{DIFF_CONTAINER_STYLE}",
                                for (i , hunk) in diff.hunks.iter().enumerate() {
                                    DiffHunkView {
                                        key: "{i}",
                                        hunk: hunk.clone(),
                                        language: language.clone(),
                                        mode,
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Infer language from file extension for syntax highlighting.
fn detect_language_from_path(path: &str) -> String {
    path.rsplit('.')
        .next()
        .map(|ext| match ext {
            "rs" => "rust",
            "py" => "python",
            "js" => "javascript",
            "ts" => "typescript",
            "tsx" => "typescript",
            "jsx" => "javascript",
            "md" => "markdown",
            "toml" => "toml",
            "yaml" | "yml" => "yaml",
            "json" => "json",
            "sh" | "bash" => "bash",
            "css" => "css",
            "html" => "html",
            "sql" => "sql",
            "go" => "go",
            "rb" => "ruby",
            "java" => "java",
            "c" | "h" => "c",
            "cpp" | "hpp" | "cc" => "cpp",
            other => other,
        })
        .unwrap_or("text")
        .to_string()
}
