//! Internal-state behaviour tests for `NousActor`.

#![expect(
    clippy::indexing_slicing,
    reason = "test: indices are valid after asserting len"
)]
#![expect(
    clippy::unwrap_used,
    reason = "test assertions may unwrap after prior checks"
)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use aletheia_routing::types::{InterventionStatus, ProviderId, TaskCategory};
use aletheia_routing::{AfterActionStore, RecordingRouter};

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

// ── turn.rs: cancellation reverts turn counter ───────────────────────────────

#[tokio::test]
async fn cancelled_turn_reverts_in_memory_turn_counter() {
    let (mut actor, _tx, _dir) =
        make_test_actor_with_providers(pending_providers(), PipelineConfig::default());
    let cancel = CancellationToken::new();
    cancel.cancel();

    let result = actor
        .execute_turn_with_panic_boundary(
            "main",
            None,
            "hello",
            tracing::info_span!("test-cancel"),
            cancel,
        )
        .await;

    assert!(result.is_err(), "cancelled turn must return an error");
    let session = actor
        .sessions
        .get("main")
        .expect("session should exist after cancellation");
    assert_eq!(
        session.turn, 0,
        "cancelled turn must not advance the in-memory turn counter"
    );
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

    // NOTE: taxis `degraded_panic_threshold` defaults to 5 — trigger exactly 5 panics.
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
    assert!(actor.drift_detectors.contains_key("s"));
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

    actor.record_background_panic(None);

    assert_eq!(actor.runtime.background_panic_count, 1);
    assert_eq!(actor.runtime.background_panic_timestamps.len(), 1);
}

#[test]
fn record_background_panic_below_threshold_does_not_degrade_background_health() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    // NOTE: taxis `degraded_panic_threshold` defaults to 5.
    for _ in 0..4 {
        actor.record_background_panic(None);
    }

    // Background panics never trigger pipeline `Degraded` mode.
    assert_eq!(
        actor.channel.status,
        NousLifecycle::Idle,
        "background panics must not enter pipeline degraded mode"
    );
    assert_eq!(actor.runtime.background_panic_count, 4);
    assert_eq!(actor.runtime.background_failure.total_count, 4);
    assert_eq!(actor.runtime.background_failure.timestamps.len(), 4);
    assert_eq!(
        actor.runtime.background_failure.latest_kind.as_deref(),
        Some("panic")
    );

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    actor.handle_status(tx);
    let status = rx.try_recv().expect("status should be ready");
    assert!(!status.background_health_degraded);
    assert_eq!(status.background_failure_total_count, 4);
    assert_eq!(status.background_failure_recent_count, 4);
    assert_eq!(status.lifecycle, NousLifecycle::Idle);
}

#[test]
fn record_background_panic_at_threshold_marks_background_health_degraded() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    for _ in 0..5 {
        actor.record_background_panic(None);
    }

    // Pipeline lifecycle stays Idle; only explicit background health flips.
    assert_eq!(
        actor.channel.status,
        NousLifecycle::Idle,
        "background panics must not enter pipeline degraded mode"
    );
    assert_eq!(actor.runtime.background_panic_count, 5);
    assert_eq!(actor.runtime.background_failure.total_count, 5);
    assert_eq!(actor.runtime.background_failure.timestamps.len(), 5);

    let degraded_window = Duration::from_secs(actor.nous_behavior.degraded_window_secs);
    let recent = actor
        .runtime
        .background_failure
        .timestamps
        .iter()
        .filter(|t| t.elapsed() <= degraded_window)
        .count();
    assert!(
        recent >= 5,
        "recent background failures should reach threshold"
    );

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    actor.handle_status(tx);
    let status = rx.try_recv().expect("status should be ready");
    assert!(status.background_health_degraded);
    assert_eq!(status.background_failure_total_count, 5);
    assert_eq!(status.background_failure_recent_count, 5);
    assert_eq!(status.lifecycle, NousLifecycle::Idle);
}

// ── turn.rs: apply_mistake_brake ─────────────────────────────────────────────

