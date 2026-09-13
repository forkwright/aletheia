//! File search with debounced query against the workspace search API.

use dioxus::prelude::*;

use skene::api::types::WorkspaceSearchResult as SearchResult;

use crate::state::connection::ConnectionConfig;

const SEARCH_INPUT_STYLE: &str = "\
    width: 100%; \
    padding: var(--space-2) var(--space-3); \
    border: 1px solid var(--border, #2e2b27); \
    border-radius: var(--radius-sm, 4px); \
    background: var(--bg-surface, #1a1816); \
    color: var(--text-primary, var(--text-primary)); \
    font-size: var(--text-sm); \
    outline: none; \
    box-sizing: border-box;\
";

const RESULT_ITEM_STYLE: &str = "\
    padding: var(--space-2) var(--space-3); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    border-radius: var(--radius-sm, 4px); \
    font-size: var(--text-sm); \
    color: var(--text-primary, var(--text-primary)); \
    overflow: hidden; \
    text-overflow: ellipsis; \
    white-space: nowrap;\
";

const DEBOUNCE_MS: u64 = 300;
const SEARCH_LIMIT: usize = 50;

#[component]
pub(crate) fn FileSearch(
    on_select_file: EventHandler<String>,
    is_searching: Signal<bool>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut query = use_signal(String::new);
    let mut results = use_signal(Vec::<SearchResult>::new);
    let mut debounce_generation = use_signal(|| 0u64);
    let mut is_searching = is_searching;

    let mut do_search = move |q: String| {
        if q.is_empty() {
            results.set(Vec::new());
            is_searching.set(false);
            return;
        }
        is_searching.set(true);

        let current_gen = {
            let mut g = debounce_generation.write();
            *g += 1;
            *g
        };

        let cfg = config.read().clone();
        spawn(async move {
            // WHY: debounce by checking generation after sleep.
            tokio::time::sleep(tokio::time::Duration::from_millis(DEBOUNCE_MS)).await;
            if *debounce_generation.read() != current_gen {
                return;
            }

            let Ok(client) =
                skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone())
            else {
                return;
            };

            if let Ok(items) = client.workspace_search(&q, SEARCH_LIMIT).await
                && *debounce_generation.read() == current_gen
            {
                results.set(items);
            }
        });
    };

    // WHY: Clone the results into a local Vec so the signal is not borrowed
    // during iteration, avoiding conflicts with mutable closures inside the
    // loop body.
    let result_snapshot = results.read().clone();
    let has_results = !result_snapshot.is_empty();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: var(--space-1);",
            input {
                style: "{SEARCH_INPUT_STYLE}",
                r#type: "text",
                placeholder: "Search files...",
                value: "{query}",
                oninput: move |evt: Event<FormData>| {
                    let val = evt.value();
                    query.set(val.clone());
                    do_search(val);
                },
                onkeydown: move |evt: Event<KeyboardData>| {
                    if evt.key() == Key::Escape {
                        query.set(String::new());
                        results.set(Vec::new());
                        is_searching.set(false);
                    } else if evt.key() == Key::Enter {
                        let first_path = results.read().first().map(|r| r.path.clone());
                        if let Some(path) = first_path {
                            on_select_file.call(path);
                            query.set(String::new());
                            results.set(Vec::new());
                            is_searching.set(false);
                        }
                    }
                },
            }
            if has_results {
                div {
                    style: "display: flex; flex-direction: column; gap: 1px; max-height: 300px; overflow-y: auto;",
                    for result in result_snapshot {
                        SearchResultItem {
                            result,
                            on_select_file,
                            query,
                            results,
                            is_searching,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SearchResultItem(
    result: SearchResult,
    on_select_file: EventHandler<String>,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    is_searching: Signal<bool>,
) -> Element {
    let path = result.path.clone();
    let display_name = result.path.clone();
    let display_path = if result.snippet.is_empty() {
        result.path.clone()
    } else {
        result.snippet.clone()
    };
    let mut query = query;
    let mut results = results;
    let mut is_searching = is_searching;

    rsx! {
        div {
            style: "{RESULT_ITEM_STYLE}",
            onclick: move |_| {
                on_select_file.call(path.clone());
                query.set(String::new());
                results.set(Vec::new());
                is_searching.set(false);
            },
            div {
                style: "font-weight: var(--weight-medium);",
                "{display_name}"
            }
            div {
                style: "font-size: var(--text-xs); color: var(--text-muted, #706c66);",
                "{display_path}"
            }
        }
    }
}
