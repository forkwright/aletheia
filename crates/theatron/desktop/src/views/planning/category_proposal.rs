//! Category proposal card: accept/reject an agent's proposed category change.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::planning::{CategoryProposal, ProposalAction, ProposalActionRequest};

const CARD_STYLE: &str = "\
    background: #1e1e3a; \
    border: 1px solid #4a4aff; \
    border-radius: 8px; \
    padding: 12px 16px; \
    margin-bottom: 8px;\
";

const CARD_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 8px;\
";

const BADGE: &str = "\
    display: inline-block; \
    padding: 2px 8px; \
    border-radius: 4px; \
    font-size: 11px; \
    font-weight: 600;\
";

const ACCEPT_BTN: &str = "\
    background: #1a3a1a; \
    color: #22c55e; \
    border: 1px solid #22c55e; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const REJECT_BTN: &str = "\
    background: #3a1a1a; \
    color: #ef4444; \
    border: 1px solid #ef4444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
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
                    style: "font-size: 13px; color: #e0e0e0; font-weight: 600;",
                    "Category Change Proposal"
                }
                div {
                    style: "font-size: 11px; color: #8080ff;",
                    "by {proposal.agent_name}"
                }
            }

            div {
                style: "font-size: 13px; color: #e0e0e0; margin-bottom: 8px;",
                "{proposal.requirement_title}"
            }

            div {
                style: "display: flex; align-items: center; gap: 8px; margin-bottom: 8px;",
                span {
                    style: "{BADGE} background: #2a2a3a; color: #888;",
                    "{current_label}"
                }
                span { style: "color: #555;", "->" }
                span {
                    style: "{BADGE} background: #1a2a3a; color: #4a9aff;",
                    "{proposed_label}"
                }
            }

            div {
                style: "font-size: 12px; color: #aaa; margin-bottom: 10px; font-style: italic;",
                "\"{proposal.rationale}\""
            }

            div {
                style: "display: flex; gap: 8px;",
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
