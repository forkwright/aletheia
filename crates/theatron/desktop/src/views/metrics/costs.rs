//! Cost view: cost summary, trend chart, budget panel, and agent cost breakdown.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::chart::{TimeSeriesChart, TimeSeriesColumn};
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::metrics::{
    budget_bar_color, budget_progress_pct, compute_delta_f64, day_of_month_today,
    days_in_current_month, format_cost, project_month_end, BudgetConfig, CostMetricsResponse,
    DateRange, Granularity,
};

use super::agent_costs::AgentCosts;

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
    color: #eab308; \
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

const MAX_CHART_COLS: usize = 120;

/// Cost tab: summary cards, trend chart, budget panel, agent comparison.
#[component]
pub(crate) fn Costs() -> Element {
    let config = use_context::<Signal<ConnectionConfig>>();
    let mut fetch_state = use_signal(|| FetchState::<CostMetricsResponse>::Loading);
    let mut granularity = use_signal(|| Granularity::Daily);
    let mut date_range = use_signal(|| DateRange::Last30Days);
    let budget = use_signal(|| BudgetConfig::default());
    let budget_input = use_signal(|| String::new());

    use_effect(move || {
        let cfg = config.read().clone();
        let gran = *granularity.read();
        let range = date_range.read().clone();

        spawn(async move {
            fetch_state.set(FetchState::Loading);
            let client = authenticated_client(&cfg);
            let (from, to) = range.to_query_dates();
            let url = format!(
                "{}/api/v1/metrics/costs?granularity={}&from={}&to={}",
                cfg.server_url.trim_end_matches('/'),
                gran.url_param(),
                from,
                to,
            );
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<CostMetricsResponse>().await {
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
                }
            }

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
                    { loaded_costs_view(data, budget, budget_input) }
                },
            }
        }
    }
}

fn loaded_costs_view(
    data: CostMetricsResponse,
    mut budget: Signal<BudgetConfig>,
    mut budget_input: Signal<String>,
) -> Element {
    let today_d = compute_delta_f64(data.today_cost, data.prev_today_cost);
    let week_d = compute_delta_f64(data.week_cost, data.prev_week_cost);
    let month_d = compute_delta_f64(data.month_cost, data.prev_month_cost);

    let projected = project_month_end(
        data.month_cost,
        day_of_month_today(),
        days_in_current_month(),
    );

    let step = if data.series.len() > MAX_CHART_COLS {
        data.series.len() / MAX_CHART_COLS
    } else {
        1
    };
    let columns: Vec<TimeSeriesColumn> = data
        .series
        .iter()
        .step_by(step.max(1))
        .map(|pt| TimeSeriesColumn {
            label: pt.date.clone(),
            primary: pt.cost_usd,
            secondary: 0.0,
            primary_color: "#eab308".to_string(),
            secondary_color: "#eab308".to_string(),
        })
        .collect();

    let budget_limit = budget.read().monthly_limit_usd;
    let budget_pct = budget_progress_pct(data.month_cost, budget_limit);
    let bar_color = budget_bar_color(budget_pct).to_string();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",

            // Summary cards
            div {
                style: "display: flex; gap: 12px; flex-wrap: wrap;",
                { cost_card("Today", &format_cost(today_d.value), today_d.delta_pct, today_d.is_up) }
                { cost_card("This Week", &format_cost(week_d.value), week_d.delta_pct, week_d.is_up) }
                { cost_card("This Month", &format_cost(month_d.value), month_d.delta_pct, month_d.is_up) }

                // Projected month-end
                div {
                    style: "{CARD_STYLE}",
                    div { style: "{CARD_LABEL_STYLE}", "Projected Month-End" }
                    div { style: "{CARD_VALUE_STYLE}", "{format_cost(projected)}" }
                    div { style: "font-size: 11px; color: #706c66; margin-top: 2px; font-family: 'IBM Plex Mono', monospace;", "linear projection" }
                }
            }

            // Cost trend chart
            div {
                style: "{SECTION_STYLE}",
                div { style: "{SECTION_TITLE_STYLE}", "Cost Over Time" }
                TimeSeriesChart {
                    columns,
                    height_px: 160,
                    primary_label: "Cost (USD)".to_string(),
                    secondary_label: "".to_string(),
                }
            }

            // Budget panel
            div {
                style: "{SECTION_STYLE}",
                div { style: "{SECTION_TITLE_STYLE}", "Monthly Budget" }
                { budget_panel(
                    data.month_cost,
                    budget_limit,
                    budget_pct,
                    &bar_color,
                    budget_input.read().clone(),
                    move |v: String| budget_input.set(v),
                    move |limit: f64| budget.set(BudgetConfig { monthly_limit_usd: limit }),
                ) }
            }

            // Agent cost comparison
            if !data.agents.is_empty() {
                div {
                    style: "{SECTION_STYLE}",
                    div { style: "{SECTION_TITLE_STYLE}", "By Agent" }
                    AgentCosts { agents: data.agents }
                }
            }
        }
    }
}

