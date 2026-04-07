//! Project dashboard: grid of project cards with status, progress, and navigation.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::app::Route;
use crate::state::connection::ConnectionConfig;
use crate::state::planning::{Project, ProjectStore, status_badge_style, status_label};

#[derive(Debug, Clone)]
enum FetchState {
    Loading,
    Loaded(ProjectStore),
    NotAvailable,
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: var(--space-4); \
    gap: var(--space-4);\
";

const GRID_STYLE: &str = "\
    display: grid; \
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); \
    gap: var(--space-4); \
    flex: 1; \
    overflow-y: auto; \
    align-content: start;\
";

const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    padding: var(--space-4); \
    cursor: pointer;\
";

const CARD_TITLE: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary); \
    margin-bottom: var(--space-1);\
";

const CARD_DESC: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-3); \
    display: -webkit-box; \
    -webkit-line-clamp: 2; \
    -webkit-box-orient: vertical; \
    overflow: hidden;\
";

const BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-md); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold);\
";

const PROGRESS_TRACK: &str = "\
    background: var(--bg-surface-dim); \
    height: 4px; \
    border-radius: var(--radius-sm); \
    overflow: hidden; \
    margin: var(--space-2) 0;\
";

const META_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    margin-top: var(--space-2);\
";

const REFRESH_BTN: &str = "\
    background: var(--bg-surface); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-sm); \
    cursor: pointer;\
";

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: var(--space-3); \
    color: var(--text-muted);\
";

/// Project dashboard -- the default `/planning` view.
///
/// Fetches projects from `GET /api/planning/projects` and displays a card grid.
/// Clicking a card navigates to the project detail view.
#[component]
pub(crate) fn Planning() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let nav = use_navigator();
    let mut fetch_state = use_signal(|| FetchState::Loading);

    let mut do_refresh = move || {
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<Vec<Project>>().await {
                    Ok(projects) => {
                        fetch_state.set(FetchState::Loaded(ProjectStore { projects }));
                    }
                    Err(e) => {
                        fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                    }
                },
                // WHY: 404 means planning endpoint not available on this pylon version.
                Ok(resp) if resp.status().as_u16() == 404 => {
                    fetch_state.set(FetchState::NotAvailable);
                }
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

            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h2 { style: "font-size: 20px; margin: 0;", "Planning" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| do_refresh(),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                        "Loading projects..."
                    }
                },
                FetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--status-error);",
                        "Error: {err}"
                    }
                },
                FetchState::NotAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: 48px;", "[P]" }
                        div { style: "font-size: 16px;", "Project planning not available" }
                        div { style: "font-size: 13px; max-width: 400px; text-align: center;",
                            "The planning API is not available on this pylon instance. "
                            "Projects will appear here when connected to a pylon with dianoia integration."
                        }
                    }
                },
                FetchState::Loaded(store) => {
                    if store.projects.is_empty() {
                        rsx! {
                            div {
                                style: "{PLACEHOLDER_STYLE}",
                                div { style: "font-size: 16px;", "No projects" }
                                div { style: "font-size: 13px;",
                                    "Projects will appear here when created."
                                }
                            }
                        }
                    } else {
                        rsx! {
                            div {
                                style: "{GRID_STYLE}",
                                for project in &store.projects {
                                    {
                                        let pid = project.id.clone();
                                        rsx! {
                                            div {
                                                key: "{project.id}",
                                                style: "{CARD_STYLE}",
                                                onclick: move |_| {
                                                    nav.push(Route::PlanningProject { project_id: pid.clone() });
                                                },
                                                {render_project_card(project)}
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

fn render_project_card(project: &Project) -> Element {
    let (badge_bg, badge_fg) = status_badge_style(project.status);
    let label = status_label(project.status);
    let pct = ProjectStore::progress_pct(project);

    let phase_text = project
        .current_phase
        .as_ref()
        .map(|p| format!("{} (Phase {} of {})", p.name, p.number, p.total))
        .unwrap_or_default();

    let activity_text = project.last_activity.as_deref().unwrap_or("—");

    let agents_text = if project.active_agents.is_empty() {
        "No active agents".to_string()
    } else {
        project.active_agents.join(", ")
    };

    let progress_fill =
        format!("width: {pct}%; height: 100%; background: var(--status-success); border-radius: var(--radius-sm);");

    rsx! {
        div { style: "{CARD_TITLE}", "{project.name}" }

        if !project.description.is_empty() {
            div { style: "{CARD_DESC}", "{project.description}" }
        }

        div {
            style: "display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2);",
            span {
                style: "{BADGE_STYLE} background: {badge_bg}; color: {badge_fg};",
                "{label}"
            }
            if !phase_text.is_empty() {
                span { style: "font-size: var(--text-sm); color: var(--text-secondary);", "{phase_text}" }
            }
        }

        // Progress bar
        div {
            style: "display: flex; align-items: center; gap: var(--space-2);",
            div {
                style: "{PROGRESS_TRACK} flex: 1;",
                div { style: "{progress_fill}" }
            }
            span { style: "font-size: var(--text-xs); color: var(--text-secondary); min-width: 32px;",
                "{pct}%"
            }
        }

        div {
            style: "{META_STYLE}",
            span { "Last: {activity_text}" }
            span { "{agents_text}" }
        }
    }
}
