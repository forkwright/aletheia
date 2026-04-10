//! Tool detail drill-down: usage, success rate, duration, and recent invocations.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::tool_metrics::{
    DateRange, ToolInvocation, ToolStat, ToolStatsResponse, format_duration_ms, page_count,
    paginate,
};

const INVOCATIONS_PER_PAGE: usize = 20;

// -- Style constants ----------------------------------------------------------

const BACK_BTN: &str = "\
    background: transparent; \
    color: #4a4aff; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const TABLE_STYLE: &str = "\
    width: 100%; \
    border-collapse: collapse; \
    font-size: 12px;\
";

const TH_STYLE: &str = "\
    text-align: left; \
    color: #888; \
    font-weight: normal; \
    padding: 4px 8px; \
    border-bottom: 1px solid #333;\
";

const TD_STYLE: &str = "\
    padding: 4px 8px; \
    border-bottom: 1px solid #1e1e38; \
    color: #ccc;\
";

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 14px 18px; \
    min-width: 130px; \
    flex: 1;\
";

const CARD_VALUE: &str = "\
    font-size: 24px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin-bottom: 2px;\
";

const CARD_LABEL: &str = "\
    font-size: 11px; \
    color: #888; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const PAGE_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 4px; \
    padding: 2px 8px; \
    font-size: 12px; \
    cursor: pointer;\
";

const PAGE_BTN_DISABLED: &str = "\
    background: #1a1a2e; \
    color: #444; \
    border: 1px solid #2a2a2a; \
    border-radius: 4px; \
    padding: 2px 8px; \
    font-size: 12px; \
    cursor: default;\
";

// -- Component ----------------------------------------------------------------