fn budget_panel(
    month_cost: f64,
    budget_limit: f64,
    budget_pct: f64,
    bar_color: &str,
    input_value: String,
    mut on_input: impl FnMut(String) + 'static,
    mut on_set: impl FnMut(f64) + 'static,
) -> Element {
    let bar_color = bar_color.to_string();
    let input_for_set = input_value.clone();

    if budget_limit > 0.0 {
        rsx! {
            div {
                style: "display: flex; flex-direction: column; gap: 8px;",
                div {
                    style: "display: flex; justify-content: space-between; font-size: 12px; font-family: 'IBM Plex Mono', monospace;",
                    span { style: "color: #a8a49e;", "{format_cost(month_cost)} spent" }
                    span { style: "color: #706c66;", "of {format_cost(budget_limit)}" }
                }
                div {
                    style: "height: 8px; background: #1a1816; border-radius: 4px; overflow: hidden; border: 1px solid #2a2724;",
                    div {
                        style: "height: 100%; width: {budget_pct:.0}%; background: {bar_color}; border-radius: 4px; transition: width 0.3s ease;",
                    }
                }
                div {
                    style: "display: flex; align-items: center; gap: 8px; margin-top: 4px;",
                    input {
                        style: "padding: 4px 8px; font-size: 12px; background: #12110f; border: 1px solid #3a3530; border-radius: 4px; color: #e8e6e3; width: 100px; font-family: 'IBM Plex Mono', monospace;",
                        placeholder: "New limit $",
                        value: "{input_value}",
                        oninput: move |e| on_input(e.value()),
                    }
                    button {
                        style: "padding: 4px 10px; font-size: 12px; background: #2a2724; color: #e8e6e3; border: 1px solid #3a3530; border-radius: 4px; cursor: pointer; font-family: 'IBM Plex Mono', monospace;",
                        onclick: move |_| {
                            if let Ok(v) = input_for_set.trim().parse::<f64>() {
                                on_set(v);
                            }
                        },
                        "Set"
                    }
                }
            }
        }
    } else {
        rsx! {
            div {
                style: "display: flex; align-items: center; gap: 8px;",
                span { style: "font-size: 12px; color: #706c66; font-family: 'IBM Plex Mono', monospace;", "No budget set." }
                input {
                    style: "padding: 4px 8px; font-size: 12px; background: #12110f; border: 1px solid #3a3530; border-radius: 4px; color: #e8e6e3; width: 100px; font-family: 'IBM Plex Mono', monospace;",
                    placeholder: "Monthly limit $",
                    value: "{input_value}",
                    oninput: move |e| on_input(e.value()),
                }
                button {
                    style: "padding: 4px 10px; font-size: 12px; background: #2a2724; color: #e8e6e3; border: 1px solid #3a3530; border-radius: 4px; cursor: pointer; font-family: 'IBM Plex Mono', monospace;",
                    onclick: move |_| {
                        if let Ok(v) = input_for_set.trim().parse::<f64>() {
                            on_set(v);
                        }
                    },
                    "Set"
                }
            }
        }
    }
}

fn cost_card(label: &str, value: &str, delta_pct: f64, is_up: bool) -> Element {
    let arrow = if is_up { "↑" } else { "↓" };
    let delta_color = if is_up { "#ef4444" } else { "#22c55e" };
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