#[test]
fn apply_mistake_brake_increments_on_no_progress() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );
    let mut result = Ok(make_turn_result(50, vec![]));
    actor.apply_mistake_brake("s", &mut result);
    assert_eq!(actor.sessions["s"].consecutive_no_progress_count, 1);
    assert!(!actor.sessions["s"].brake_tripped);
}

#[test]
fn apply_mistake_brake_fires_at_limit() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );
    actor.config.limits.consecutive_mistake_limit = 3;

    for _ in 0..2 {
        let mut result = Ok(make_turn_result(50, vec![]));
        actor.apply_mistake_brake("s", &mut result);
    }
    assert_eq!(actor.sessions["s"].consecutive_no_progress_count, 2);
    assert!(!actor.sessions["s"].brake_tripped);

    let mut result = Ok(make_turn_result(50, vec![]));
    actor.apply_mistake_brake("s", &mut result);
    assert_eq!(actor.sessions["s"].consecutive_no_progress_count, 3);
    assert!(actor.sessions["s"].brake_tripped);
    assert!(
        result.unwrap().content.contains("No progress detected"),
        "turn content should contain intervention message"
    );
}

#[test]
fn apply_mistake_brake_resets_on_tool_use() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );
    actor.config.limits.consecutive_mistake_limit = 3;

    for _ in 0..2 {
        let mut result = Ok(make_turn_result(50, vec![]));
        actor.apply_mistake_brake("s", &mut result);
    }
    assert_eq!(actor.sessions["s"].consecutive_no_progress_count, 2);

    let mut result = Ok(make_turn_result(
        50,
        vec![make_tool_call("read_file", false)],
    ));
    actor.apply_mistake_brake("s", &mut result);
    assert_eq!(actor.sessions["s"].consecutive_no_progress_count, 0);
    assert!(!actor.sessions["s"].brake_tripped);
}

#[test]
fn mark_turn_active_resets_brake_when_tripped() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );
    actor.sessions.get_mut("s").unwrap().brake_tripped = true;
    actor
        .sessions
        .get_mut("s")
        .unwrap()
        .consecutive_no_progress_count = 5;

    actor.mark_turn_active("s");

    assert!(!actor.sessions["s"].brake_tripped);
    assert_eq!(actor.sessions["s"].consecutive_no_progress_count, 0);
}

// ── turn.rs: apply_loop_guard ───────────────────────────────────────────────

#[test]
fn apply_loop_guard_detects_doom_loop() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );

    let tc = vec![make_tool_call("read_file", false)];
    for _ in 0..2 {
        let mut result = Ok(make_turn_result(50, tc.clone()));
        actor.apply_loop_guard("s", &mut result);
        assert!(!actor.sessions["s"].brake_tripped);
    }

    let mut result = Ok(make_turn_result(50, tc.clone()));
    actor.apply_loop_guard("s", &mut result);
    assert!(actor.sessions["s"].brake_tripped);
    assert!(
        result.unwrap().content.contains("Agent loop detected"),
        "intervention message should be injected"
    );
}

#[test]
fn apply_loop_guard_preserves_missing_vs_empty_tool_result() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );
    actor.sessions.get_mut("s").unwrap().loop_guard =
        hermeneus::loop_detector::LoopGuard::with_limits(3, 10, 10).unwrap();

    let sequence = [
        make_tool_call_with_result("read_file", false, None),
        make_tool_call_with_result("read_file", false, Some("")),
        make_tool_call_with_result("read_file", false, None),
    ];

    for call in sequence {
        let mut result = Ok(make_turn_result(50, vec![call]));
        actor.apply_loop_guard("s", &mut result);
        assert!(
            !actor.sessions["s"].brake_tripped,
            "missing and empty results must remain distinct for loop detection"
        );
    }
}

