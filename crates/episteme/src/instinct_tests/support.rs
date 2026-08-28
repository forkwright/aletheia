//! Shared observation-fixture builders for instinct tests.
#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use crate::knowledge::parse_timestamp;

pub(super) fn ts(s: &str) -> jiff::Timestamp {
    parse_timestamp(s).expect("valid test timestamp")
}

pub(super) fn make_observation(
    tool_name: &str,
    context: &str,
    outcome: ToolOutcome,
    timestamp: &str,
) -> ToolObservation {
    ToolObservation {
        tool_name: tool_name.to_owned(),
        parameters: serde_json::json!({}),
        outcome,
        context_summary: context.to_owned(),
        nous_id: "test-nous".to_owned(),
        project_id: None,
        observed_at: ts(timestamp),
    }
}
