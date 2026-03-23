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
    padding: 16px; \
    gap: 16px;\
";

const GRID_STYLE: &str = "\
    display: grid; \
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); \
    gap: 16px; \
    flex: 1; \
    overflow-y: auto; \
    align-content: start;\
";

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px; \
    cursor: pointer; \
    transition: border-color 0.15s;\
";

const CARD_TITLE: &str = "\
    font-size: 16px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin-bottom: 4px;\
";

const CARD_DESC: &str = "\
    font-size: 13px; \
    color: #888; \
    margin-bottom: 12px; \
    display: -webkit-box; \
    -webkit-line-clamp: 2; \
    -webkit-box-orient: vertical; \
    overflow: hidden;\
";

const BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: 2px 8px; \
    border-radius: 4px; \
    font-size: 11px; \
    font-weight: 600;\
";

const PROGRESS_TRACK: &str = "\
    background: #2a2a3a; \
    height: 4px; \
    border-radius: 2px; \
    overflow: hidden; \
    margin: 8px 0;\
";

const META_STYLE: &str = "\
    font-size: 11px; \
    color: #666; \
    display: flex; \
    align-items: center; \
    gap: 12px; \
    margin-top: 8px;\
";

const REFRESH_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: 12px; \
    color: #555;\
";

/// Project dashboard — the default `/planning` view.
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
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading projects..."
                    }
                },
                FetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #ef4444;",
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
        format!("width: {pct}%; height: 100%; background: #22c55e; border-radius: 2px;");

    rsx! {
        div { style: "{CARD_TITLE}", "{project.name}" }

        if !project.description.is_empty() {
            div { style: "{CARD_DESC}", "{project.description}" }
        }

        div {
            style: "display: flex; align-items: center; gap: 8px; margin-bottom: 8px;",
            span {
                style: "{BADGE_STYLE} background: {badge_bg}; color: {badge_fg};",
                "{label}"
            }
            if !phase_text.is_empty() {
                span { style: "font-size: 12px; color: #888;", "{phase_text}" }
            }
        }

        // Progress bar
        div {
            style: "display: flex; align-items: center; gap: 8px;",
            div {
                style: "{PROGRESS_TRACK} flex: 1;",
                div { style: "{progress_fill}" }
            }
            span { style: "font-size: 11px; color: #888; min-width: 32px;",
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
