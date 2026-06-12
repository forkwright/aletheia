//! Project detail view: header with project info and tabbed sub-views.

use dioxus::prelude::*;

use crate::app::Route;
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
#[expect(
    dead_code,
    reason = "project metadata route is pending B23 backend work"
)]
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

const TAB_DISABLED: &str = "\
    padding: var(--space-2) var(--space-4); \
    border: 1px solid transparent; \
    border-radius: var(--radius-md) 6px 0 0; \
    font-size: var(--text-sm); \
    color: var(--text-muted); \
    background: transparent; \
    opacity: 0.6; \
    cursor: not-allowed;\
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
    let project_state = use_signal(|| FetchState::NotAvailable);
    let active_tab = use_signal(|| ActiveTab::Verification);

    let tab = *active_tab.read();
    let active_tab_label = tab_label(tab);

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

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
                span { " / {active_tab_label}" }
            }

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
                        div { style: "color: var(--text-secondary); font-size: var(--text-base);", "Project metadata not available" }
                    }
                },
            }

            div {
                style: "{TAB_BAR_STYLE}",
                {render_tab(ActiveTab::Requirements, active_tab)}
                {render_tab(ActiveTab::Roadmap, active_tab)}
                {render_tab(ActiveTab::Checkpoints, active_tab)}
                {render_tab(ActiveTab::Verification, active_tab)}
                {render_tab(ActiveTab::Discussion, active_tab)}
                {render_tab(ActiveTab::Execution, active_tab)}
            }

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

#[must_use]
fn tab_label(tab: ActiveTab) -> &'static str {
    match tab {
        ActiveTab::Requirements => "Requirements",
        ActiveTab::Roadmap => "Roadmap",
        ActiveTab::Checkpoints => "Checkpoints",
        ActiveTab::Verification => "Verification",
        ActiveTab::Discussion => "Discussion",
        ActiveTab::Execution => "Execution",
    }
}

/// Only the verification API is currently wired on pylon.
#[must_use]
fn is_tab_supported(tab: ActiveTab) -> bool {
    matches!(tab, ActiveTab::Verification)
}

/// Render a tab button. Supported tabs are clickable; placeholder tabs are disabled.
fn render_tab(tab: ActiveTab, mut active_tab: Signal<ActiveTab>) -> Element {
    let label = tab_label(tab);
    let is_active = *active_tab.read() == tab;

    if is_tab_supported(tab) {
        rsx! {
            button {
                style: if is_active { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                onclick: move |_| active_tab.set(tab),
                "{label}"
            }
        }
    } else {
        rsx! {
            button {
                style: "{TAB_DISABLED}",
                disabled: true,
                "{label}"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActiveTab, is_tab_supported, tab_label};

    #[test]
    fn verification_tab_is_supported() {
        assert!(is_tab_supported(ActiveTab::Verification));
    }

    #[test]
    fn non_verification_tabs_are_disabled() {
        assert!(!is_tab_supported(ActiveTab::Requirements));
        assert!(!is_tab_supported(ActiveTab::Roadmap));
        assert!(!is_tab_supported(ActiveTab::Checkpoints));
        assert!(!is_tab_supported(ActiveTab::Discussion));
        assert!(!is_tab_supported(ActiveTab::Execution));
    }

    #[test]
    fn tab_labels_are_distinct() {
        let tabs = [
            ActiveTab::Requirements,
            ActiveTab::Roadmap,
            ActiveTab::Checkpoints,
            ActiveTab::Verification,
            ActiveTab::Discussion,
            ActiveTab::Execution,
        ];
        let labels: std::collections::HashSet<_> = tabs.iter().map(|t| tab_label(*t)).collect();
        assert_eq!(labels.len(), tabs.len(), "all tab labels must be distinct");
    }
}
