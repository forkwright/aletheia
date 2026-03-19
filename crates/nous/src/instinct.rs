//! Instinct observation bridge: records tool usage from the nous pipeline
//! into mneme's instinct system for behavioral pattern learning.

use tracing::{debug, instrument, warn};

use aletheia_mneme::instinct::{
    ToolObservation, ToolOutcome, sanitize_parameters, truncate_context_summary,
};

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

    let sanitized_params = sanitize_parameters(&tool_call.input);
    let truncated_summary = truncate_context_summary(context_summary);

    ToolObservation {
        tool_name: tool_call.name.clone(),
        parameters: sanitized_params,
        outcome,
        context_summary: truncated_summary,
        nous_id: nous_id.to_owned(),
        observed_at: jiff::Timestamp::now(),
    }
}

/// Record tool observations from a completed turn.
///
/// Extracts observations from all tool calls and logs them. In a full
/// deployment with a knowledge store, these would be persisted to the
/// `tool_observations` relation for later aggregation.
#[instrument(skip_all, fields(nous_id = %nous_id, tool_count = tool_calls.len()))]
pub(crate) fn record_observations(
    tool_calls: &[ToolCall],
    context_summary: &str,
    nous_id: &str,
) -> Vec<ToolObservation> {
    let observations: Vec<ToolObservation> = tool_calls
        .iter()
        .map(|tc| observation_from_tool_call(tc, context_summary, nous_id))
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
        }
    }

    #[test]
    fn observation_from_successful_tool_call() {
        let tc = make_tool_call("grep", false);
        let obs = observation_from_tool_call(&tc, "searching code", "nous-1");
        assert_eq!(obs.tool_name, "grep");
        assert_eq!(obs.nous_id, "nous-1");
        assert!(obs.outcome.is_success());
        // Secret should be redacted in sanitized params
        assert_eq!(obs.parameters["api_key"], "[REDACTED]");
        assert_eq!(obs.parameters["path"], "/src/main.rs");
    }

    #[test]
    fn observation_from_failed_tool_call() {
        let tc = make_tool_call("exec", true);
        let obs = observation_from_tool_call(&tc, "running command", "nous-2");
        assert!(!obs.outcome.is_success());
        match &obs.outcome {
            ToolOutcome::Failure { error } => assert!(error.contains("tool failed")),
            _ => panic!("expected Failure outcome"),
        }
    }

    #[test]
    fn record_observations_returns_all() {
        let calls = vec![
            make_tool_call("grep", false),
            make_tool_call("read_file", false),
            make_tool_call("exec", true),
        ];
        let observations = record_observations(&calls, "multi-tool turn", "nous-1");
        assert_eq!(observations.len(), 3);
    }

    #[test]
    fn record_observations_empty_calls() {
        let observations = record_observations(&[], "no tools used", "nous-1");
        assert!(observations.is_empty());
    }
}