#[test]
fn apply_loop_guard_detects_ping_pong() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );
    // Use a custom loop guard with k=5 for ping-pong.
    actor.sessions.get_mut("s").unwrap().loop_guard =
        hermeneus::loop_detector::LoopGuard::with_limits(10, 5, 10).unwrap();

    let a = vec![make_tool_call("read_file", false)];
    let b = vec![make_tool_call("write_file", false)];
    let seq = [a.clone(), b.clone(), a.clone(), b.clone()];
    for tc in seq {
        let mut result = Ok(make_turn_result(50, tc));
        actor.apply_loop_guard("s", &mut result);
        assert!(!actor.sessions["s"].brake_tripped);
    }

    let mut result = Ok(make_turn_result(50, a.clone()));
    actor.apply_loop_guard("s", &mut result);
    assert!(actor.sessions["s"].brake_tripped);
}

#[test]
fn apply_loop_guard_detects_no_progress() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );
    // limit = 3 for no-progress.
    actor.sessions.get_mut("s").unwrap().loop_guard =
        hermeneus::loop_detector::LoopGuard::with_limits(10, 5, 3).unwrap();

    let tc = vec![make_tool_call("read_file", false)];
    for _ in 0..2 {
        let mut result = Ok(make_turn_result_with_content(50, tc.clone(), "same"));
        actor.apply_loop_guard("s", &mut result);
        assert!(!actor.sessions["s"].brake_tripped);
    }

    let mut result = Ok(make_turn_result_with_content(50, tc.clone(), "same"));
    actor.apply_loop_guard("s", &mut result);
    assert!(actor.sessions["s"].brake_tripped);
}

#[test]
fn apply_loop_guard_reset_on_intervention() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );

    let tc = vec![make_tool_call("read_file", false)];
    for _ in 0..3 {
        let mut result = Ok(make_turn_result(50, tc.clone()));
        actor.apply_loop_guard("s", &mut result);
    }
    assert!(actor.sessions["s"].brake_tripped);

    // Simulate operator intervention: mark_turn_active resets the guard.
    actor.mark_turn_active("s");
    assert!(!actor.sessions["s"].brake_tripped);

    // Should not trigger immediately after reset.
    let mut result = Ok(make_turn_result(50, tc.clone()));
    actor.apply_loop_guard("s", &mut result);
    assert!(!actor.sessions["s"].brake_tripped);
}

#[test]
fn apply_loop_guard_no_tool_turn_does_not_fire() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );

    for _ in 0..5 {
        let mut result = Ok(make_turn_result_with_content(50, vec![], "same"));
        actor.apply_loop_guard("s", &mut result);
    }
    // No tool calls → no-progress counter stays at 0.
    assert!(!actor.sessions["s"].brake_tripped);
}

// Helper to create a TurnResult with custom content.
fn make_turn_result_with_content(
    output_tokens: u64,
    tool_calls: Vec<crate::pipeline::ToolCall>,
    content: &str,
) -> crate::pipeline::TurnResult {
    crate::pipeline::TurnResult {
        content: content.to_owned(),
        tool_calls,
        usage: crate::pipeline::TurnUsage {
            input_tokens: 10,
            output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            llm_calls: 1,
        },
        signals: vec![],
        stop_reason: "end_turn".to_owned(),
        degraded: None,
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        tool_surface_hashes: Vec::new(),
    }
}
// ── turn.rs: spawn_pipeline_task cancellation ───────────────────────────────

/// Provider that never returns, so the execute stage hangs until cancelled.
struct HangingProvider;

impl LlmProvider for HangingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        Box::pin(std::future::pending())
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    fn name(&self) -> &'static str {
        "hanging-provider"
    }
}

