//! Project detail view: tab container for planning sub-views.

use dioxus::prelude::*;

use crate::views::planning::checkpoints::CheckpointsView;
use crate::views::planning::discussion::DiscussionView;
use crate::views::planning::execution::ExecutionView;
use crate::views::planning::verification::VerificationView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Checkpoints,
    Verification,
    Discussion,
    Execution,
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow: hidden;\
";

const TAB_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 4px; \
    padding: 8px 16px 0; \
    border-bottom: 1px solid #2a2a3a; \
    background: #0f0f1a;\
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

const TAB_CONTENT_STYLE: &str = "\
    flex: 1; \
    overflow: hidden;\
";

/// Project detail view with Checkpoints, Verification, Discussion, and Execution tabs.
#[component]
pub(crate) fn ProjectDetail(project_id: String) -> Element {
    let mut active_tab = use_signal(|| ActiveTab::Checkpoints);

    let project_id_ck = project_id.clone();
    let project_id_vf = project_id.clone();
    let project_id_dc = project_id.clone();
    let project_id_ex = project_id.clone();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            // Tab bar
            div {
                style: "{TAB_BAR_STYLE}",
                button {
                    style: if *active_tab.read() == ActiveTab::Checkpoints { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Checkpoints),
                    "Checkpoints"
                }
                button {
                    style: if *active_tab.read() == ActiveTab::Verification { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Verification),
                    "Verification"
                }
                button {
                    style: if *active_tab.read() == ActiveTab::Discussion { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Discussion),
                    "Discussion"
                }
                button {
                    style: if *active_tab.read() == ActiveTab::Execution { "{TAB_ACTIVE}" } else { "{TAB_INACTIVE}" },
                    onclick: move |_| active_tab.set(ActiveTab::Execution),
                    "Execution"
                }
            }

            // Tab content
            div {
                style: "{TAB_CONTENT_STYLE}",
                match *active_tab.read() {
                    ActiveTab::Checkpoints => rsx! {
                        CheckpointsView { project_id: project_id_ck.clone() }
                    },
                    ActiveTab::Verification => rsx! {
                        VerificationView { project_id: project_id_vf.clone() }
                    },
                    ActiveTab::Discussion => rsx! {
                        DiscussionView { project_id: project_id_dc.clone() }
                    },
                    ActiveTab::Execution => rsx! {
                        ExecutionView { project_id: project_id_ex.clone() }
                    },
                }
            }
        }
    }
}
