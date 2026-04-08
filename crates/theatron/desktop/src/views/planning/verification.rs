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
    padding: 16px;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 16px;\
";

const SECTION_LABEL: &str = "\
    font-size: 11px; \
    font-weight: 600; \
    color: #666; \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin: 16px 0 8px;\
";

const COVERAGE_SECTION: &str = "\
    background: #1a1a2e; \
    border: 1px solid #2a2a3a; \
    border-radius: 8px; \
    padding: 14px 16px; \
    margin-bottom: 16px;\
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

const GAP_SECTION: &str = "\
    margin-top: 16px; \
    background: #1a1a2e; \
    border: 1px solid #2a2a3a; \
    border-radius: 8px; \
    padding: 14px 16px;\
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

const VERIFY_BTN: &str = "\
    background: #1a2a4a; \
    color: #4a9aff; \
    border: 1px solid #4a9aff; \
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
                h3 { style: "margin: 0; font-size: 16px; color: #e0e0e0;", "Verification" }
                div {
                    style: "display: flex; gap: 8px; align-items: center;",
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
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading verification..."
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
                        div { style: "font-size: 16px;", "Verification not available" }
                        div { style: "font-size: 13px; max-width: 360px; text-align: center;",
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
                            div { style: "font-size: 11px; color: #555; margin-bottom: 12px;",
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
                                div { style: "color: #555; font-size: 13px; padding: 8px 0;",
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
                                                        td { style: "{TD_STYLE} color: #e0e0e0;", "{req.title}" }
                                                        td {
                                                            style: "{TD_STYLE} color: {status_color};",
                                                            "{status_label}"
                                                        }
                                                        td {
                                                            style: "{TD_STYLE} color: {cov_color}; font-weight: 600;",
                                                            "{req.coverage_pct}%"
                                                        }
                                                        td { style: "{TD_STYLE} color: #888;", "{req.tier}" }
                                                        td { style: "{TD_STYLE} color: #aaa;", "{evidence_summary}" }
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
        VerificationStatus::Verified => "#22c55e",
        VerificationStatus::PartiallyVerified => "#f59e0b",
        VerificationStatus::Unverified => "#888",
        VerificationStatus::Failed => "#ef4444",
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
