//! (Split from `actor/tests.rs` — see parent mod.)

use super::*;

#[test]
fn mark_turn_active_sets_active_lifecycle_and_session() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.channel.status, NousLifecycle::Idle);
    assert!(actor.active_session.is_none());
    assert!(
        !actor
            .runtime
            .active_turn
            .load(std::sync::atomic::Ordering::Acquire)
    );

    actor.mark_turn_active("my-session");

    assert_eq!(actor.channel.status, NousLifecycle::Active);
    assert_eq!(actor.active_session.as_deref(), Some("my-session"));
    assert!(
        actor
            .runtime
            .active_turn
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[test]
fn mark_turn_active_auto_wakes_from_dormant() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.channel.status = NousLifecycle::Dormant;

    actor.mark_turn_active("s");

    // After auto-wake + active set, lifecycle must be Active.
    assert_eq!(actor.channel.status, NousLifecycle::Active);
}

// ── turn.rs: record_pipeline_panic ───────────────────────────────────────────

#[test]
fn record_pipeline_panic_increments_count_and_records_timestamp() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.runtime.pipeline_panic_count, 0);
    assert!(actor.runtime.last_panic_at.is_none());
    assert!(actor.runtime.pipeline_panic_timestamps.is_empty());

    actor.record_pipeline_panic();

    assert_eq!(actor.runtime.pipeline_panic_count, 1);
    assert!(actor.runtime.last_panic_at.is_some());
    assert_eq!(actor.runtime.pipeline_panic_timestamps.len(), 1);
}

#[test]
fn record_pipeline_panic_multiple_increments_count() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    for _ in 0..3 {
        actor.record_pipeline_panic();
    }

    assert_eq!(actor.runtime.pipeline_panic_count, 3);
    assert_eq!(actor.runtime.pipeline_panic_timestamps.len(), 3);
}

#[test]
fn record_pipeline_panic_enters_degraded_at_threshold() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.channel.status, NousLifecycle::Idle);

    // DEGRADED_PANIC_THRESHOLD is 5 — trigger exactly 5 panics.
    for _ in 0..5 {
        actor.record_pipeline_panic();
    }

    assert_eq!(
        actor.channel.status,
        NousLifecycle::Degraded,
        "should enter degraded after 5 panics in window"
    );
}

#[test]
fn record_pipeline_panic_below_threshold_stays_idle() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    for _ in 0..4 {
        actor.record_pipeline_panic();
    }

    assert_eq!(actor.channel.status, NousLifecycle::Idle);
}

#[test]
fn record_drift_metrics_creates_detector_for_new_session() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert!(actor.drift_detectors.is_empty());

    let result = make_turn_result(100, vec![]);
    actor.record_drift_metrics("my-session", &result);

    assert!(
        actor.drift_detectors.contains_key("my-session"),
        "detector should be created for new session"
    );
}

#[test]
fn record_drift_metrics_zero_tool_calls_produces_zero_error_rate() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    let result = make_turn_result(50, vec![]);
    // Should not panic; zero-division guard must hold.
    actor.record_drift_metrics("s", &result);
}

#[test]
fn record_drift_metrics_all_errored_calls_produces_rate_one() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    let calls = vec![make_tool_call("bash", true), make_tool_call("read", true)];
    let result = make_turn_result(80, calls);
    // Should not panic; rate = 1.0.
    actor.record_drift_metrics("s", &result);
    // Detector created.
    assert!(actor.drift_detectors.contains_key("s"));
}

#[test]
fn record_drift_metrics_mixed_calls_partial_error_rate() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    let calls = vec![
        make_tool_call("bash", false),
        make_tool_call("read", true), // 1 of 2 errors → rate = 0.5
    ];
    let result = make_turn_result(60, calls);
    actor.record_drift_metrics("s", &result);
    assert!(actor.drift_detectors.contains_key("s"));
}

#[test]
fn record_drift_metrics_accumulates_across_turns() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    for _ in 0..3 {
        let result = make_turn_result(50, vec![]);
        actor.record_drift_metrics("s", &result);
    }
    // Still only one detector for the session.
    assert_eq!(actor.drift_detectors.len(), 1);
}

// ── background.rs: record_background_panic ───────────────────────────────────

#[test]
fn record_background_panic_increments_count() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.runtime.background_panic_count, 0);

    actor.record_background_panic();

    assert_eq!(actor.runtime.background_panic_count, 1);
    assert_eq!(actor.runtime.background_panic_timestamps.len(), 1);
}

#[test]
fn record_background_panic_does_not_enter_degraded() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    for _ in 0..10 {
        actor.record_background_panic();
    }

    // Background panics never trigger degraded mode — only pipeline panics do.
    assert_eq!(
        actor.channel.status,
        NousLifecycle::Idle,
        "background panics must not enter degraded mode"
    );
    assert_eq!(actor.runtime.background_panic_count, 10);
}
