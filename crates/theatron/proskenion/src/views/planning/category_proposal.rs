//! Category proposal card: accept/reject an agent's proposed category change.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::planning::{CategoryProposal, ProposalAction, ProposalActionRequest};

const CARD_STYLE: &str = "\
    background: #1e1e3a; \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-md); \
    padding: var(--space-3) var(--space-4); \
    margin-bottom: var(--space-2);\
";

const CARD_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: var(--space-2);\
";

const BADGE: &str = "\
    display: inline-block; \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-sm); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold);\
";

const ACCEPT_BTN: &str = "\
    background: #1a3a1a; \
    color: var(--status-success); \
    border: 1px solid var(--status-success); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const REJECT_BTN: &str = "\
    background: #3a1a1a; \
    color: var(--status-error); \
    border: 1px solid var(--status-error); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

/// Inline proposal card displayed when an agent proposes a category change.
///
/// Sends action to `POST /api/planning/projects/{project_id}/proposals/{proposal_id}`.
#[component]
pub(crate) fn CategoryProposalCard(
    proposal: CategoryProposal,
    project_id: String,
    on_action_complete: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let submitting = use_signal(|| false);

    let current_label = proposal.current_category.label();
    let proposed_label = proposal.proposed_category.label();
    let is_submitting = *submitting.read();

    let prop_id_accept = proposal.id.clone();
    let prop_id_reject = proposal.id.clone();
    let pid_accept = project_id.clone();
    let pid_reject = project_id.clone();

    rsx! {
        div {
            style: "{CARD_STYLE}",

            div {
                style: "{CARD_HEADER}",
                div {
                    style: "font-size: var(--text-sm); color: var(--text-primary); font-weight: var(--weight-semibold);",
                    "Category Change Proposal"
                }
                div {
                    style: "font-size: var(--text-xs); color: var(--accent);",
                    "by {proposal.agent_name}"
                }
            }

            div {
                style: "font-size: var(--text-sm); color: var(--text-primary); margin-bottom: var(--space-2);",
                "{proposal.requirement_title}"
            }

            div {
                style: "display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2);",
                span {
                    style: "{BADGE} background: var(--border); color: var(--text-secondary);",
                    "{current_label}"
                }
                span { style: "color: var(--text-muted);", "->" }
                span {
                    style: "{BADGE} background: var(--bg-surface-dim); color: var(--accent);",
                    "{proposed_label}"
                }
            }

            div {
                style: "font-size: var(--text-xs); color: var(--text-secondary); margin-bottom: var(--space-3); font-style: italic;",
                "\"{proposal.rationale}\""
            }

            div {
                style: "display: flex; gap: var(--space-2);",
                button {
                    style: "{ACCEPT_BTN}",
                    disabled: is_submitting,
                    onclick: move |_| {
                        send_proposal_action(
                            config, &pid_accept, &prop_id_accept,
                            ProposalAction::Accept, submitting, on_action_complete,
                        );
                    },
                    "Accept"
                }
                button {
                    style: "{REJECT_BTN}",
                    disabled: is_submitting,
                    onclick: move |_| {
                        send_proposal_action(
                            config, &pid_reject, &prop_id_reject,
                            ProposalAction::Reject, submitting, on_action_complete,
                        );
                    },
                    "Reject"
                }
            }
        }
    }
}

fn send_proposal_action(
    config: Signal<ConnectionConfig>,
    project_id: &str,
    proposal_id: &str,
    action: ProposalAction,
    mut submitting: Signal<bool>,
    on_complete: EventHandler<()>,
) {
    let cfg = config.read().clone();
    let pid = project_id.to_string();
    let prop_id = proposal_id.to_string();
    submitting.set(true);

    spawn(async move {
        let client = authenticated_client(&cfg);
        let url = format!(
            "{}/api/planning/projects/{pid}/proposals/{prop_id}",
            cfg.server_url.trim_end_matches('/')
        );

        let body = ProposalActionRequest { action };

        match client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                submitting.set(false);
                on_complete.call(());
            }
            Ok(resp) => {
                tracing::warn!("proposal action returned {}", resp.status());
                submitting.set(false);
            }
            Err(e) => {
                tracing::warn!("proposal action error: {e}");
                submitting.set(false);
            }
        }
    });
}
