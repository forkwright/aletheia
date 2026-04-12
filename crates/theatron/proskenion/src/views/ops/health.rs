//! Service health panel: cron jobs, daemon tasks, and failure summary.

use dioxus::prelude::*;

use crate::state::ops::{CronJobInfo, DaemonTaskInfo, ServiceHealthStore};

const PANEL_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4); \
    flex: 1; \
    overflow-y: auto; \
    min-width: 280px;\
";

const SECTION_TITLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-3);\
";

const SUBSECTION_TITLE: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    margin: var(--space-3) 0 var(--space-2) 0; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) 0; \
    border-bottom: 1px solid #222; \
    font-size: var(--text-xs);\
";

const DOT_BASE: &str = "\
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    flex-shrink: 0;\
";

const NAME_STYLE: &str = "\
    color: var(--text-primary); \
    flex: 1; \
    white-space: nowrap; \
    overflow: hidden; \
    text-overflow: ellipsis;\
";

const DETAIL_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    white-space: nowrap;\
";

const FAILURE_BOX: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    margin-bottom: var(--space-3);\
";

const FAILURE_COUNT: &str = "\
    font-size: var(--text-2xl); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

const EMPTY_STATE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    padding: var(--space-1) 0;\
";

#[component]
pub(crate) fn ServiceHealthPanel(store: Signal<ServiceHealthStore>) -> Element {
    let data = store.read();

    let trend_color = data.failure_trend.color();
    let trend_indicator = data.failure_trend.indicator();
    let trend_style = format!("color: {trend_color}; font-size: var(--text-md); margin-left: auto;");

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            div { style: "{SECTION_TITLE}", "Service Health" }

            div {
                style: "{FAILURE_BOX}",
                span { style: "{FAILURE_COUNT}", "{data.failure_count}" }
                span { style: "color: var(--text-secondary); font-size: var(--text-xs);", "failures" }
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
    let status_style = format!("color: {}; font-size: var(--text-xs);", task.status.dot_color());
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
                    style: "color: var(--status-warning); font-size: var(--text-xs);",
                    "{task.restart_count} restarts"
                }
            }
        }
    }
}
