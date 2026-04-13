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
    color: var(--accent); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TABLE_STYLE: &str = "\
    width: 100%; \
    border-collapse: collapse; \
    font-size: var(--text-xs);\
";

const TH_STYLE: &str = "\
    text-align: left; \
    color: var(--text-secondary); \
    font-weight: normal; \
    padding: var(--space-1) var(--space-2); \
    border-bottom: 1px solid var(--border);\
";

const TD_STYLE: &str = "\
    padding: var(--space-1) var(--space-2); \
    border-bottom: 1px solid #1e1e38; \
    color: var(--text-primary);\
";

const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4); \
    min-width: 130px; \
    flex: 1;\
";

const CARD_VALUE: &str = "\
    font-size: var(--text-2xl); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary); \
    margin-bottom: var(--space-1);\
";

const CARD_LABEL: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const PAGE_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const PAGE_BTN_DISABLED: &str = "\
    background: var(--bg-surface); \
    color: var(--border); \
    border: 1px solid #2a2a2a; \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-xs); \
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
        div { style: "display: flex; flex-direction: column; gap: var(--space-4);",

            // Header: back button + tool name
            div {
                style: "display: flex; align-items: center; gap: var(--space-3);",
                button {
                    style: "{BACK_BTN}",
                    onclick: move |_| on_back.call(()),
                    "← Back"
                }
                h3 {
                    style: "font-size: var(--text-md); margin: 0; font-family: var(--font-mono); color: var(--text-primary);",
                    "{tool_name}"
                }
            }

            match &*detail_fetch.read() {
                FetchState::Loading => rsx! {
                    div { style: "color: var(--text-secondary); padding: var(--space-2);", "Loading tool detail..." }
                },
                FetchState::Error(err) => rsx! {
                    div { style: "color: var(--status-error); padding: var(--space-2);", "Error: {err}" }
                },
                FetchState::Loaded(data) => {
                    let stat = data.tools.iter().find(|t| t.name == tool_name);
                    let inv_count = data.invocations.len();
                    let page = *detail_page.read();
                    rsx! {
                        {render_summary_cards(stat)}
                        div {
                            style: "font-size: var(--text-sm); color: var(--text-secondary); margin-bottom: var(--space-1);",
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
        return rsx! { div { style: "color: var(--text-muted); font-size: var(--text-xs);", "No stats for this tool." } };
    };

    let rate = if stat.total > 0 {
        (stat.succeeded * 100) / stat.total
    } else {
        0
    };
    let rate_color = if rate > 90 {
        "var(--status-success)"
    } else if rate > 70 {
        "var(--status-warning)"
    } else {
        "var(--status-error)"
    };

    rsx! {
        div {
            style: "display: flex; flex-wrap: wrap; gap: var(--space-3);",
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
            div { style: "color: var(--text-muted); font-size: var(--text-xs); padding: var(--space-2);", "No invocations recorded." }
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
                            ("✓", "var(--status-success)")
                        } else {
                            ("✗", "var(--status-error)")
                        };
                        let error = inv.error.as_deref().unwrap_or("—");
                        let dur = format_duration_ms(inv.duration_ms);
                        let ts = inv.timestamp.clone();
                        let agent = inv.agent_id.clone();
                        let key = format!("{idx}-{ts}");

                        rsx! {
                            tr {
                                key: "{key}",
                                td { style: "{TD_STYLE} font-size: var(--text-xs); color: var(--text-muted);", "{ts}" }
                                td { style: "{TD_STYLE} font-family: var(--font-mono); font-size: var(--text-xs);", "{agent}" }
                                td { style: "{TD_STYLE}", "{dur}" }
                                td { style: "{TD_STYLE} color: {result_color}; font-weight: var(--weight-bold);", "{result_text}" }
                                td {
                                    style: "{TD_STYLE} font-size: var(--text-xs); color: var(--text-secondary); max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
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
            style: "display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-xs); color: var(--text-secondary);",
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