#[component]
pub(crate) fn ToolDetailView(
    tool_name: String,
    date_range: DateRange,
    on_back: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut detail_fetch = use_signal(|| FetchState::<ToolStatsResponse>::Loading);
    let mut detail_page = use_signal(|| 0usize);

    let tool_name_for_effect = tool_name.clone();
    use_effect(move || {
        let cfg = config.read().clone();
        let name = tool_name_for_effect.clone();
        detail_fetch.set(FetchState::Loading);
        detail_page.set(0);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');
            let days = date_range.days();
            let encoded = form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>();
            let url = format!("{base}/api/tool-stats?days={days}&tool={encoded}");

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<ToolStatsResponse>().await {
                        Ok(data) => detail_fetch.set(FetchState::Loaded(data)),
                        Err(e) => {
                            detail_fetch.set(FetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                Ok(resp) => {
                    detail_fetch.set(FetchState::Error(format!(
                        "tool-stats returned {}",
                        resp.status()
                    )));
                }
                Err(e) => {
                    detail_fetch.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    rsx! {
        div { style: "display: flex; flex-direction: column; gap: 16px;",

            // Header: back button + tool name
            div {
                style: "display: flex; align-items: center; gap: 12px;",
                button {
                    style: "{BACK_BTN}",
                    onclick: move |_| on_back.call(()),
                    "← Back"
                }
                h3 {
                    style: "font-size: 16px; margin: 0; font-family: monospace; color: #e0e0e0;",
                    "{tool_name}"
                }
            }

            match &*detail_fetch.read() {
                FetchState::Loading => rsx! {
                    div { style: "color: #888; padding: 8px;", "Loading tool detail..." }
                },
                FetchState::Error(err) => rsx! {
                    div { style: "color: #ef4444; padding: 8px;", "Error: {err}" }
                },
                FetchState::Loaded(data) => {
                    let stat = data.tools.iter().find(|t| t.name == tool_name);
                    let inv_count = data.invocations.len();
                    let page = *detail_page.read();
                    rsx! {
                        {render_summary_cards(stat)}
                        div {
                            style: "font-size: 13px; color: #aaa; margin-bottom: 4px;",
                            "Recent Invocations"
                        }
                        {render_invocations_table(&data.invocations, page)}
                        {render_pagination(page, inv_count, detail_page)}
                    }
                }
            }
        }
    }
}

fn render_summary_cards(stat: Option<&ToolStat>) -> Element {
    let Some(stat) = stat else {
        return rsx! { div { style: "color: #555; font-size: 12px;", "No stats for this tool." } };
    };

    let rate = if stat.total > 0 {
        (stat.succeeded * 100) / stat.total
    } else {
        0
    };
    let rate_color = if rate > 90 {
        "#22c55e"
    } else if rate > 70 {
        "#eab308"
    } else {
        "#ef4444"
    };

    rsx! {
        div {
            style: "display: flex; flex-wrap: wrap; gap: 10px;",
            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{stat.total}" }
                div { style: "{CARD_LABEL}", "Total Calls" }
            }
            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE} color: {rate_color};", "{rate}%" }
                div { style: "{CARD_LABEL}", "Success Rate" }
            }
            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{format_duration_ms(stat.p50_ms)}" }
                div { style: "{CARD_LABEL}", "Median Duration" }
            }
            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{format_duration_ms(stat.p95_ms)}" }
                div { style: "{CARD_LABEL}", "p95 Duration" }
            }
        }
    }
}

fn render_invocations_table(invocations: &[ToolInvocation], page: usize) -> Element {
    if invocations.is_empty() {
        return rsx! {
            div { style: "color: #555; font-size: 12px; padding: 8px;", "No invocations recorded." }
        };
    }

    let page_items = paginate(invocations, page, INVOCATIONS_PER_PAGE);

    rsx! {
        table {
            style: "{TABLE_STYLE}",
            thead {
                tr {
                    th { style: "{TH_STYLE}", "Timestamp" }
                    th { style: "{TH_STYLE}", "Agent" }
                    th { style: "{TH_STYLE}", "Duration" }
                    th { style: "{TH_STYLE}", "Result" }
                    th { style: "{TH_STYLE}", "Error" }
                }
            }
            tbody {
                for (idx, inv) in page_items.iter().enumerate() {
                    {
                        let (result_text, result_color) = if inv.success {
                            ("✓", "#22c55e")
                        } else {
                            ("✗", "#ef4444")
                        };
                        let error = inv.error.as_deref().unwrap_or("—");
                        let dur = format_duration_ms(inv.duration_ms);
                        let ts = inv.timestamp.clone();
                        let agent = inv.agent_id.clone();
                        let key = format!("{idx}-{ts}");

                        rsx! {
                            tr {
                                key: "{key}",
                                td { style: "{TD_STYLE} font-size: 11px; color: #666;", "{ts}" }
                                td { style: "{TD_STYLE} font-family: monospace; font-size: 11px;", "{agent}" }
                                td { style: "{TD_STYLE}", "{dur}" }
                                td { style: "{TD_STYLE} color: {result_color}; font-weight: bold;", "{result_text}" }
                                td {
                                    style: "{TD_STYLE} font-size: 11px; color: #888; max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    "{error}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_pagination(page: usize, total_items: usize, mut detail_page: Signal<usize>) -> Element {
    let total_pages = page_count(total_items, INVOCATIONS_PER_PAGE);
    if total_pages <= 1 {
        return rsx! {};
    }

    rsx! {
        div {
            style: "display: flex; align-items: center; gap: 8px; font-size: 12px; color: #888;",
            button {
                style: if page > 0 { PAGE_BTN } else { PAGE_BTN_DISABLED },
                disabled: page == 0,
                onclick: move |_| {
                    let p = *detail_page.read();
                    if p > 0 {
                        detail_page.set(p - 1);
                    }
                },
                "← Prev"
            }
            span { "Page {page + 1} of {total_pages}" }
            button {
                style: if page + 1 < total_pages { PAGE_BTN } else { PAGE_BTN_DISABLED },
                disabled: page + 1 >= total_pages,
                onclick: move |_| {
                    let p = *detail_page.read();
                    if p + 1 < total_pages {
                        detail_page.set(p + 1);
                    }
                },
                "Next →"
            }
        }
    }
}
