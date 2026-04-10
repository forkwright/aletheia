//! Requirements table: v1/v2/out-of-scope categorization with inline editing.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::planning::{
    CategoryProposal, Requirement, RequirementCategory, RequirementPriority, RequirementStatus,
    RequirementStore, RequirementUpdateRequest,
};
use crate::views::planning::category_proposal::CategoryProposalCard;

#[derive(Debug, Clone)]
enum FetchState {
    Loading,
    Loaded(RequirementStore),
    NotAvailable,
    Error(String),
}

/// Which field is being edited: `(requirement_id, field_name)`.
type EditingField = Option<(String, &'static str)>;

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: 16px;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 12px;\
";

const FILTER_BAR: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    margin-bottom: 12px; \
    flex-wrap: wrap;\
";

const SEARCH_INPUT: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 6px 10px; \
    color: #e0e0e0; \
    font-size: 13px; \
    min-width: 200px;\
";

const FILTER_SELECT: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 6px 8px; \
    color: #e0e0e0; \
    font-size: 12px;\
";

const TAB_BAR: &str = "\
    display: flex; \
    gap: 4px; \
    margin-bottom: 12px; \
    border-bottom: 1px solid #2a2a3a; \
    padding-bottom: 0;\
";

const TAB_ACTIVE: &str = "\
    padding: 6px 16px; \
    border: 1px solid #2a2a3a; \
    border-bottom: 1px solid #0f0f1a; \
    border-radius: 6px 6px 0 0; \
    font-size: 13px; \
    font-weight: 600; \
    color: #e0e0e0; \
    background: #0f0f1a; \
    cursor: pointer;\
";

const TAB_INACTIVE: &str = "\
    padding: 6px 16px; \
    border: 1px solid transparent; \
    border-radius: 6px 6px 0 0; \
    font-size: 13px; \
    color: #666; \
    background: transparent; \
    cursor: pointer;\
";

const TABLE_STYLE: &str = "\
    width: 100%; \
    border-collapse: collapse; \
    font-size: 13px;\
";

const TH_STYLE: &str = "\
    text-align: left; \
    padding: 6px 10px; \
    font-size: 11px; \
    font-weight: 600; \
    color: #666; \
    text-transform: uppercase; \
    letter-spacing: 0.4px; \
    border-bottom: 1px solid #2a2a3a;\
";

const TD_STYLE: &str = "\
    padding: 8px 10px; \
    border-bottom: 1px solid #1a1a2a; \
    vertical-align: top;\
";

const EDIT_INPUT: &str = "\
    background: #0f0f1a; \
    border: 1px solid #4a9aff; \
    border-radius: 4px; \
    padding: 4px 6px; \
    color: #e0e0e0; \
    font-size: 13px; \
    width: 100%;\
";

const CATEGORY_SELECT: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 2px 6px; \
    color: #e0e0e0; \
    font-size: 12px;\
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
    gap: 10px; \
    color: #555;\
";

/// Requirements table view for a planning project.
///
/// Fetches from `GET /api/planning/projects/{project_id}/requirements`.
/// Provides category tabs, inline editing, search, and filter controls.
#[component]
pub(crate) fn RequirementsView(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::Loading);
    let mut fetch_trigger = use_signal(|| 0u32);
    let mut active_category = use_signal(|| RequirementCategory::V1);
    let mut search_query = use_signal(String::new);
    let mut status_filter = use_signal(|| None::<RequirementStatus>);
    let mut priority_filter = use_signal(|| None::<RequirementPriority>);
    let mut editing: Signal<EditingField> = use_signal(|| None);
    let mut edit_value = use_signal(String::new);

    let project_id_effect = project_id.clone();

    use_effect(move || {
        let _ = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');

            // Fetch requirements and proposals in parallel.
            let req_url = format!("{base}/api/planning/projects/{pid}/requirements");
            let prop_url = format!("{base}/api/planning/projects/{pid}/proposals");

            let (reqs_result, props_result) =
                tokio::join!(client.get(&req_url).send(), client.get(&prop_url).send(),);

            let requirements = match reqs_result {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Vec<Requirement>>().await {
                        Ok(r) => r,
                        Err(e) => {
                            fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                            return;
                        }
                    }
                }
                Ok(resp) if resp.status().as_u16() == 404 => {
                    fetch_state.set(FetchState::NotAvailable);
                    return;
                }
                Ok(resp) => {
                    fetch_state.set(FetchState::Error(format!(
                        "server returned {}",
                        resp.status()
                    )));
                    return;
                }
                Err(e) => {
                    fetch_state.set(FetchState::Error(format!("connection error: {e}")));
                    return;
                }
            };

            // WHY: Proposals endpoint may not exist yet; treat 404 as empty.
            let proposals = match props_result {
                Ok(resp) if resp.status().is_success() => resp
                    .json::<Vec<CategoryProposal>>()
                    .await
                    .unwrap_or_default(),
                _ => Vec::new(),
            };

            fetch_state.set(FetchState::Loaded(RequirementStore {
                requirements,
                proposals,
            }));
        });
    });

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            div {
                style: "{HEADER_ROW}",
                h3 { style: "margin: 0; font-size: 16px; color: #e0e0e0;", "Requirements" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| {
                        let next = *fetch_trigger.peek() + 1;
                        fetch_trigger.set(next);
                    },
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading requirements..."
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
                        div { style: "font-size: 16px;", "Requirements not available" }
                        div { style: "font-size: 13px; max-width: 360px; text-align: center;",
                            "The requirements API is not available on this pylon instance."
                        }
                    }
                },
                FetchState::Loaded(store) => {
                    let pending_proposals: Vec<CategoryProposal> = store
                        .pending_proposals()
                        .into_iter()
                        .cloned()
                        .collect();
                    let v1_count = store.by_category(RequirementCategory::V1).len();
                    let v2_count = store.by_category(RequirementCategory::V2).len();
                    let oos_count = store.by_category(RequirementCategory::OutOfScope).len();

                    // Apply filters.
                    let cat = *active_category.read();
                    let query = search_query.read().clone();
                    let s_filter = *status_filter.read();
                    let p_filter = *priority_filter.read();

                    let mut filtered: Vec<Requirement> = store
                        .search(&query)
                        .into_iter()
                        .filter(|r| r.category == cat)
                        .filter(|r| s_filter.is_none_or(|s| r.status == s))
                        .filter(|r| p_filter.is_none_or(|p| r.priority == p))
                        .cloned()
                        .collect();
                    filtered.sort_by_key(|r| r.priority);

                    rsx! {
                        // Pending proposals
                        if !pending_proposals.is_empty() {
                            div {
                                style: "margin-bottom: 12px;",
                                for proposal in &pending_proposals {
                                    CategoryProposalCard {
                                        key: "{proposal.id}",
                                        proposal: proposal.clone(),
                                        project_id: project_id.clone(),
                                        on_action_complete: move |_| {
                                            let next = *fetch_trigger.peek() + 1;
                                            fetch_trigger.set(next);
                                        },
                                    }
                                }
                            }
                        }

                        // Category tabs
                        div {
                            style: "{TAB_BAR}",
                            button {
                                style: if cat == RequirementCategory::V1 { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                                onclick: move |_| active_category.set(RequirementCategory::V1),
                                "v1 ({v1_count})"
                            }
                            button {
                                style: if cat == RequirementCategory::V2 { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                                onclick: move |_| active_category.set(RequirementCategory::V2),
                                "v2 ({v2_count})"
                            }
                            button {
                                style: if cat == RequirementCategory::OutOfScope { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                                onclick: move |_| active_category.set(RequirementCategory::OutOfScope),
                                "Out of Scope ({oos_count})"
                            }
                        }

                        // Filter bar
                        div {
                            style: "{FILTER_BAR}",
                            input {
                                style: "{SEARCH_INPUT}",
                                r#type: "text",
                                placeholder: "Search requirements...",
                                value: "{search_query}",
                                oninput: move |evt| search_query.set(evt.value()),
                            }
                            select {
                                style: "{FILTER_SELECT}",
                                onchange: move |evt| {
                                    let val = evt.value();
                                    status_filter.set(match val.as_str() {
                                        "proposed" => Some(RequirementStatus::Proposed),
                                        "accepted" => Some(RequirementStatus::Accepted),
                                        "implemented" => Some(RequirementStatus::Implemented),
                                        "verified" => Some(RequirementStatus::Verified),
                                        _ => None,
                                    });
                                },
                                option { value: "", "All Statuses" }
                                option { value: "proposed", "Proposed" }
                                option { value: "accepted", "Accepted" }
                                option { value: "implemented", "Implemented" }
                                option { value: "verified", "Verified" }
                            }
                            select {
                                style: "{FILTER_SELECT}",
                                onchange: move |evt| {
                                    let val = evt.value();
                                    priority_filter.set(match val.as_str() {
                                        "P0" => Some(RequirementPriority::P0),
                                        "P1" => Some(RequirementPriority::P1),
                                        "P2" => Some(RequirementPriority::P2),
                                        _ => None,
                                    });
                                },
                                option { value: "", "All Priorities" }
                                option { value: "P0", "P0" }
                                option { value: "P1", "P1" }
                                option { value: "P2", "P2" }
                            }
                        }

                        // Requirements table
                        div {
                            style: "flex: 1; overflow-y: auto;",
                            if filtered.is_empty() {
                                div {
                                    style: "{PLACEHOLDER_STYLE}",
                                    div { style: "font-size: 14px;", "No matching requirements" }
                                }
                            } else {
                                table {
                                    style: "{TABLE_STYLE}",
                                    thead {
                                        tr {
                                            th { style: "{TH_STYLE}", "Title" }
                                            th { style: "{TH_STYLE}", "Description" }
                                            th { style: "{TH_STYLE}", "Status" }
                                            th { style: "{TH_STYLE}", "Priority" }
                                            th { style: "{TH_STYLE}", "Agent" }
                                            th { style: "{TH_STYLE}", "Category" }
                                        }
                                    }
                                    tbody {
                                        for req in &filtered {
                                            {
                                                let editing_snapshot = editing.read().clone();
                                                let is_editing_title = editing_snapshot
                                                    .as_ref()
                                                    .is_some_and(|(id, f)| id == &req.id && *f == "title");
                                                let is_editing_desc = editing_snapshot
                                                    .as_ref()
                                                    .is_some_and(|(id, f)| id == &req.id && *f == "description");
                                                let req_id = req.id.clone();
                                                let req_id2 = req.id.clone();
                                                let req_id3 = req.id.clone();
                                                let req_id4 = req.id.clone();
                                                let req_title = req.title.clone();
                                                let req_desc = req.description.clone();
                                                let pid1 = project_id.clone();
                                                let pid2 = project_id.clone();
                                                let pid3 = project_id.clone();
                                                let pid4 = project_id.clone();
                                                let pid5 = project_id.clone();
                                                let priority_color = req.priority.color();
                                                let status_color = req.status.color();
                                                let status_label = req.status.label();
                                                let priority_label = req.priority.label();
                                                let agent = req.assigned_agent.as_deref().unwrap_or("—");
                                                let desc_display = if req.description.is_empty() {
                                                    "—".to_string()
                                                } else {
                                                    req.description.clone()
                                                };
                                                let cat_value = match req.category {
                                                    RequirementCategory::V1 => "v1",
                                                    RequirementCategory::V2 => "v2",
                                                    RequirementCategory::OutOfScope => "out_of_scope",
                                                };

                                                rsx! {
                                                    tr {
                                                        key: "{req.id}",

                                                        // Title (click to edit)
                                                        td {
                                                            style: "{TD_STYLE} color: #e0e0e0;",
                                                            if is_editing_title {
                                                                input {
                                                                    style: "{EDIT_INPUT}",
                                                                    r#type: "text",
                                                                    value: "{edit_value}",
                                                                    oninput: move |evt| edit_value.set(evt.value()),
                                                                    onkeydown: move |evt| {
                                                                        if evt.key() == Key::Enter {
                                                                            send_edit(config, &pid1, &req_id, "title", &edit_value.read(), editing, fetch_trigger);
                                                                        } else if evt.key() == Key::Escape {
                                                                            editing.set(None);
                                                                        }
                                                                    },
                                                                    onfocusout: move |_| {
                                                                        send_edit(config, &pid2, &req_id2, "title", &edit_value.read(), editing, fetch_trigger);
                                                                    },
                                                                }
                                                            } else {
                                                                span {
                                                                    style: "cursor: text;",
                                                                    onclick: move |_| {
                                                                        edit_value.set(req_title.clone());
                                                                        editing.set(Some((req_id3.clone(), "title")));
                                                                    },
                                                                    "{req.title}"
                                                                }
                                                            }
                                                        }

                                                        // Description (click to edit)
                                                        td {
                                                            style: "{TD_STYLE} color: #aaa;",
                                                            if is_editing_desc {
                                                                input {
                                                                    style: "{EDIT_INPUT}",
                                                                    r#type: "text",
                                                                    value: "{edit_value}",
                                                                    oninput: move |evt| edit_value.set(evt.value()),
                                                                    onkeydown: {
                                                                        let rid = req_id4.clone();
                                                                        move |evt| {
                                                                            if evt.key() == Key::Enter {
                                                                                send_edit(config, &pid3, &rid, "description", &edit_value.read(), editing, fetch_trigger);
                                                                            } else if evt.key() == Key::Escape {
                                                                                editing.set(None);
                                                                            }
                                                                        }
                                                                    },
                                                                    onfocusout: {
                                                                        let rid = req_id4.clone();
                                                                        move |_| {
                                                                            send_edit(config, &pid4, &rid, "description", &edit_value.read(), editing, fetch_trigger);
                                                                        }
                                                                    },
                                                                }
                                                            } else {
                                                                span {
                                                                    style: "cursor: text;",
                                                                    onclick: {
                                                                        let rid = req_id4.clone();
                                                                        move |_| {
                                                                            edit_value.set(req_desc.clone());
                                                                            editing.set(Some((rid.clone(), "description")));
                                                                        }
                                                                    },
                                                                    "{desc_display}"
                                                                }
                                                            }
                                                        }

                                                        td {
                                                            style: "{TD_STYLE} color: {status_color}; font-weight: 600;",
                                                            "{status_label}"
                                                        }
                                                        td {
                                                            style: "{TD_STYLE}",
                                                            span {
                                                                style: "color: {priority_color}; font-weight: 600;",
                                                                "{priority_label}"
                                                            }
                                                        }
                                                        td { style: "{TD_STYLE} color: #888;", "{agent}" }

                                                        // Category dropdown
                                                        td {
                                                            style: "{TD_STYLE}",
                                                            select {
                                                                style: "{CATEGORY_SELECT}",
                                                                value: "{cat_value}",
                                                                onchange: {
                                                                    let rid = req.id.clone();
                                                                    move |evt| {
                                                                        let new_cat = match evt.value().as_str() {
                                                                            "v1" => RequirementCategory::V1,
                                                                            "v2" => RequirementCategory::V2,
                                                                            _ => RequirementCategory::OutOfScope,
                                                                        };
                                                                        send_category_change(config, &pid5, &rid, new_cat, fetch_trigger);
                                                                    }
                                                                },
                                                                option { value: "v1", "v1" }
                                                                option { value: "v2", "v2" }
                                                                option { value: "out_of_scope", "Out of Scope" }
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
            }
        }
    }
}

/// Send a requirement field edit to the API.
fn send_edit(
    config: Signal<ConnectionConfig>,
    project_id: &str,
    req_id: &str,
    field: &'static str,
    value: &str,
    mut editing: Signal<EditingField>,
    mut fetch_trigger: Signal<u32>,
) {
    let cfg = config.read().clone();
    let pid = project_id.to_string();
    let rid = req_id.to_string();

    let update = match field {
        "title" => RequirementUpdateRequest {
            title: Some(value.to_string()),
            description: None,
            category: None,
        },
        "description" => RequirementUpdateRequest {
            title: None,
            description: Some(value.to_string()),
            category: None,
        },
        _ => return,
    };

    spawn(async move {
        let client = authenticated_client(&cfg);
        let url = format!(
            "{}/api/planning/projects/{pid}/requirements/{rid}",
            cfg.server_url.trim_end_matches('/')
        );

        match client.put(&url).json(&update).send().await {
            Ok(resp) if resp.status().is_success() => {
                editing.set(None);
                let next = *fetch_trigger.peek() + 1;
                fetch_trigger.set(next);
            }
            Ok(resp) => {
                tracing::warn!("requirement update returned {}", resp.status());
                editing.set(None);
            }
            Err(e) => {
                tracing::warn!("requirement update error: {e}");
                editing.set(None);
            }
        }
    });
}

/// Send a requirement category change to the API.
fn send_category_change(
    config: Signal<ConnectionConfig>,
    project_id: &str,
    req_id: &str,
    new_category: RequirementCategory,
    mut fetch_trigger: Signal<u32>,
) {
    let cfg = config.read().clone();
    let pid = project_id.to_string();
    let rid = req_id.to_string();

    let update = RequirementUpdateRequest {
        title: None,
        description: None,
        category: Some(new_category),
    };

    spawn(async move {
        let client = authenticated_client(&cfg);
        let url = format!(
            "{}/api/planning/projects/{pid}/requirements/{rid}",
            cfg.server_url.trim_end_matches('/')
        );

        if let Ok(resp) = client.put(&url).json(&update).send().await
            && resp.status().is_success()
        {
            let next = *fetch_trigger.peek() + 1;
            fetch_trigger.set(next);
        }
    });
}
