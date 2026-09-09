//! Requirements table: read-only v1/v2/out-of-scope categorization.

use dioxus::prelude::*;

use crate::state::planning::{
    Requirement, RequirementCategory, RequirementPriority, RequirementStatus, RequirementStore,
};

// WHY(#7224): requirement edits and category-proposal review used to POST to
// pylon via `project_requirement_url`/`project_proposal_url`, but neither
// route has ever had a pylon handler and dianoia has no persisted, mutable
// requirement/proposal entity to back one -- see the same-numbered note on
// `skene::api::routes::planning`. The edit actions and `CategoryProposalCard`
// were deleted; this view is now a read-only requirements table.

#[derive(Debug, Clone)]
#[expect(dead_code, reason = "requirements routes are pending B23 backend work")]
enum FetchState {
    Loading,
    Loaded(RequirementStore),
    NotAvailable,
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: var(--space-4);\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: var(--space-3);\
";

const FILTER_BAR: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-3); \
    flex-wrap: wrap;\
";

const SEARCH_INPUT: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    color: var(--text-primary); \
    font-size: var(--text-sm); \
    min-width: 200px;\
";

const FILTER_SELECT: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-2); \
    color: var(--text-primary); \
    font-size: var(--text-xs);\
";

const TAB_BAR: &str = "\
    display: flex; \
    gap: var(--space-1); \
    margin-bottom: var(--space-3); \
    border-bottom: 1px solid var(--border); \
    padding-bottom: 0;\
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

const TABLE_STYLE: &str = "\
    width: 100%; \
    border-collapse: collapse; \
    font-size: var(--text-sm);\
";

const TH_STYLE: &str = "\
    text-align: left; \
    padding: var(--space-2) var(--space-3); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    color: var(--text-muted); \
    text-transform: uppercase; \
    letter-spacing: 0.4px; \
    border-bottom: 1px solid var(--border);\
";

const TD_STYLE: &str = "\
    padding: var(--space-2) 10px; \
    border-bottom: 1px solid var(--border-separator); \
    vertical-align: top;\
";

const REFRESH_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
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

/// Requirements table view for a planning project.
///
/// Shows requirements when the pylon requirements API exists.
/// Provides category tabs, search, and filter controls. Read-only: editing
/// a requirement's title, description, or category has no pylon backend
/// (see the `WHY(#7224)` note above).
#[component]
pub(crate) fn RequirementsView(project_id: String) -> Element {
    let mut fetch_state = use_signal(|| FetchState::NotAvailable);
    let mut active_category = use_signal(|| RequirementCategory::V1);
    let mut search_query = use_signal(String::new);
    let mut status_filter = use_signal(|| None::<RequirementStatus>);
    let mut priority_filter = use_signal(|| None::<RequirementPriority>);

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            role: "region",
            "aria-label": "Requirements",

            div {
                style: "{HEADER_ROW}",
                h3 { style: "margin: 0; font-size: var(--text-md); color: var(--text-primary);", "Requirements" }
                button {
                    style: "{REFRESH_BTN}",
                    "aria-label": "Refresh requirements",
                    onclick: move |_| {
                        fetch_state.set(FetchState::NotAvailable);
                    },
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                        "Loading requirements..."
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
                        div { style: "font-size: var(--text-md);", "Requirements not available" }
                        div { style: "font-size: var(--text-sm); max-width: 360px; text-align: center;",
                            "The requirements API is not available on this pylon instance."
                        }
                    }
                },
                FetchState::Loaded(store) => {
                    let v1_count = store.by_category(RequirementCategory::V1).len();
                    let v2_count = store.by_category(RequirementCategory::V2).len();
                    let oos_count = store.by_category(RequirementCategory::OutOfScope).len();

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
                        div {
                            style: "{TAB_BAR}",
                            role: "tablist",
                            "aria-label": "Requirement categories",
                            button {
                                style: if cat == RequirementCategory::V1 { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                                role: "tab",
                                "aria-selected": if cat == RequirementCategory::V1 { "true" } else { "false" },
                                onclick: move |_| active_category.set(RequirementCategory::V1),
                                "v1 ({v1_count})"
                            }
                            button {
                                style: if cat == RequirementCategory::V2 { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                                role: "tab",
                                "aria-selected": if cat == RequirementCategory::V2 { "true" } else { "false" },
                                onclick: move |_| active_category.set(RequirementCategory::V2),
                                "v2 ({v2_count})"
                            }
                            button {
                                style: if cat == RequirementCategory::OutOfScope { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                                role: "tab",
                                "aria-selected": if cat == RequirementCategory::OutOfScope { "true" } else { "false" },
                                onclick: move |_| active_category.set(RequirementCategory::OutOfScope),
                                "Out of Scope ({oos_count})"
                            }
                        }

                        div {
                            style: "{FILTER_BAR}",
                            input {
                                style: "{SEARCH_INPUT}",
                                r#type: "text",
                                placeholder: "Search requirements...",
                                value: "{search_query}",
                                "aria-label": "Search requirements",
                                oninput: move |evt| search_query.set(evt.value()),
                            }
                            select {
                                style: "{FILTER_SELECT}",
                                "aria-label": "Filter by status",
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
                                "aria-label": "Filter by priority",
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

                        div {
                            style: "flex: 1; overflow-y: auto;",
                            if filtered.is_empty() {
                                div {
                                    style: "{PLACEHOLDER_STYLE}",
                                    div { style: "font-size: var(--text-base);", "No matching requirements" }
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
                                                let category_label = req.category.label();

                                                rsx! {
                                                    tr {
                                                        key: "{req.id}",
                                                        td { style: "{TD_STYLE} color: var(--text-primary);", "{req.title}" }
                                                        td { style: "{TD_STYLE} color: var(--text-secondary);", "{desc_display}" }
                                                        td {
                                                            style: "{TD_STYLE} color: {status_color}; font-weight: var(--weight-semibold);",
                                                            "{status_label}"
                                                        }
                                                        td {
                                                            style: "{TD_STYLE}",
                                                            span {
                                                                style: "color: {priority_color}; font-weight: var(--weight-semibold);",
                                                                "{priority_label}"
                                                            }
                                                        }
                                                        td { style: "{TD_STYLE} color: var(--text-secondary);", "{agent}" }
                                                        td { style: "{TD_STYLE} color: var(--text-secondary);", "{category_label}" }
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