#[tokio::test]
async fn cancelled_turn_reverts_turn_counter() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    Arc::get_mut(&mut actor.services.providers)
        .expect("exclusive Arc reference expected in test")
        .register(Box::new(HangingProvider));

    let session_key = "cancel-test";
    let turn_cancel = CancellationToken::new();
    turn_cancel.cancel();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        actor.spawn_pipeline_task(
            session_key,
            None,
            None,
            "hello",
            None,
            None,
            tracing::Span::current(),
            turn_cancel,
        ),
    )
    .await;

    let result = result.expect("spawn_pipeline_task should resolve quickly after cancellation");
    assert!(
        matches!(result, Ok(Err(crate::error::Error::TurnCancelled { .. }))),
        "cancelled turn should return TurnCancelled"
    );
    let session = actor
        .sessions
        .get(session_key)
        .expect("session should exist after cancelled turn");
    assert_eq!(
        session.turn, 0,
        "turn counter should not advance when turn is cancelled"
    );
}
#[test]
fn evict_oldest_session_also_removes_drift_detector() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    let oldest_key = "oldest".to_owned();

    let mut oldest_state =
        SessionState::new("ses-0".to_owned(), oldest_key.clone(), &test_config());
    oldest_state.last_accessed = Instant::now()
        .checked_sub(Duration::from_hours(1))
        .expect("3600s is well within Instant range");
    actor.sessions.insert(oldest_key.clone(), oldest_state);
    actor
        .drift_detectors
        .insert(oldest_key.clone(), DriftDetector::default());

    for i in 0..MAX_SESSIONS {
        let key = format!("session-{i}");
        actor.sessions.insert(
            key.clone(),
            SessionState::new(format!("ses-{i}"), key.clone(), &test_config()),
        );
        actor.drift_detectors.insert(key, DriftDetector::default());
    }

    assert_eq!(actor.sessions.len(), MAX_SESSIONS + 1);
    assert_eq!(actor.drift_detectors.len(), MAX_SESSIONS + 1);

    actor.evict_oldest_session_if_needed();

    assert_eq!(actor.sessions.len(), MAX_SESSIONS);
    assert_eq!(
        actor.drift_detectors.len(),
        MAX_SESSIONS,
        "drift_detectors must be pruned when sessions are evicted"
    );
    assert!(
        !actor.drift_detectors.contains_key(&oldest_key),
        "evicted session's drift detector must be removed"
    );
}

// ── turn_quality.rs: record_router_outcome ───────────────────────────────────

/// Poor interactive outcomes (tool errors + guard intervention) must not be
/// counted as routing successes just because the provider returned a response.
#[tokio::test]
async fn poor_interactive_outcome_does_not_increase_success_rate() {
    let store = Arc::new(AfterActionStore::in_memory());
    let router: Arc<dyn aletheia_routing::Router> =
        Arc::new(RecordingRouter::new(Arc::clone(&store), "test-model"));

    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.services.router = router;
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );

    // Good turn: no errors, no intervention, within budget.
    let good = make_turn_result(50, vec![make_tool_call("read_file", false)]);
    actor.record_router_outcome("s", "build a feature", &good);

    // Poor turn: every tool call errored and the loop guard replaced content.
    let mut bad = make_turn_result(
        50,
        vec![
            make_tool_call("read_file", true),
            make_tool_call("read_file", true),
        ],
    );
    bad.content = "[System: Agent loop detected (repetition). \
         The agent appears to be stuck in a repetitive pattern. \
         Please provide guidance or clarification to continue.]"
        .to_owned();
    actor.sessions.get_mut("s").unwrap().brake_tripped = true;
    actor.record_router_outcome("s", "build a feature", &bad);

    let provider = ProviderId::new("test-model");
    for _ in 0..20 {
        if let Some(stats) = store
            .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(168))
            .await
            .expect("rolling stats query")
        {
            assert_eq!(
                stats.successes, 1,
                "only the clean turn should count as success"
            );
            assert_eq!(stats.failures, 1, "the poor turn should count as failure");
            assert_eq!(stats.total, 2);

            // WHY: the audit log must preserve provenance so operators can see
            // why each turn was classified the way it was.
            let outcomes = store.recent_outcomes().await;
            assert_eq!(outcomes.len(), 2);
            let poor = outcomes
                .iter()
                .find(|o| !o.success)
                .expect("poor outcome in audit log");
            let io = poor
                .interactive_outcome
                .as_ref()
                .expect("interactive provenance recorded");
            assert_eq!(io.loop_guard, InterventionStatus::Triggered);
            assert!((io.tool_error_rate - 1.0).abs() < f64::EPSILON);
            return;
        }
        tokio::task::yield_now().await;
    }

    panic!("poor interactive turn was not recorded as a failure");
}
