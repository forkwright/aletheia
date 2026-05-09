#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

mod loop_detector;
mod pipeline_types;

#[tokio::test]
async fn stage_timeout_helper_returns_typed_timeout_for_history() {
    let config = NousConfig::default();
    let emitter = EventEmitter::new();

    let err = run_stage_with_timeout(&config, "history", 1, &emitter, async {
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

    let err = run_stage_with_timeout(&config, "context", 1, &emitter, async {
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

    let err = run_stage_with_timeout(&config, "guard", 1, &emitter, async {
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

    let err = run_stage_with_timeout(&config, "finalize", 1, &emitter, async {
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

    let err = run_stage_with_timeout(&config, "reflection", 1, &emitter, async {
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

    let result = run_stage_with_timeout(&config, "history", 0, &emitter, async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(42)
    })
    .await;

    assert_eq!(result.expect("should succeed"), 42);
}
