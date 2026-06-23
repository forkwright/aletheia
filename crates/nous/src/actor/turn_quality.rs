//! Per-turn quality guards and outcome recording.

use std::sync::Arc;

use aletheia_routing::RoutingDecision;
use aletheia_routing::types::{ProviderId, TaskCategory, TurnOutcome};
use tracing::warn;

use super::NousActor;
use crate::drift::{DriftConfig, DriftDetector, TurnMetrics};
use crate::pipeline::TurnResult;

impl NousActor {
    /// Apply the consecutive-mistake brake to a turn result.
    ///
    /// Increments the global counter on no-progress turns, resets it on tool-use
    /// turns, and increments per-tool-group counters for failed tools. When the
    /// global limit is reached, the turn content is replaced with an intervention
    /// message and the brake is tripped. The brake resets on the next user turn.
    pub(super) fn apply_mistake_brake(
        &mut self,
        session_key: &str,
        result: &mut crate::error::Result<TurnResult>,
    ) {
        let Ok(turn_result) = result else { return };
        let Some(session) = self.sessions.get_mut(session_key) else {
            return;
        };

        if turn_result.tool_calls.is_empty() {
            session.consecutive_no_progress_count += 1;
        } else {
            session.consecutive_no_progress_count = 0;
            for tc in &turn_result.tool_calls {
                if !tc.is_error
                    && let Some(def) = koina::id::ToolName::new(&tc.name)
                        .ok()
                        .and_then(|n| self.services.tools.get_def(&n))
                {
                    for group in &def.groups {
                        session.consecutive_mistake_counts.remove(group);
                    }
                }
            }
        }

        for tc in &turn_result.tool_calls {
            if tc.is_error
                && let Some(def) = koina::id::ToolName::new(&tc.name)
                    .ok()
                    .and_then(|n| self.services.tools.get_def(&n))
            {
                for group in &def.groups {
                    *session
                        .consecutive_mistake_counts
                        .entry(*group)
                        .or_insert(0) += 1;
                }
            }
        }

        let limit = self.config.limits.consecutive_mistake_limit;
        if session.consecutive_no_progress_count >= limit {
            warn!(
                session_key = %session_key,
                count = session.consecutive_no_progress_count,
                limit,
                "consecutive no-progress brake fired"
            );
            turn_result.content = format!(
                "[System: No progress detected for {} consecutive turns. \
                 The agent has not used any tools. Please provide guidance \
                 or clarification to continue.]",
                session.consecutive_no_progress_count
            );
            session.brake_tripped = true;
        }
    }

    /// Apply the extended loop guard (doom-loop, ping-pong, no-progress) to a
    /// turn result.
    ///
    /// If any detector fires, the turn content is replaced with an
    /// operator-intervention message and the brake is tripped. The guard
    /// resets on the next operator-intervention turn via
    /// [`mark_turn_active`](Self::mark_turn_active).
    pub(super) fn apply_loop_guard(
        &mut self,
        session_key: &str,
        result: &mut crate::error::Result<TurnResult>,
    ) {
        let Ok(turn_result) = result else { return };
        let Some(session) = self.sessions.get_mut(session_key) else {
            return;
        };

        let tool_call_data: Vec<(String, String, String)> = turn_result
            .tool_calls
            .iter()
            .map(|tc| {
                (
                    tc.name.clone(),
                    tc.input.to_string(),
                    loop_guard_result_signature(tc.result.as_deref()),
                )
            })
            .collect();
        let tool_refs: Vec<(&str, &str, &str)> = tool_call_data
            .iter()
            .map(|(n, a, r)| (n.as_str(), a.as_str(), r.as_str()))
            .collect();

        if let Err(e) =
            session
                .loop_guard
                .record(&turn_result.content, &turn_result.reasoning, &tool_refs)
        {
            warn!(
                session_key = %session_key,
                error = %e,
                "loop guard detected agent loop — halting turn"
            );
            turn_result.content = format!(
                "[System: Agent loop detected ({e}). \
                 The agent appears to be stuck in a repetitive pattern. \
                 Please provide guidance or clarification to continue.]"
            );
            session.brake_tripped = true;
        }
    }

    /// Extract quality metrics from a turn result and feed them to the
    /// per-session drift detector.
    pub(super) fn record_drift_metrics(&mut self, session_key: &str, turn_result: &TurnResult) {
        let total_calls = turn_result.tool_calls.len();
        let error_calls = turn_result
            .tool_calls
            .iter()
            .filter(|tc| tc.is_error)
            .count();

        // WHY: tool call counts are in the low tens per turn; u32 is ample and
        // the u32→f64 conversion is lossless (u32::MAX < f64 mantissa precision).
        let tool_error_rate = if total_calls > 0 {
            let errors = f64::from(u32::try_from(error_calls).unwrap_or(u32::MAX));
            let total = f64::from(u32::try_from(total_calls).unwrap_or(u32::MAX));
            errors / total
        } else {
            0.0
        };

        let metrics = TurnMetrics {
            response_tokens: turn_result.usage.output_tokens,
            tool_error_rate,
            // WHY: user_correction detection requires classifying the *next*
            // user message, which is not available at finalize time. Default
            // to false; a future hook or the next turn's preprocessing can
            // retroactively set this via `mark_correction`.
            user_correction: false,
            tool_call_count: u32::try_from(total_calls).unwrap_or(u32::MAX),
            timestamp: jiff::Timestamp::now(),
        };

        let detector = self
            .drift_detectors
            .entry(session_key.to_owned())
            .or_insert_with(|| DriftDetector::new(DriftConfig::default()));

        let _drift_events = detector.record(metrics);
        // NOTE: drift events are already logged at warn level by the detector.
        // Future work: store drift_events in session metadata for API exposure.
    }

    /// Record the turn outcome in the empirical router.
    ///
    /// Called from `finalize_turn` after a successful turn. Uses heuristic
    /// category inference from the user message text and treats any non-degraded
    /// turn as a success (coarse signal; good enough for routing calibration).
    ///
    /// WHY fire-and-forget via the trait: `Router::after_action` is sync and
    /// internally spawns the store write as a background task so the response
    /// path is never blocked by the write lock.
    #[tracing::instrument(skip(self, content, turn_result), fields(model = %turn_result.model_used))]
    pub(super) fn record_router_outcome(&self, content: &str, turn_result: &TurnResult) {
        if turn_result.model_used.is_empty() {
            // WHY: degraded-mode turns have no model; skip.
            return;
        }

        let task_category = TaskCategory::from_prompt(content);
        // WHY: a non-degraded turn that reached the LLM is a routing success.
        // Degraded turns are excluded by the early return above.
        let outcome = TurnOutcome::new(
            ProviderId::new(turn_result.model_used.as_str()),
            task_category,
            turn_result.degraded.is_none(),
            true, // is_interactive
        );

        // WHY: confidence is not available at finalize time (the store was
        // queried during execute), so the decision carries only the provider.
        let decision = RoutingDecision::new(Arc::from(turn_result.model_used.as_str()), None);

        if let Err(e) = self.services.router.after_action(&decision, &outcome) {
            tracing::warn!(error = %e, "empirical router after_action failed");
        }
    }
}

fn loop_guard_result_signature(result: Option<&str>) -> String {
    match result {
        Some(result) => format!("present:{result}"),
        None => "missing".to_owned(),
    }
}
