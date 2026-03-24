//! Token usage view: time series chart, summary cards, agent/model breakdowns.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::chart::{TimeSeriesChart, TimeSeriesColumn};
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::metrics::{
    compute_delta_u64, format_tokens, DateRange, Granularity, TokenMetricsResponse,
};

use super::agent_breakdown::AgentBreakdown;
use super::model_breakdown::ModelBreakdown;

const SECTION_STYLE: &str = "\
    background: #1a1816; \
    border: 1px solid #2a2724; \
    border-radius: 8px; \
    padding: 16px;\
";

const SECTION_TITLE_STYLE: &str = "\
    font-size: 12px; \
    font-weight: 600; \
    color: #a8a49e; \
    text-transform: uppercase; \
    letter-spacing: 0.05em; \
    margin-bottom: 12px; \
    font-family: 'IBM Plex Mono', monospace;\
";

const CARD_STYLE: &str = "\
    background: #1a1816; \
    border: 1px solid #2a2724; \
    border-radius: 8px; \
    padding: 12px 16px; \
    flex: 1;\
";

const CARD_LABEL_STYLE: &str = "\
    font-size: 11px; \
    color: #706c66; \
    margin-bottom: 4px; \
    font-family: 'IBM Plex Mono', monospace;\
";

const CARD_VALUE_STYLE: &str = "\
    font-size: 20px; \
    font-weight: 600; \
    color: #e8e6e3; \
    font-family: 'IBM Plex Mono', monospace;\
";

const CONTROL_BTN_ACTIVE: &str = "\
    padding: 4px 10px; \
    font-size: 12px; \
    background: #2a2724; \
    color: #e8e6e3; \
    border: 1px solid #3a3530; \
    border-radius: 4px; \
    cursor: pointer; \
    font-family: 'IBM Plex Mono', monospace;\
";

const CONTROL_BTN_INACTIVE: &str = "\
    padding: 4px 10px; \
    font-size: 12px; \
    background: transparent; \
    color: #706c66; \
    border: 1px solid transparent; \
    border-radius: 4px; \
    cursor: pointer; \
    font-family: 'IBM Plex Mono', monospace;\
";

/// Max columns to render in the time series chart.
const MAX_CHART_COLS: usize = 120;

/// Token usage tab: summary + chart + breakdowns.
#[component]
pub(crate) fn Tokens() -> Element {
    let config = use_context::<Signal<ConnectionConfig>>();
    let mut fetch_state = use_signal(|| FetchState::<TokenMetricsResponse>::Loading);
    let mut granularity = use_signal(|| Granularity::Daily);
    let mut date_range = use_signal(|| DateRange::Last30Days);
    let mut custom_from = use_signal(|| String::new());
    let mut custom_to = use_signal(|| String::new());
    let agent_filter = use_signal(|| Option::<String>::None);

    use_effect(move || {
        let cfg = config.read().clone();
        let gran = *granularity.read();
        let range = date_range.read().clone();

        spawn(async move {
            fetch_state.set(FetchState::Loading);
            let client = authenticated_client(&cfg);
            let (from, to) = range.to_query_dates();
            let url = format!(
                "{}/api/v1/metrics/tokens?granularity={}&from={}&to={}",
                cfg.server_url.trim_end_matches('/'),
                gran.url_param(),
                from,
                to,
            );
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<TokenMetricsResponse>().await {
                        Ok(data) => fetch_state.set(FetchState::Loaded(data)),
                        Err(e) => fetch_state.set(FetchState::Error(format!("parse error: {e}"))),
                    }
                }
                Ok(resp) => {
                    fetch_state.set(FetchState::Error(format!("server returned {}", resp.status())));
                }
                Err(e) => {
                    fetch_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",

            // Controls row
            div {
                style: "display: flex; gap: 8px; align-items: center; flex-wrap: wrap;",

                // Granularity
                div {
                    style: "display: flex; gap: 4px;",
                    for g in [Granularity::Daily, Granularity::Weekly, Granularity::Monthly] {
                        {
                            let active = *granularity.read() == g;
                            rsx! {
                                button {
                                    style: if active { CONTROL_BTN_ACTIVE } else { CONTROL_BTN_INACTIVE },
                                    onclick: move |_| granularity.set(g),
                                    "{g.label()}"
                                }
                            }
                        }
                    }
                }

                div { style: "width: 1px; height: 20px; background: #2a2724;" }

                // Date range presets
                div {
                    style: "display: flex; gap: 4px;",
                    for r in [DateRange::Last7Days, DateRange::Last30Days, DateRange::Last90Days] {
                        {
                            let is_active = matches!(
                                *date_range.read(),
                                ref dr if std::mem::discriminant(dr) == std::mem::discriminant(&r)
                            );
                            let r2 = r.clone();
                            rsx! {
                                button {
                                    style: if is_active { CONTROL_BTN_ACTIVE } else { CONTROL_BTN_INACTIVE },
                                    onclick: move |_| date_range.set(r2.clone()),
                                    "{r.label()}"
                                }
                            }
                        }
                    }
                    button {
                        style: if matches!(*date_range.read(), DateRange::Custom { .. }) { CONTROL_BTN_ACTIVE } else { CONTROL_BTN_INACTIVE },
                        onclick: move |_| {
                            let f = custom_from.read().clone();
                            let t = custom_to.read().clone();
                            date_range.set(DateRange::Custom { from: f, to: t });
                        },
                        "Custom"
                    }
                }

                // Custom date inputs
                if matches!(*date_range.read(), DateRange::Custom { .. }) {
                    input {
                        style: "padding: 4px 8px; font-size: 12px; background: #1a1816; border: 1px solid #3a3530; border-radius: 4px; color: #e8e6e3; width: 100px; font-family: 'IBM Plex Mono', monospace;",
                        placeholder: "YYYY-MM-DD",
                        value: "{custom_from}",
                        oninput: move |e| {
                            custom_from.set(e.value());
                            let f = e.value();
                            let t = custom_to.read().clone();
                            date_range.set(DateRange::Custom { from: f, to: t });
                        }
                    }
                    span { style: "color: #706c66; font-size: 12px;", "→" }
                    input {
                        style: "padding: 4px 8px; font-size: 12px; background: #1a1816; border: 1px solid #3a3530; border-radius: 4px; color: #e8e6e3; width: 100px; font-family: 'IBM Plex Mono', monospace;",
                        placeholder: "YYYY-MM-DD",
                        value: "{custom_to}",
                        oninput: move |e| {
                            custom_to.set(e.value());
                            let f = custom_from.read().clone();
                            let t = e.value();
                            date_range.set(DateRange::Custom { from: f, to: t });
                        }
                    }
                }
            }

            // Body: loading / error / loaded
            match fetch_state.read().clone() {
                FetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; height: 200px; color: #706c66; font-size: 13px;",
                        "Loading…"
                    }
                },
                FetchState::Error(msg) => rsx! {
                    div {
                        style: "padding: 16px; background: #2a1818; border: 1px solid #7f1d1d; border-radius: 8px; color: #fca5a5; font-size: 13px;",
                        "Error: {msg}"
                    }
                },
                FetchState::Loaded(data) => rsx! {
                    { loaded_tokens_view(data, agent_filter) }
                },
            }
        }
    }
}

