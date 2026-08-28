// WHY: `AlwaysPassQa` and the paired success-fixture builders were copied
// verbatim into every pipeline-stage test module (preparation, health_check,
// mod, context, execution, post_processing, validation) plus the orchestrator
// group tests. One field added to `QaResult` or `SessionOutcome`/`MockOutcome`
// meant repairing eight identical literals by hand. Shared here so it means
// repairing one.
#![cfg(test)]

use std::future::Future;
use std::pin::Pin;

use jiff::Timestamp;

use crate::engine::{SessionEvent, SessionResult};
use crate::error::Result;
use crate::http::mock::MockOutcome;
use crate::qa::{PromptSpec, QaGate};
use crate::types::{MechanicalIssue, QaResult, QaVerdict};

/// A [`QaGate`] that always reports [`QaVerdict::Pass`] with no criteria or
/// mechanical issues, and never touches an LLM provider.
pub(crate) struct AlwaysPassQa;

impl QaGate for AlwaysPassQa {
    fn evaluate<'a>(
        &'a self,
        prompt: &'a PromptSpec,
        pr_number: u64,
        _diff: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<QaResult>> + Send + 'a>> {
        Box::pin(async move {
            Ok(QaResult {
                prompt_number: prompt.prompt_number,
                pr_number,
                verdict: QaVerdict::Pass,
                criteria_results: vec![],
                mechanical_issues: vec![],
                reasons: vec![],
                cost_usd: 0.0,
                evaluated_at: Timestamp::now(),
                semantic_evaluated: false,
            })
        })
    }

    fn mechanical_check(&self, _diff: &str, _prompt: &PromptSpec) -> Vec<MechanicalIssue> {
        vec![]
    }
}

/// A [`MockOutcome::Success`] with `success: true` and a plain `"done"` result.
///
/// Shared by orchestrator-group and pipeline-execution/post-processing tests
/// that only need a session to succeed, not to inspect its PR text.
pub(crate) fn success_mock_outcome(session_id: &str, cost: f64, turns: u32) -> MockOutcome {
    MockOutcome::Success {
        events: vec![SessionEvent::TurnComplete { turn: turns }],
        result: SessionResult {
            session_id: session_id.to_owned(),
            cost_usd: cost,
            num_turns: turns,
            duration_ms: 100,
            success: true,
            result_text: Some("done".to_owned()),
            model: Some("claude-3-5-sonnet".to_owned()),
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
        },
    }
}
