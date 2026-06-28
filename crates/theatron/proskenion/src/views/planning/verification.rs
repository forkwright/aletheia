//! Verification view: goal-backward requirement verification results.

use dioxus::prelude::*;
use skene::api::routes::planning::{project_verification_refresh_url, project_verification_url};

use crate::api::client::authenticated_client;
use crate::components::coverage_bar::{CoverageBar, coverage_color};
use crate::state::connection::ConnectionConfig;
use crate::state::toasts::{ToastSeverity, ToastStore};
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

const PRIVACY_ROW: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    gap: var(--space-2); \
    margin-bottom: var(--space-3);\
";

const PRIVACY_BADGE: &str = "\
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: 2px var(--space-2); \
    color: var(--text-secondary); \
    font-size: var(--text-xs); \
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
    border-bottom: 1px solid var(--border-separator); \
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
    background: var(--bg-surface); \
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
/// Fetches from `GET /api/v1/planning/projects/{project_id}/verification`.
/// Displays overall and per-tier coverage bars, a requirement table,
/// and the gap analysis panel.
#[component]
pub(crate) fn VerificationView(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let toast_store = try_consume_context::<Signal<ToastStore>>();
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
            let client = match authenticated_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    fetch_state.set(FetchState::Error(err.to_string()));
                    return;
                }
            };
            let url = project_verification_url(&cfg.server_url, &pid);

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
        let toast_store = toast_store;
        reverifying.set(true);

        spawn(async move {
            let client = match authenticated_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    if let Some(mut store) = toast_store {
                        store.write().push_full(
                            ToastSeverity::Error,
                            "Re-verify failed".to_owned(),
                            Some(err.to_string()),
                            None,
                        );
                    }
                    reverifying.set(false);
                    return;
                }
            };
            let url = project_verification_refresh_url(&cfg.server_url, &pid);

            match client.post(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    reverifying.set(false);
                    let next = *fetch_trigger.peek() + 1;
                    fetch_trigger.set(next);
                }
                Ok(resp) => {
                    let status = resp.status();
                    tracing::warn!("re-verify returned {status}");
                    if let Some(mut store) = toast_store {
                        let (title, body) = reverify_failure_message(status.as_u16());
                        store.write().push_full(
                            ToastSeverity::Error,
                            title.to_owned(),
                            Some(body.to_owned()),
                            None,
                        );
                    }
                    reverifying.set(false);
                }
                Err(e) => {
                    tracing::warn!("re-verify error: {e}");
                    if let Some(mut store) = toast_store {
                        store.write().push_full(
                            ToastSeverity::Error,
                            "Re-verify failed".to_owned(),
                            Some(format!(
                                "Could not reach the verification refresh endpoint: {e}"
                            )),
                            None,
                        );
                    }
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
                    let (visibility, classification, redacted) =
                        verification_privacy(store.result.as_ref());
                    let redaction = redaction_label(redacted);
                    let reqs: Vec<RequirementVerification> = store
                        .result
                        .as_ref()
                        .map_or_else(Vec::new, |r| r.requirements.clone());

                    rsx! {
                        div {
                            style: "flex: 1; overflow-y: auto;",
                            div { style: "font-size: var(--text-xs); color: var(--text-muted); margin-bottom: var(--space-3);",
                                "Last verified: {last_verified}"
                            }
                            div {
                                style: "{PRIVACY_ROW}",
                                span { style: "{PRIVACY_BADGE}", "Visibility: {visibility}" }
                                span { style: "{PRIVACY_BADGE}", "Classification: {classification}" }
                                span { style: "{PRIVACY_BADGE}", "{redaction}" }
                            }

                            div {
                                style: "{COVERAGE_SECTION}",
                                div { style: "{SECTION_LABEL}", "Coverage" }
                                CoverageBar { coverage: overall, label: "Overall".to_string() }
                                CoverageBar { coverage: v1_cov, label: "v1".to_string() }
                                CoverageBar { coverage: v2_cov, label: "v2".to_string() }
                            }

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

fn reverify_failure_message(status: u16) -> (&'static str, &'static str) {
    match status {
        404 | 501 => (
            "Re-verify unavailable",
            "This pylon instance does not expose the verification refresh endpoint.",
        ),
        _ => (
            "Re-verify failed",
            "The verification refresh request returned an error. Try Refresh or check the server logs.",
        ),
    }
}

fn verification_privacy(result: Option<&VerificationResult>) -> (String, String, bool) {
    result.map_or_else(
        || ("private".to_string(), "restricted".to_string(), true),
        |result| {
            (
                result.visibility.clone(),
                result.classification.clone(),
                result.redacted,
            )
        },
    )
}

fn redaction_label(redacted: bool) -> &'static str {
    if redacted {
        "Details redacted"
    } else {
        "Details visible"
    }
}

#[cfg(test)]
mod tests {
    use super::{redaction_label, reverify_failure_message, verification_privacy};

    #[test]
    fn reverify_failure_message_marks_missing_endpoint_unavailable() {
        let (not_found_title, not_found_body) = reverify_failure_message(404);
        assert_eq!(not_found_title, "Re-verify unavailable");
        assert!(not_found_body.contains("does not expose"));

        let (not_implemented_title, _) = reverify_failure_message(501);
        assert_eq!(not_implemented_title, "Re-verify unavailable");
    }

    #[test]
    fn reverify_failure_message_marks_other_statuses_failed() {
        let (title, body) = reverify_failure_message(500);
        assert_eq!(title, "Re-verify failed");
        assert!(body.contains("returned an error"));
    }

    #[test]
    fn redaction_label_tracks_privacy_state() {
        assert_eq!(redaction_label(true), "Details redacted");
        assert_eq!(redaction_label(false), "Details visible");
    }

    #[test]
    fn privacy_defaults_to_restricted_when_result_missing() {
        let (visibility, classification, redacted) = verification_privacy(None);
        assert_eq!(visibility, "private");
        assert_eq!(classification, "restricted");
        assert!(redacted);
    }
}