fn loaded_tokens_view(
    data: TokenMetricsResponse,
    mut agent_filter: Signal<Option<String>>,
) -> Element {
    let today_d = compute_delta_u64(data.today_total(), data.prev_today_total());
    let week_d = compute_delta_u64(data.week_total(), data.prev_week_total());
    let month_d = compute_delta_u64(data.month_total(), data.prev_month_total());
    let grand_total = data.grand_total_tokens();

    // Build time series columns, downsampled to MAX_CHART_COLS
    let series = &data.series;
    let step = if series.len() > MAX_CHART_COLS {
        series.len() / MAX_CHART_COLS
    } else {
        1
    };
    let columns: Vec<TimeSeriesColumn> = series
        .iter()
        .step_by(step.max(1))
        .map(|pt| TimeSeriesColumn {
            label: pt.date.clone(),
            primary: pt.input_tokens as f64,
            secondary: pt.output_tokens as f64,
            primary_color: "#5b6af0".to_string(),
            secondary_color: "#10b981".to_string(),
        })
        .collect();

    let active_filter = agent_filter.read().clone();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",

            // Summary cards
            div {
                style: "display: flex; gap: 12px; flex-wrap: wrap;",
                { delta_card("Today", &format_tokens(today_d.value as u64), today_d.delta_pct, today_d.is_up) }
                { delta_card("This Week", &format_tokens(week_d.value as u64), week_d.delta_pct, week_d.is_up) }
                { delta_card("This Month", &format_tokens(month_d.value as u64), month_d.delta_pct, month_d.is_up) }
            }

            // Time series chart
            div {
                style: "{SECTION_STYLE}",
                div { style: "{SECTION_TITLE_STYLE}",
                    if let Some(ref name) = active_filter {
                        "Usage Over Time — {name}"
                    } else {
                        "Usage Over Time"
                    }
                }
                TimeSeriesChart {
                    columns,
                    height_px: 160,
                    primary_label: "Input".to_string(),
                    secondary_label: "Output".to_string(),
                }
            }

            // Agent breakdown
            if !data.agents.is_empty() {
                div {
                    style: "{SECTION_STYLE}",
                    div { style: "{SECTION_TITLE_STYLE}", "By Agent" }
                    AgentBreakdown {
                        agents: data.agents.clone(),
                        grand_total,
                        active_filter: active_filter.clone(),
                        on_filter: move |id: Option<String>| agent_filter.set(id),
                    }
                }
            }

            // Model breakdown
            if !data.models.is_empty() {
                div {
                    style: "{SECTION_STYLE}",
                    div { style: "{SECTION_TITLE_STYLE}", "By Model" }
                    ModelBreakdown {
                        models: data.models.clone(),
                        grand_total,
                    }
                }
            }
        }
    }
}

#[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "display-only: value already validated as non-negative f64 from u64 source"
)]
fn delta_card(label: &str, value: &str, delta_pct: f64, is_up: bool) -> Element {
    let arrow = if is_up { "↑" } else { "↓" };
    let delta_color = if is_up { "#22c55e" } else { "#ef4444" };
    let delta_str = format!("{arrow} {delta_pct:.1}%");
    let value = value.to_string();
    let label = label.to_string();
    rsx! {
        div {
            style: "{CARD_STYLE}",
            div { style: "{CARD_LABEL_STYLE}", "{label}" }
            div { style: "{CARD_VALUE_STYLE}", "{value}" }
            if delta_pct > 0.0 {
                div { style: "font-size: 11px; color: {delta_color}; margin-top: 2px; font-family: 'IBM Plex Mono', monospace;", "{delta_str} vs prev" }
            }
        }
    }
}
