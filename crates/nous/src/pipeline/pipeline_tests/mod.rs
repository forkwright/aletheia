#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: indices guarded by preceding assert"
)]
use super::*;

use crate::budget::{StageTimingStatus, TimeBudget};
use crate::config::StageBudget;

mod loop_detector;
mod pipeline_types;

#[tokio::test]
async fn stage_timeout_helper_returns_typed_timeout_for_history() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        history_secs: 1,
        ..StageBudget::default()
    });

    let err = run_stage_with_timeout(&config, "history", &mut time_budget, &emitter, async {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(())
    })
    .await
    .expect_err("stage should time out");

    assert!(
        matches!(
            err,
            error::Error::PipelineTimeout {
                ref stage,
                timeout_secs: 1,
                ..
            } if stage == "history"
        ),
        "expected typed history timeout, got {err:?}"
    );
}

#[tokio::test]
async fn stage_timeout_helper_returns_typed_timeout_for_context() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        context_secs: 1,
        ..StageBudget::default()
    });

    let err = run_stage_with_timeout(&config, "context", &mut time_budget, &emitter, async {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(())
    })
    .await
    .expect_err("stage should time out");

    assert!(
        matches!(
            err,
            error::Error::PipelineTimeout {
                ref stage,
                timeout_secs: 1,
                ..
            } if stage == "context"
        ),
        "expected typed context timeout, got {err:?}"
    );
}

#[tokio::test]
async fn stage_timeout_helper_returns_typed_timeout_for_guard() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        guard_secs: 1,
        ..StageBudget::default()
    });

    let err = run_stage_with_timeout(&config, "guard", &mut time_budget, &emitter, async {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(())
    })
    .await
    .expect_err("stage should time out");

    assert!(
        matches!(
            err,
            error::Error::PipelineTimeout {
                ref stage,
                timeout_secs: 1,
                ..
            } if stage == "guard"
        ),
        "expected typed guard timeout, got {err:?}"
    );
}

#[tokio::test]
async fn stage_timeout_helper_returns_typed_timeout_for_finalize() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        finalize_secs: 1,
        ..StageBudget::default()
    });

    let err = run_stage_with_timeout(&config, "finalize", &mut time_budget, &emitter, async {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(())
    })
    .await
    .expect_err("stage should time out");

    assert!(
        matches!(
            err,
            error::Error::PipelineTimeout {
                ref stage,
                timeout_secs: 1,
                ..
            } if stage == "finalize"
        ),
        "expected typed finalize timeout, got {err:?}"
    );
}

#[tokio::test]
async fn stage_timeout_helper_returns_typed_timeout_for_reflection() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        reflection_secs: 1,
        ..StageBudget::default()
    });

    let err = run_stage_with_timeout(&config, "reflection", &mut time_budget, &emitter, async {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(())
    })
    .await
    .expect_err("stage should time out");

    assert!(
        matches!(
            err,
            error::Error::PipelineTimeout {
                ref stage,
                timeout_secs: 1,
                ..
            } if stage == "reflection"
        ),
        "expected typed reflection timeout, got {err:?}"
    );
}

#[tokio::test]
async fn stage_timeout_disabled_when_secs_is_zero() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        history_secs: 0,
        ..StageBudget::default()
    });

    let result = run_stage_with_timeout(&config, "history", &mut time_budget, &emitter, async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(42)
    })
    .await;

    assert_eq!(result.expect("should succeed"), 42);
}

#[tokio::test]
async fn stage_with_time_budget_records_timeout_status() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        history_secs: 1,
        total_secs: 300,
        ..StageBudget::default()
    });

    let err = run_stage_with_timeout(&config, "history", &mut time_budget, &emitter, async {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(())
    })
    .await
    .expect_err("stage should time out");

    assert!(matches!(err, error::Error::PipelineTimeout { .. }));
    let summary = time_budget.summary();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].name, "history");
    assert_eq!(summary[0].status, StageTimingStatus::TimedOut);
}

// --- recall-stage-timeout degrade categorization (aletheia#7218) ---

#[test]
fn recall_timeout_or_propagate_downgrades_only_a_recall_timeout() {
    assert_eq!(
        recall_timeout_or_propagate(Ok(())).expect("Ok should stay Ok"),
        None,
        "a completed recall stage carries no timeout marker"
    );

    let recall_timeout = error::PipelineTimeoutSnafu {
        stage: "recall",
        timeout_secs: 15u32,
    }
    .build();
    assert_eq!(
        recall_timeout_or_propagate(Err(recall_timeout)).expect("recall timeout must degrade"),
        Some(15),
        "a recall-stage timeout must downgrade to a degrade marker, not an error"
    );
}

#[test]
fn recall_timeout_or_propagate_still_fails_other_stage_timeouts() {
    let history_timeout = error::PipelineTimeoutSnafu {
        stage: "history",
        timeout_secs: 5u32,
    }
    .build();
    assert!(
        recall_timeout_or_propagate(Err(history_timeout)).is_err(),
        "a timeout on a different stage must still propagate and fail the turn"
    );
}

#[test]
fn recall_timeout_or_propagate_still_fails_non_timeout_errors() {
    let guard_rejected = error::GuardRejectedSnafu {
        reason: "denied".to_owned(),
    }
    .build();
    assert!(
        recall_timeout_or_propagate(Err(guard_rejected)).is_err(),
        "a non-timeout error must still propagate unchanged"
    );
}

#[tokio::test]
async fn stage_with_time_budget_records_completed_status() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();
    let mut time_budget = TimeBudget::new(StageBudget {
        history_secs: 10,
        total_secs: 300,
        ..StageBudget::default()
    });

    let result = run_stage_with_timeout(&config, "history", &mut time_budget, &emitter, async {
        Ok(42)
    })
    .await;

    assert_eq!(result.expect("should succeed"), 42);
    let summary = time_budget.summary();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].name, "history");
    assert_eq!(summary[0].status, StageTimingStatus::Completed);
}
