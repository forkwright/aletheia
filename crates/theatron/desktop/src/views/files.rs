//! Workspace file browser: list and view files via the API.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;

#[derive(Debug, Clone, serde::Deserialize)]
struct FileEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    is_dir: bool,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Clone)]
enum ContentState {
    None,
    Loading,
    Loaded { path: String, content: String },
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    height: 100%; \
    gap: 16px;\
";

const TREE_STYLE: &str = "\
    width: 280px; \
    flex-shrink: 0; \
    overflow-y: auto; \
    display: flex; \
    flex-direction: column; \
    gap: 2px;\
";

const FILE_ITEM_STYLE: &str = "\
    padding: 6px 12px; \
    border-radius: 4px; \
    cursor: pointer; \
    font-size: 13px; \
    color: #e0e0e0; \
    display: flex; \
    align-items: center; \
    gap: 6px;\
";

const CONTENT_STYLE: &str = "\
    flex: 1; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px; \
    overflow: auto; \
    font-family: monospace; \
    font-size: 13px; \
    color: #e0e0e0; \
    white-space: pre-wrap; \
    word-wrap: break-word;\
";

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    color: #888; \
    font-size: 14px;\
";

const REFRESH_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

#[component]
pub(crate) fn Files() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut file_state = use_signal(|| FetchState::<Vec<FileEntry>>::Loading);
    let mut content_state = use_signal(|| ContentState::None);

    let mut fetch_files = move || {
        let cfg = config.read().clone();
        file_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/v1/workspace/files",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Vec<FileEntry>>().await {
                        Ok(files) => file_state.set(FetchState::Loaded(files)),
                        Err(e) => {
                            file_state.set(FetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    file_state.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    file_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    };

    use_effect(move || {
        fetch_files();
    });

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%; gap: 12px;",
            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h2 { style: "font-size: 20px; margin: 0;", "Files" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| fetch_files(),
                    "Refresh"
                }
            }
            div {
                style: "{CONTAINER_STYLE}",
                div {
                    style: "{TREE_STYLE}",
                    match &*file_state.read() {
                        FetchState::Loading => rsx! {
                            div { style: "{STATUS_STYLE}", "Loading..." }
                        },
                        FetchState::Error(err) => rsx! {
                            div { style: "{STATUS_STYLE} color: #ef4444;", "Error: {err}" }
                        },
                        FetchState::Loaded(files) => {
                            if files.is_empty() {
                                rsx! {
                                    div { style: "{STATUS_STYLE}", "No files in workspace" }
                                }
                            } else {
                                rsx! {
                                    for file in files {
                                        {render_file_item(file, &config, &mut content_state)}
                                    }
                                }
                            }
                        }
                    }
                }
                div {
                    style: "{CONTENT_STYLE}",
                    match &*content_state.read() {
                        ContentState::None => rsx! {
                            div { style: "{STATUS_STYLE}", "Select a file to view its content" }
                        },
                        ContentState::Loading => rsx! {
                            div { style: "{STATUS_STYLE}", "Loading file..." }
                        },
                        ContentState::Loaded { path, content } => rsx! {
                            div {
                                div { style: "color: #7a7aff; font-size: 12px; margin-bottom: 8px;",
                                    "{path}"
                                }
                                "{content}"
                            }
                        },
                        ContentState::Error(err) => rsx! {
                            div { style: "{STATUS_STYLE} color: #ef4444;", "Error: {err}" }
                        },
                    }
                }
            }
        }
    }
}

fn render_file_item(
    file: &FileEntry,
    config: &Signal<ConnectionConfig>,
    content_state: &mut Signal<ContentState>,
) -> Element {
    let icon = if file.is_dir { "[D]" } else { "[F]" };
    let name = file.name.clone();
    let path = file.path.clone();
    let is_dir = file.is_dir;
    let size = file.size;
    let cfg = config.read().clone();

    let mut content_state = *content_state;

    rsx! {
        div {
            style: "{FILE_ITEM_STYLE}",
            onclick: move |_| {
                if is_dir {
                    return;
                }
                let path = path.clone();
                let cfg = cfg.clone();
                content_state.set(ContentState::Loading);

                spawn(async move {
                    let client = authenticated_client(&cfg);
                    // WHY: URL-encode the path query parameter to handle spaces
                    // and special characters in file paths.
                    let base = cfg.server_url.trim_end_matches('/');
                    let encoded_path: String = form_urlencoded::byte_serialize(path.as_bytes()).collect();
                    let url = format!(
                        "{base}/api/v1/workspace/files/content?path={encoded_path}",
                    );

                    match client.get(&url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            match resp.text().await {
                                Ok(text) => {
                                    content_state.set(ContentState::Loaded {
                                        path,
                                        content: text,
                                    });
                                }
                                Err(e) => {
                                    content_state.set(ContentState::Error(format!(
                                        "read error: {e}"
                                    )));
                                }
                            }
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            content_state.set(ContentState::Error(format!(
                                "server returned {status}"
                            )));
                        }
                        Err(e) => {
                            content_state
                                .set(ContentState::Error(format!("connection error: {e}")));
                        }
                    }
                });
            },
            span { style: "color: #888;", "{icon}" }
            span { "{name}" }
            if !is_dir {
                span { style: "color: #555; font-size: 11px; margin-left: auto;",
                    "{format_size(size)}"
                }
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes < KB {
        format!("{bytes}B")
    } else if bytes < MB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    }
}
