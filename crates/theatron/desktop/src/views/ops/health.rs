//! Service health panel: cron jobs, daemon tasks, and failure summary.

use dioxus::prelude::*;

use crate::state::ops::{CronJobInfo, DaemonTaskInfo, ServiceHealthStore};

const PANEL_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px; \
    flex: 1; \
    overflow-y: auto; \
    min-width: 280px;\
";

const SECTION_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    margin-bottom: 10px;\
";

const SUBSECTION_TITLE: &str = "\
    font-size: 12px; \
    font-weight: bold; \
    color: #888; \
    margin: 12px 0 6px 0; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 5px 0; \
    border-bottom: 1px solid #222; \
    font-size: 12px;\
";

const DOT_BASE: &str = "\
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    flex-shrink: 0;\
";

const NAME_STYLE: &str = "\
    color: #e0e0e0; \
    flex: 1; \
    white-space: nowrap; \
    overflow: hidden; \
    text-overflow: ellipsis;\
";

const DETAIL_STYLE: &str = "\
    color: #666; \
    font-size: 11px; \
    white-space: nowrap;\
";

const FAILURE_BOX: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 12px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    margin-bottom: 12px;\
";

const FAILURE_COUNT: &str = "\
    font-size: 24px; \
    font-weight: bold; \
    color: #e0e0e0;\
";

const EMPTY_STATE: &str = "\
    color: #555; \
    font-size: 12px; \
    padding: 4px 0;\
";

#[component]
pub(crate) fn ServiceHealthPanel(store: Signal<ServiceHealthStore>) -> Element {
    let data = store.read();

    let trend_color = data.failure_trend.color();
    let trend_indicator = data.failure_trend.indicator();
    let trend_style = format!("color: {trend_color}; font-size: 16px; margin-left: auto;");

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            div { style: "{SECTION_TITLE}", "Service Health" }

            div {
                style: "{FAILURE_BOX}",
                span { style: "{FAILURE_COUNT}", "{data.failure_count}" }
                span { style: "color: #888; font-size: 12px;", "failures" }
                span {
                    style: "{trend_style}",
                    title: "trend",
                    "{trend_indicator}"
                }
            }

            div { style: "{SUBSECTION_TITLE}", "Cron Jobs" }

            if data.cron_jobs.is_empty() {
                div { style: "{EMPTY_STATE}", "No cron jobs configured" }
            }

            for (i , job) in data.cron_jobs.iter().enumerate() {
                {render_cron_row(i, job)}
            }

            div { style: "{SUBSECTION_TITLE}", "Daemon Tasks" }

            if data.daemon_tasks.is_empty() {
                div { style: "{EMPTY_STATE}", "No daemon tasks registered" }
            }

            for (i , task) in data.daemon_tasks.iter().enumerate() {
                {render_daemon_row(i, task)}
            }
        }
    }
}

fn render_cron_row(i: usize, job: &CronJobInfo) -> Element {
    let dot_style = format!("{DOT_BASE} background: {};", job.last_result.dot_color());

    rsx! {
        div {
            key: "cron-{i}",
            style: "{ROW_STYLE}",
            span { style: "{dot_style}" }
            span { style: "{NAME_STYLE}", "{job.name}" }
            span { style: "{DETAIL_STYLE}", "{job.schedule}" }
            if let Some(ref last) = job.last_run {
                span { style: "{DETAIL_STYLE}", "{last}" }
            }
        }
    }
}

fn render_daemon_row(i: usize, task: &DaemonTaskInfo) -> Element {
    let dot_style = format!("{DOT_BASE} background: {};", task.status.dot_color());
    let status_style = format!("color: {}; font-size: 11px;", task.status.dot_color());
    let status_label = task.status.label();

    rsx! {
        div {
            key: "task-{i}",
            style: "{ROW_STYLE}",
            span { style: "{dot_style}" }
            span { style: "{NAME_STYLE}", "{task.name}" }
            span { style: "{status_style}", "{status_label}" }
            if let Some(ref uptime) = task.uptime {
                span { style: "{DETAIL_STYLE}", "{uptime}" }
            }
            if task.restart_count > 0 {
                span {
                    style: "color: #eab308; font-size: 11px;",
                    "{task.restart_count} restarts"
                }
            }
        }
    }
}
