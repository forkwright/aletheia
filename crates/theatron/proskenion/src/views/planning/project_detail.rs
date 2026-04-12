//! Project detail view: header with project info and tabbed sub-views.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::app::Route;
use crate::state::connection::ConnectionConfig;
use crate::state::planning::{Project, status_badge_style, status_label};
use crate::views::planning::checkpoints::CheckpointsView;
use crate::views::planning::discussion::DiscussionView;
use crate::views::planning::execution::ExecutionView;
use crate::views::planning::requirements::RequirementsView;
use crate::views::planning::roadmap::RoadmapView;
use crate::views::planning::verification::VerificationView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Requirements,
    Roadmap,
    Checkpoints,
    Verification,
    Discussion,
    Execution,
}

#[derive(Debug, Clone)]
enum FetchState {
    Loading,
    Loaded(Project),
    NotAvailable,
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow: hidden;\
";

const BREADCRUMB_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    padding: var(--space-2) var(--space-4) 0;\
";

const BREADCRUMB_LINK: &str = "\
    color: var(--accent); \
    text-decoration: none; \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const HEADER_STYLE: &str = "\
    padding: var(--space-2) var(--space-4) var(--space-3); \
    border-bottom: 1px solid var(--border);\
";

const TAB_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-1); \
    padding: var(--space-2) var(--space-4) 0; \
    border-bottom: 1px solid var(--border); \
    background: var(--bg-surface-dim);\
";

const TAB_ACTIVE: &str = "\
    padding: var(--space-2) var(--space-4); \
    border: 1px solid var(--border); \
    border-bottom: 1px solid var(--bg-surface-dim); \
    border-radius: var(--radius-md) 6px 0 0; \
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary); \
    background: var(--bg-surface-dim); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TAB_INACTIVE: &str = "\
    padding: var(--space-2) var(--space-4); \
    border: 1px solid transparent; \
    border-radius: var(--radius-md) 6px 0 0; \
    font-size: var(--text-sm); \
    color: var(--text-muted); \
    background: transparent; \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TAB_CONTENT_STYLE: &str = "\
    flex: 1; \
    overflow: hidden;\
";

const BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-sm); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold);\
";

/// Route component for `/planning/:project_id`.
///
/// Fetches project info, renders header with breadcrumb and status,
/// then delegates to tabbed sub-views.
#[component]
pub(crate) fn PlanningProject(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut project_state = use_signal(|| FetchState::Loading);
    let mut active_tab = use_signal(|| ActiveTab::Requirements);

    let project_id_effect = project_id.clone();

    use_effect(move || {
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        project_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<Project>().await {
                    Ok(project) => project_state.set(FetchState::Loaded(project)),
                    Err(e) => {
                        project_state.set(FetchState::Error(format!("parse error: {e}")));
                    }
                },
                Ok(resp) if resp.status().as_u16() == 404 => {
                    project_state.set(FetchState::NotAvailable);
                }
                Ok(resp) => {
                    let status = resp.status();
                    project_state.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    project_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    let tab = *active_tab.read();
    let tab_label = match tab {
        ActiveTab::Requirements => "Requirements",
        ActiveTab::Roadmap => "Roadmap",
        ActiveTab::Checkpoints => "Checkpoints",
        ActiveTab::Verification => "Verification",
        ActiveTab::Discussion => "Discussion",
        ActiveTab::Execution => "Execution",
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            // Breadcrumb
            div {
                style: "{BREADCRUMB_STYLE}",
                Link {
                    to: Route::Planning {},
                    style: "{BREADCRUMB_LINK}",
                    "Planning"
                }
                span { " / " }
                match &*project_state.read() {
                    FetchState::Loaded(p) => rsx! {
                        span { style: "color: var(--text-secondary);", "{p.name}" }
                    },
                    _ => rsx! {
                        span { style: "color: var(--text-secondary);", "{project_id}" }
                    },
                }
                span { " / {tab_label}" }
            }

            // Header
            match &*project_state.read() {
                FetchState::Loaded(project) => {
                    let (badge_bg, badge_fg) = status_badge_style(project.status);
                    let label = status_label(project.status);

                    rsx! {
                        div {
                            style: "{HEADER_STYLE}",
                            div {
                                style: "display: flex; align-items: center; gap: var(--space-3);",
                                h2 { style: "margin: 0; font-size: var(--text-xl); color: var(--text-primary);", "{project.name}" }
                                span {
                                    style: "{BADGE_STYLE} background: {badge_bg}; color: {badge_fg};",
                                    "{label}"
                                }
                            }
                            if !project.description.is_empty() {
                                div {
                                    style: "font-size: var(--text-sm); color: var(--text-secondary); margin-top: var(--space-1);",
                                    "{project.description}"
                                }
                            }
                        }
                    }
                },
                FetchState::Loading => rsx! {
                    div {
                        style: "{HEADER_STYLE}",
                        div { style: "color: var(--text-secondary); font-size: var(--text-base);", "Loading project..." }
                    }
                },
                FetchState::Error(err) => rsx! {
                    div {
                        style: "{HEADER_STYLE}",
                        div { style: "color: var(--status-error); font-size: var(--text-base);", "Error: {err}" }
                    }
                },
                FetchState::NotAvailable => rsx! {
                    div {
                        style: "{HEADER_STYLE}",
                        div { style: "color: var(--text-secondary); font-size: var(--text-base);", "Project not found" }
                    }
                },
            }

            // Tab bar
            div {
                style: "{TAB_BAR_STYLE}",
                button {
                    style: if tab == ActiveTab::Requirements { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Requirements),
                    "Requirements"
                }
                button {
                    style: if tab == ActiveTab::Roadmap { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Roadmap),
                    "Roadmap"
                }
                button {
                    style: if tab == ActiveTab::Checkpoints { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Checkpoints),
                    "Checkpoints"
                }
                button {
                    style: if tab == ActiveTab::Verification { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Verification),
                    "Verification"
                }
                button {
                    style: if tab == ActiveTab::Discussion { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Discussion),
                    "Discussion"
                }
                button {
                    style: if tab == ActiveTab::Execution { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Execution),
                    "Execution"
                }
            }

            // Tab content
            div {
                style: "{TAB_CONTENT_STYLE}",
                match tab {
                    ActiveTab::Requirements => rsx! {
                        RequirementsView { project_id: project_id.clone() }
                    },
                    ActiveTab::Roadmap => rsx! {
                        RoadmapView { project_id: project_id.clone() }
                    },
                    ActiveTab::Checkpoints => rsx! {
                        CheckpointsView { project_id: project_id.clone() }
                    },
                    ActiveTab::Verification => rsx! {
                        VerificationView { project_id: project_id.clone() }
                    },
                    ActiveTab::Discussion => rsx! {
                        DiscussionView { project_id: project_id.clone() }
                    },
                    ActiveTab::Execution => rsx! {
                        ExecutionView { project_id: project_id.clone() }
                    },
                }
            }
        }
    }
}
