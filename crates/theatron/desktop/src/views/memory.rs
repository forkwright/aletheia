//! Memory inspector: list and search knowledge facts.

use dioxus::prelude::*;

use crate::state::connection::ConnectionConfig;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct Fact {
    #[serde(default)]
    entity: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    source: String,
}

#[derive(Debug, Clone)]
enum FetchState {
    Idle,
    Loading,
    Loaded(Vec<Fact>),
    Error(String),
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    gap: 16px;\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    flex-wrap: wrap;\
";

const SEARCH_STYLE: &str = "\
    flex: 1; \
    min-width: 200px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 8px 12px; \
    color: #e0e0e0; \
    font-size: 14px;\
";

const FILTER_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 8px 12px; \
    color: #e0e0e0; \
    font-size: 13px; \
    cursor: pointer;\
";

const FACT_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 12px 16px; \
    display: flex; \
    flex-direction: column; \
    gap: 4px;\
";

const FACT_ENTITY_STYLE: &str = "\
    font-weight: bold; \
    color: #7a7aff; \
    font-size: 13px;\
";

const FACT_CONTENT_STYLE: &str = "\
    color: #e0e0e0; \
    font-size: 14px; \
    line-height: 1.4;\
";

const FACT_META_STYLE: &str = "\
    display: flex; \
    gap: 16px; \
    font-size: 11px; \
    color: #666;\
";

const LIST_STYLE: &str = "\
    flex: 1; \
    overflow-y: auto; \
    display: flex; \
    flex-direction: column; \
    gap: 8px;\
";

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: #888; \
    font-size: 14px;\
";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn Memory() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::Idle);
    let mut search_query = use_signal(String::new);
    let mut min_confidence = use_signal(|| 0.0_f64);

    let mut do_refresh = move || {
        let base_url = config.read().server_url.clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = reqwest::Client::new();
            let url = format!("{}/api/v1/knowledge/facts", base_url.trim_end_matches('/'));

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<Vec<Fact>>().await {
                    Ok(facts) => fetch_state.set(FetchState::Loaded(facts)),
                    Err(e) => {
                        fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                    }
                },
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    fetch_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    };

    use_effect(move || {
        do_refresh();
    });

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            h2 { style: "font-size: 20px; margin: 0;", "Memory" }

            div {
                style: "{HEADER_STYLE}",
                input {
                    style: "{SEARCH_STYLE}",
                    r#type: "text",
                    placeholder: "Search facts...",
                    value: "{search_query}",
                    oninput: move |evt: Event<FormData>| {
                        search_query.set(evt.value().clone());
                    },
                }
                select {
                    style: "{FILTER_STYLE}",
                    value: "{min_confidence}",
                    onchange: move |evt: Event<FormData>| {
                        if let Ok(v) = evt.value().parse::<f64>() {
                            min_confidence.set(v);
                        }
                    },
                    option { value: "0", "All confidence" }
                    option { value: "0.5", "> 50%" }
                    option { value: "0.75", "> 75%" }
                    option { value: "0.9", "> 90%" }
                }
                button {
                    style: "{FILTER_STYLE}",
                    onclick: move |_| do_refresh(),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Idle | FetchState::Loading => rsx! {
                    div { style: "{STATUS_STYLE}", "Loading facts..." }
                },
                FetchState::Error(err) => rsx! {
                    div { style: "{STATUS_STYLE} color: #ef4444;", "Error: {err}" }
                },
                FetchState::Loaded(facts) => {
                    let query = search_query.read().to_lowercase();
                    let threshold = *min_confidence.read();
                    let filtered: Vec<&Fact> = facts
                        .iter()
                        .filter(|f| f.confidence >= threshold)
                        .filter(|f| {
                            query.is_empty()
                                || f.entity.to_lowercase().contains(&query)
                                || f.content.to_lowercase().contains(&query)
                        })
                        .collect();

                    if filtered.is_empty() {
                        rsx! {
                            div { style: "{STATUS_STYLE}", "No facts match the current filters" }
                        }
                    } else {
                        rsx! {
                            div { style: "font-size: 12px; color: #666;",
                                "{filtered.len()} of {facts.len()} facts"
                            }
                            div {
                                style: "{LIST_STYLE}",
                                for (i , fact) in filtered.iter().enumerate() {
                                    div {
                                        key: "{i}",
                                        style: "{FACT_STYLE}",
                                        div { style: "{FACT_ENTITY_STYLE}", "{fact.entity}" }
                                        div { style: "{FACT_CONTENT_STYLE}", "{fact.content}" }
                                        div {
                                            style: "{FACT_META_STYLE}",
                                            span { "{format_confidence(fact.confidence)}" }
                                            if !fact.source.is_empty() {
                                                span { "source: {fact.source}" }
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
    }
}

fn format_confidence(value: f64) -> String {
    format!("confidence: {:.0}%", value * 100.0)
}
