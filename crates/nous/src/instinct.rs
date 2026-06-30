//! Instinct observation bridge: builds sanitized tool-use observations from
//! the nous pipeline.

use tracing::{debug, instrument, warn};

use mneme::instinct::{
    DEFAULT_MAX_CONTEXT_SUMMARY_LEN, DEFAULT_MAX_PARAM_VALUE_LEN, ToolObservation, ToolOutcome,
    sanitize_parameters, truncate_context_summary,
};
use mneme::workspace::ProjectId;

use crate::pipeline::ToolCall;

/// Build a [`ToolObservation`] from a completed tool call.
///
/// Sanitizes parameters (secrets stripped, values truncated) and truncates
/// the context summary before returning. Returns `None` if serialization fails.
#[instrument(skip_all, fields(tool = %tool_call.name))]
pub(crate) fn observation_from_tool_call(
    tool_call: &ToolCall,
    context_summary: &str,
    nous_id: &str,
    project_id: Option<&ProjectId>,
) -> ToolObservation {
    let outcome = if tool_call.is_error {
        ToolOutcome::Failure {
            error: tool_call
                .result
                .as_deref()
                .unwrap_or("unknown error")
                .chars()
                .take(200)
                .collect(),
        }
    } else {
        ToolOutcome::Success
    };

    let sanitized_params = sanitize_parameters(&tool_call.input, DEFAULT_MAX_PARAM_VALUE_LEN);
    let truncated_summary =
        truncate_context_summary(context_summary, DEFAULT_MAX_CONTEXT_SUMMARY_LEN);

    ToolObservation {
        tool_name: tool_call.name.clone(),
        parameters: sanitized_params,
        outcome,
        context_summary: truncated_summary,
        nous_id: nous_id.to_owned(),
        project_id: project_id.cloned(),
        observed_at: jiff::Timestamp::now(),
    }
}

/// Record tool observations from a completed turn.
///
/// Extracts observations from all tool calls and logs them. These observations
/// are intentionally ephemeral until a persistent instinct store is wired into
/// the pipeline; callers should treat the returned vector as turn telemetry,
/// not durable behavioral memory.
#[instrument(skip_all, fields(nous_id = %nous_id, tool_count = tool_calls.len()))]
pub(crate) fn record_observations(
    tool_calls: &[ToolCall],
    context_summary: &str,
    nous_id: &str,
    project_id: Option<&ProjectId>,
) -> Vec<ToolObservation> {
    let observations: Vec<ToolObservation> = tool_calls
        .iter()
        .map(|tc| observation_from_tool_call(tc, context_summary, nous_id, project_id))
        .collect();

    if observations.is_empty() {
        return observations;
    }

    let success_count = observations
        .iter()
        .filter(|o| o.outcome.is_success())
        .count();
    debug!(
        total = observations.len(),
        successes = success_count,
        "recorded tool observations for instinct system"
    );

    for obs in &observations {
        if !obs.outcome.is_success() {
            warn!(
                tool = %obs.tool_name,
                outcome = %obs.outcome.as_stored_string(),
                "tool observation recorded with non-success outcome"
            );
        }
    }

    observations
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::ToolCall;

    fn make_tool_call(name: &str, is_error: bool) -> ToolCall {
        ToolCall {
            id: format!("tc-{name}"),
            name: name.to_owned(),
            input: serde_json::json!({"path": "/src/main.rs", "api_key": "test-key"}),
            result: if is_error {
                Some("tool failed".to_owned())
            } else {
                Some("success output".to_owned())
            },
            is_error,
            duration_ms: 42,
            approval: None,
            receipt: None,
        }
    }

    #[test]
    fn observation_from_successful_tool_call() {
        let tc = make_tool_call("grep", false);
        let obs = observation_from_tool_call(&tc, "searching code", "nous-1", None);
        assert_eq!(obs.tool_name, "grep");
        assert_eq!(obs.nous_id, "nous-1");
        assert!(obs.project_id.is_none());
        assert!(obs.outcome.is_success());
        assert_eq!(obs.parameters["api_key"], "[REDACTED]");
        assert_eq!(obs.parameters["path"], "/src/main.rs");
    }

    #[test]
    fn observation_from_failed_tool_call() {
        let tc = make_tool_call("exec", true);
        let obs = observation_from_tool_call(&tc, "running command", "nous-2", None);
        assert!(!obs.outcome.is_success());
        match &obs.outcome {
            ToolOutcome::Failure { error } => assert!(error.contains("tool failed")),
            _ => panic!("expected Failure outcome"),
        }
    }

    #[test]
    fn observation_preserves_project_partition() {
        let tc = make_tool_call("grep", false);
        let project_id = ProjectId::from_git_remote("https://github.com/forkwright/aletheia.git")
            .expect("valid remote");

        let obs =
            observation_from_tool_call(&tc, "searching project code", "nous-1", Some(&project_id));

        assert_eq!(obs.project_id.as_ref(), Some(&project_id));
    }

    #[test]
    fn record_observations_returns_all() {
        let calls = vec![
            make_tool_call("grep", false),
            make_tool_call("read_file", false),
            make_tool_call("exec", true),
        ];
        let observations = record_observations(&calls, "multi-tool turn", "nous-1", None);
        assert_eq!(observations.len(), 3);
    }

    #[test]
    fn record_observations_empty_calls() {
        let observations = record_observations(&[], "no tools used", "nous-1", None);
        assert!(observations.is_empty());
    }
}
