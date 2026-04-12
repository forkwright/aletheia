//! Verification view: goal-backward requirement verification results.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::coverage_bar::{CoverageBar, coverage_color};
use crate::state::connection::ConnectionConfig;
use crate::state::verification::{RequirementVerification, VerificationResult, VerificationStore};
use crate::views::planning::gap_analysis::GapAnalysisPanel;

#[derive(Debug, Clone)]
enum FetchState {
    Loading,
    Loaded(VerificationStore),
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
    margin-bottom: var(--space-4);\
";

const SECTION_LABEL: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    color: var(--text-muted); \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin: var(--space-4) 0 var(--space-2);\
";

const COVERAGE_SECTION: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4); \
    margin-bottom: var(--space-4);\
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
    border-bottom: 1px solid #1a1a2a; \
    vertical-align: top;\
";

const GAP_SECTION: &str = "\
    margin-top: var(--space-4); \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4);\
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

const VERIFY_BTN: &str = "\
    background: #1a2a4a; \
    color: var(--accent); \
    border: 1px solid var(--accent); \
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

/// Goal-backward verification results view.
///
/// Fetches from `GET /api/planning/projects/{project_id}/verification`.
/// Displays overall and per-tier coverage bars, a requirement table,
/// and the gap analysis panel.
///
/// # TODO(#2034)
/// `POST /api/planning/projects/{project_id}/verification/refresh` endpoint
/// is assumed but may not exist yet; the Re-verify button is wired to it.
#[component]
pub(crate) fn VerificationView(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::Loading);
    let mut reverifying = use_signal(|| false);
    // WHY: incrementing this signal causes the fetch effect to re-run.
    let mut fetch_trigger = use_signal(|| 0u32);

    let project_id_effect = project_id.clone();
    let project_id_reverify = project_id.clone();

    // Re-runs on mount and whenever fetch_trigger changes.
    use_effect(move || {
        let _ = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/verification",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<VerificationResult>().await {
                        Ok(result) => {
                            fetch_state.set(FetchState::Loaded(VerificationStore {
                                result: Some(result),
                            }));
                        }
                        Err(e) => {
                            fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                // WHY: 404 means verification endpoint not yet on this pylon version.
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
    });

    // Re-verify: call the refresh endpoint then increment fetch_trigger to re-fetch.
    let do_reverify = move |_| {
        let cfg = config.read().clone();
        let pid = project_id_reverify.clone();
        reverifying.set(true);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/verification/refresh",
                cfg.server_url.trim_end_matches('/')
            );

            match client.post(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    reverifying.set(false);
                    let next = *fetch_trigger.peek() + 1;
                    fetch_trigger.set(next);
                }
                Ok(resp) => {
                    tracing::warn!("re-verify returned {}", resp.status());
                    reverifying.set(false);
                }
                Err(e) => {
                    tracing::warn!("re-verify error: {e}");
                    reverifying.set(false);
                }
            }
        });
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_ROW}",
                h3 { style: "margin: 0; font-size: var(--text-md); color: var(--text-primary);", "Verification" }
                div {
                    style: "display: flex; gap: var(--space-2); align-items: center;",
                    button {
                        style: "{VERIFY_BTN}",
                        disabled: *reverifying.read(),
                        onclick: do_reverify,
                        if *reverifying.read() { "Verifying..." } else { "Re-verify" }
                    }
                    button {
                        style: "{REFRESH_BTN}",
                        onclick: move |_| {
                            let next = *fetch_trigger.peek() + 1;
                            fetch_trigger.set(next);
                        },
                        "Refresh"
                    }
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                        "Loading verification..."
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
                        div { style: "font-size: var(--text-md);", "Verification not available" }
                        div { style: "font-size: var(--text-sm); max-width: 360px; text-align: center;",
                            "The verification API is not available on this pylon instance."
                        }
                    }
                },
                FetchState::Loaded(store) => {
                    let overall = store.overall_coverage();
                    let v1_cov = store.tier_coverage("v1");
                    let v2_cov = store.tier_coverage("v2");
                    let last_verified = store
                        .result
                        .as_ref()
                        .map(|r| r.last_verified_at.as_str())
                        .unwrap_or("unknown");
                    let reqs: Vec<RequirementVerification> = store
                        .result
                        .as_ref()
                        .map(|r| r.requirements.clone())
                        .unwrap_or_default();

                    rsx! {
                        div {
                            style: "flex: 1; overflow-y: auto;",
                            // Timestamp
                            div { style: "font-size: var(--text-xs); color: var(--text-muted); margin-bottom: var(--space-3);",
                                "Last verified: {last_verified}"
                            }

                            // Coverage bars
                            div {
                                style: "{COVERAGE_SECTION}",
                                div { style: "{SECTION_LABEL}", "Coverage" }
                                CoverageBar { coverage: overall, label: "Overall".to_string() }
                                CoverageBar { coverage: v1_cov, label: "v1".to_string() }
                                CoverageBar { coverage: v2_cov, label: "v2".to_string() }
                            }

                            // Requirement table
                            div { style: "{SECTION_LABEL}", "Requirements" }
                            if reqs.is_empty() {
                                div { style: "color: var(--text-muted); font-size: var(--text-sm); padding: var(--space-2) 0;",
                                    "No requirements defined."
                                }
                            } else {
                                table {
                                    style: "{TABLE_STYLE}",
                                    thead {
                                        tr {
                                            th { style: "{TH_STYLE}", "Requirement" }
                                            th { style: "{TH_STYLE}", "Status" }
                                            th { style: "{TH_STYLE}", "Coverage" }
                                            th { style: "{TH_STYLE}", "Tier" }
                                            th { style: "{TH_STYLE}", "Evidence" }
                                        }
                                    }
                                    tbody {
                                        for req in &reqs {
                                            {
                                                let status_color = req_status_color(req.status);
                                                let status_label = req_status_label(req.status);
                                                let cov_color = coverage_color(req.coverage_pct);
                                                let evidence_summary = if req.evidence.is_empty() {
                                                    "—".to_string()
                                                } else {
                                                    req.evidence
                                                        .iter()
                                                        .map(|e| e.label.as_str())
                                                        .collect::<Vec<_>>()
                                                        .join(", ")
                                                };
                                                rsx! {
                                                    tr {
                                                        key: "{req.id}",
                                                        td { style: "{TD_STYLE} color: var(--text-primary);", "{req.title}" }
                                                        td {
                                                            style: "{TD_STYLE} color: {status_color};",
                                                            "{status_label}"
                                                        }
                                                        td {
                                                            style: "{TD_STYLE} color: {cov_color}; font-weight: var(--weight-semibold);",
                                                            "{req.coverage_pct}%"
                                                        }
                                                        td { style: "{TD_STYLE} color: var(--text-secondary);", "{req.tier}" }
                                                        td { style: "{TD_STYLE} color: var(--text-secondary);", "{evidence_summary}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Gap analysis panel
                            div {
                                style: "{GAP_SECTION}",
                                div { style: "{SECTION_LABEL}", "Gap Analysis" }
                                GapAnalysisPanel { requirements: reqs }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn req_status_color(status: crate::state::verification::VerificationStatus) -> &'static str {
    use crate::state::verification::VerificationStatus;
    match status {
        VerificationStatus::Verified => "var(--status-success)",
        VerificationStatus::PartiallyVerified => "var(--status-warning)",
        VerificationStatus::Unverified => "var(--text-secondary)",
        VerificationStatus::Failed => "var(--status-error)",
    }
}

fn req_status_label(status: crate::state::verification::VerificationStatus) -> &'static str {
    use crate::state::verification::VerificationStatus;
    match status {
        VerificationStatus::Verified => "Verified",
        VerificationStatus::PartiallyVerified => "Partial",
        VerificationStatus::Unverified => "Unverified",
        VerificationStatus::Failed => "Failed",
    }
}
