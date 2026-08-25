#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

// ── background.rs: reap_background_tasks ─────────────────────────────────────

#[tokio::test]
async fn reap_background_tasks_joins_completed_tasks() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor.runtime.background_tasks.spawn(async { /* no-op */ });

    tokio::task::yield_now().await;

    actor.reap_background_tasks();

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "completed tasks should be reaped"
    );
}

#[tokio::test]
async fn reap_background_tasks_records_background_panic() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor
        .runtime
        .background_tasks
        .spawn(async { panic!("background test panic") });

    // WHY: multiple yields let the spawned task panic and be collected.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }

    actor.reap_background_tasks();

    assert_eq!(
        actor.runtime.background_panic_count, 1,
        "panic in background task should increment background_panic_count"
    );
    assert_eq!(
        actor.runtime.background_failure.total_count, 1,
        "panic in background task should increment background_failure_total_count"
    );
    assert_eq!(
        actor.runtime.background_failure.timestamps.len(),
        1,
        "panic in background task should record a failure timestamp"
    );
    assert_eq!(
        actor.runtime.background_failure.latest_kind.as_deref(),
        Some("panic"),
        "latest background failure kind should be panic"
    );
    assert!(
        actor
            .runtime
            .background_failure
            .latest_message
            .as_deref()
            .is_some_and(|m: &str| m.contains("panicked")),
        "panics captured by JoinSet should carry a panic message"
    );
    assert_eq!(
        actor.channel.status,
        NousLifecycle::Idle,
        "background panic must not enter pipeline degraded mode"
    );

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    actor.handle_status(tx);
    let status = rx.try_recv().expect("status should be ready");
    assert_eq!(status.background_failure_total_count, 1);
    assert_eq!(status.background_failure_recent_count, 1);
    assert!(!status.background_health_degraded);
}

#[tokio::test]
async fn reap_background_tasks_records_cancelled_task_as_error() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let abort_handle = actor
        .runtime
        .background_tasks
        .spawn(std::future::pending::<()>());
    abort_handle.abort();

    tokio::task::yield_now().await;

    actor.reap_background_tasks();

    assert_eq!(
        actor.runtime.background_panic_count, 0,
        "cancelled task should not increment background_panic_count"
    );
    assert_eq!(
        actor.runtime.background_failure.total_count, 1,
        "cancelled task should increment background_failure_total_count"
    );
    assert_eq!(
        actor.runtime.background_failure.latest_kind.as_deref(),
        Some("error"),
        "latest background failure kind should be error for cancelled task"
    );
    assert!(
        actor
            .runtime
            .background_failure
            .latest_message
            .as_deref()
            .is_some_and(|m: &str| m.contains("cancelled")),
        "cancelled task should carry a cancellation message"
    );

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    actor.handle_status(tx);
    let status = rx.try_recv().expect("status should be ready");
    assert_eq!(status.background_failure_total_count, 1);
    assert_eq!(status.background_failure_recent_count, 1);
    assert!(!status.background_health_degraded);
}

#[tokio::test]
async fn reap_background_tasks_noop_when_empty() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.runtime.background_tasks.len(), 0);

    actor.reap_background_tasks(); // must not panic

    assert_eq!(actor.runtime.background_panic_count, 0);
}

// ── background.rs: maybe_spawn_extraction ────────────────────────────────────

#[test]
fn maybe_spawn_extraction_skips_when_no_config() {
    let config = PipelineConfig {
        extraction: None,
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction("hello world", "response text", &[], "", false);

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when extraction config is absent"
    );
}

#[test]
fn maybe_spawn_extraction_skips_when_disabled() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: false,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction("hello world", "response", &[], "", false);

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when extraction is disabled"
    );
}

#[test]
fn maybe_spawn_extraction_skips_when_content_too_short() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1000, // very high threshold
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction("short", "response", &[], "", false);

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when content is below min_message_length"
    );
}

#[tokio::test]
async fn maybe_spawn_extraction_skips_when_task_limit_reached() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1, // accept any content
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    for _ in 0..MAX_SPAWNED_TASKS {
        actor
            .runtime
            .background_tasks
            .spawn(std::future::pending::<()>());
    }
    assert_eq!(actor.runtime.background_tasks.len(), MAX_SPAWNED_TASKS);

    actor.maybe_spawn_extraction(
        "long enough content here",
        "response text here",
        &[],
        "",
        false,
    );

    assert_eq!(
        actor.runtime.background_tasks.len(),
        MAX_SPAWNED_TASKS,
        "task should not be spawned when limit is reached"
    );
}

#[tokio::test]
async fn maybe_spawn_extraction_spawns_with_tool_calls_and_reasoning() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    let tool_calls = vec![crate::pipeline::ToolCall {
        id: "tc-1".to_owned(),
        name: "read_file".to_owned(),
        input: serde_json::json!({"path": "/tmp/test.txt"}),
        result: Some("file contents".to_owned()),
        is_error: false,
        duration_ms: 42,
        approval: None,
        receipt: None,
        outcome_detail: None,
    }];

    actor.maybe_spawn_extraction(
        "user message here",
        "assistant response here",
        &tool_calls,
        "I need to check the file first.",
        false,
    );

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "extraction task should be spawned with tool calls and reasoning"
    );
}

#[tokio::test]
async fn maybe_spawn_extraction_spawns_without_tool_calls_or_reasoning() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction(
        "user message here",
        "assistant response here",
        &[],
        "",
        false,
    );

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "extraction task should be spawned even without tool calls or reasoning"
    );
}

/// Regression test for #5367: a degraded turn carries no real model
/// completion, so it must never enter the extraction/memory corpus even when
/// every other admission condition (config enabled, content long enough) is
/// met. Same fixture as `maybe_spawn_extraction_spawns_without_tool_calls_or_reasoning`
/// (which proves the non-degraded control spawns), differing only in
/// `is_degraded` — so this fails if the gate is ever removed from this call
/// site.
#[tokio::test]
async fn maybe_spawn_extraction_skips_when_degraded() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction(
        "user message here",
        "assistant response here",
        &[],
        "",
        true,
    );

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "a degraded turn must never spawn extraction, even when config would otherwise admit it"
    );
}

// ── background.rs: maybe_spawn_skill_analysis ────────────────────────────────

#[test]
fn maybe_spawn_skill_analysis_noop_on_empty_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor.maybe_spawn_skill_analysis(&[], "my-session", false);

    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[test]
fn maybe_spawn_skill_analysis_processes_successful_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let calls = vec![make_tool_call("bash", false)];
    actor.maybe_spawn_skill_analysis(&calls, "my-session", false);
    // WHY: no task is spawned unless the candidate is promoted (first occurrence never promotes).
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[test]
fn maybe_spawn_skill_analysis_processes_errored_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let calls = vec![make_tool_call("bash", true)];
    // NOTE: error calls are recorded as ToolCallRecord::errored.
    actor.maybe_spawn_skill_analysis(&calls, "s", false);
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

/// A tool call sequence that clears every heuristic gate in
/// `episteme::skills::heuristics::score_sequence` (length >= 5, >= 3 distinct
/// tools, no debugging-spiral/single-file-edit/config-specific rejection) —
/// mirrors `good_seq()` in `episteme::skills::candidate::tests`, which the
/// upstream Rule-of-Three tests (`third_occurrence_returns_promoted`) confirm
/// promotes on the third distinct-session occurrence.
fn good_skill_sequence() -> Vec<crate::pipeline::ToolCall> {
    ["Grep", "Read", "Read", "Edit", "Bash", "Bash"]
        .into_iter()
        .map(|name| make_tool_call(name, false))
        .collect()
}

/// Control for `maybe_spawn_skill_analysis_skips_when_degraded`: proves the
/// fixture actually reaches Rule-of-Three promotion (and therefore a spawn)
/// when the turn is NOT degraded, so the companion test's zero-spawn result
/// is meaningful rather than incidental.
#[tokio::test]
async fn maybe_spawn_skill_analysis_promotes_and_spawns_when_not_degraded() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);
    let calls = good_skill_sequence();

    actor.maybe_spawn_skill_analysis(&calls, "s1", false);
    actor.maybe_spawn_skill_analysis(&calls, "s2", false);
    actor.maybe_spawn_skill_analysis(&calls, "s3", false);

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "the third distinct-session occurrence of a passing sequence should promote and spawn skill extraction"
    );
}

/// Regression test for #5367: three identical passing sequences would
/// promote and spawn on the third occurrence (see the control test above);
/// with every one of them marked degraded, none may reach the candidate
/// tracker at all, so recurrence never accumulates and nothing spawns. This
/// fails if the gate is ever removed from this call site.
#[tokio::test]
async fn maybe_spawn_skill_analysis_skips_when_degraded() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);
    let calls = good_skill_sequence();

    actor.maybe_spawn_skill_analysis(&calls, "s1", true);
    actor.maybe_spawn_skill_analysis(&calls, "s2", true);
    actor.maybe_spawn_skill_analysis(&calls, "s3", true);

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "a degraded turn's tool calls must never reach the candidate tracker, so three occurrences never promote"
    );
}

// ── background.rs: maybe_spawn_distillation ──────────────────────────────────

#[tokio::test]
async fn maybe_spawn_distillation_skips_when_flag_already_set() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor
        .runtime
        .distillation_in_progress
        .store(true, std::sync::atomic::Ordering::Release);

    actor.maybe_spawn_distillation("s", false).await;

    assert!(
        actor
            .runtime
            .distillation_in_progress
            .load(std::sync::atomic::Ordering::Acquire),
        "flag should remain set"
    );
    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when distillation is already in progress"
    );
}

#[tokio::test]
async fn maybe_spawn_distillation_clears_flag_when_no_session_store() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor.maybe_spawn_distillation("s", false).await;

    assert!(
        !actor
            .runtime
            .distillation_in_progress
            .load(std::sync::atomic::Ordering::Acquire),
        "flag should be cleared when no task was spawned"
    );
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[tokio::test]
async fn maybe_spawn_distillation_clears_flag_when_session_not_found() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    actor.stores.session_store = Some(Arc::new(tokio::sync::Mutex::new(store)));

    actor
        .maybe_spawn_distillation("nonexistent-session", false)
        .await;

    assert!(
        !actor
            .runtime
            .distillation_in_progress
            .load(std::sync::atomic::Ordering::Acquire),
        "flag must be cleared when session is not found"
    );
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

/// Build an actor whose session `"s"` (store id `"ses-1"`) has enough
/// messages to trip `should_trigger_distillation`'s "never distilled"
/// threshold (`AgentBehaviorDefaults::distillation_never_distilled_trigger`,
/// default 30 — see `crates/taxis/src/config/agents.rs`), with a provider
/// registered for `DistillTriggerConfig::default().model` so
/// `try_spawn_distillation` clears every gate up to the actual spawn.
fn make_distillation_ready_actor() -> (
    NousActor,
    mpsc::Sender<NousMessage>,
    tempfile::TempDir, // kept alive: drop would delete tempdir
) {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new("distilled").models(&[koina::defaults::DEFAULT_MODEL]),
    ));
    let (mut actor, tx, dir) =
        make_test_actor_with_providers(Arc::new(providers), PipelineConfig::default());

    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    store
        .create_session("ses-1", "test-agent", "s", None, None)
        .expect("create session");
    for i in 0..30_i64 {
        store
            .append_message(
                "ses-1",
                mneme::types::Role::User,
                &format!("turn {i}"),
                None,
                None,
                10,
            )
            .expect("append message");
    }
    actor.stores.session_store = Some(Arc::new(tokio::sync::Mutex::new(store)));
    actor.sessions.insert(
        "s".to_owned(),
        SessionState::new("ses-1".to_owned(), "s".to_owned(), &test_config()),
    );

    (actor, tx, dir)
}

/// Control for `maybe_spawn_distillation_skips_when_degraded`: proves the
/// fixture actually reaches a spawn when the turn is NOT degraded, so the
/// companion test's zero-spawn result is meaningful rather than incidental.
#[tokio::test]
async fn maybe_spawn_distillation_spawns_when_not_degraded() {
    let (mut actor, _tx, _dir) = make_distillation_ready_actor();

    actor.maybe_spawn_distillation("s", false).await;

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "a never-distilled session past the message-count threshold should spawn a distillation task"
    );
}

/// Regression test for #5367: same fixture as the control above, which
/// would otherwise spawn — a degraded turn must never trigger distillation
/// of the session it just produced. Fails if the gate is ever removed from
/// this call site.
#[tokio::test]
async fn maybe_spawn_distillation_skips_when_degraded() {
    let (mut actor, _tx, _dir) = make_distillation_ready_actor();

    actor.maybe_spawn_distillation("s", true).await;

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "a degraded turn must never spawn distillation, even when the session is otherwise eligible"
    );
    assert!(
        !actor
            .runtime
            .distillation_in_progress
            .load(std::sync::atomic::Ordering::Acquire),
        "the in-progress flag must never be touched for a degraded turn"
    );
}

// ── background.rs: maybe_run_auto_dream ──────────────────────────────────────

/// Build an actor with everything `maybe_run_auto_dream` needs to clear every
/// gate up to constructing the `DreamEngine`: an in-memory session store, an
/// in-memory knowledge store, and a provider registered for
/// `DistillTriggerConfig::default().model`. Mirrors
/// `make_distillation_ready_actor` above.
#[cfg(feature = "knowledge-store")]
fn make_auto_dream_ready_actor() -> (
    NousActor,
    mpsc::Sender<NousMessage>,
    tempfile::TempDir, // kept alive: drop would delete tempdir
) {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new("dreamed").models(&[koina::defaults::DEFAULT_MODEL]),
    ));
    let (mut actor, tx, dir) =
        make_test_actor_with_providers(Arc::new(providers), PipelineConfig::default());

    let session_store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    actor.stores.session_store = Some(Arc::new(tokio::sync::Mutex::new(session_store)));
    let knowledge_store =
        mneme::knowledge_store::KnowledgeStore::open_mem().expect("in-memory knowledge store");
    actor.stores.knowledge_store = Some(knowledge_store);

    (actor, tx, dir)
}

/// Control for `maybe_run_auto_dream_skips_when_degraded`: proves the fixture
/// actually reaches `DreamEngine` construction when the turn is NOT degraded,
/// so the companion test's untouched-engine result is meaningful rather than
/// incidental — same shape as `maybe_spawn_distillation_spawns_when_not_degraded`
/// above.
#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn maybe_run_auto_dream_builds_engine_when_not_degraded() {
    let (mut actor, _tx, _dir) = make_auto_dream_ready_actor();

    actor.maybe_run_auto_dream(false).await;

    assert!(
        actor.runtime.auto_dream_engine.is_some(),
        "a non-degraded turn with stores and a provider ready should build the dream engine"
    );
}

/// Regression test for #6752: same fixture as the control above, which would
/// otherwise build the engine — a degraded turn must never trigger dream
/// consolidation off the session it just produced. Unlike the other three
/// `is_degraded`-gated background paths, this one `.await`s inline instead of
/// spawning, so `background_tasks.len()` cannot observe it; `auto_dream_engine`
/// is the seam this pair asserts against instead. Fails if the `is_degraded`
/// check is ever removed from `maybe_run_auto_dream`.
#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn maybe_run_auto_dream_skips_when_degraded() {
    let (mut actor, _tx, _dir) = make_auto_dream_ready_actor();

    actor.maybe_run_auto_dream(true).await;

    assert!(
        actor.runtime.auto_dream_engine.is_none(),
        "a degraded turn must never build the dream engine, even when stores \
         and a provider are otherwise ready"
    );
}

// ── turn.rs: finalize_turn corpus side-effect gating (#5367) ────────────────

/// Control for `finalize_turn_never_spawns_extraction_for_a_degraded_turn`:
/// proves `finalize_turn`'s wiring reaches the extraction call site (and
/// threads a real, non-degraded `TurnResult`) so the companion test's
/// zero-spawn result is meaningful rather than incidental. Together the pair
/// exercises the actual call site in `finalize_turn`, not just the predicate
/// or the callee in isolation — it fails if `finalize_turn` ever stops
/// threading `turn_result.is_degraded()` through to `maybe_spawn_extraction`.
#[tokio::test]
async fn finalize_turn_spawns_extraction_for_a_healthy_turn() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    let result: crate::error::Result<crate::pipeline::TurnResult> =
        Ok(make_turn_result(50, vec![]));
    actor
        .finalize_turn(
            "no-such-session",
            "user content long enough to pass the extraction gate",
            &result,
            None,
        )
        .await;

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "finalize_turn must still reach the extraction call site for a healthy turn"
    );
}

/// Regression test for #5367: same fixture as the control above, calling the
/// real `finalize_turn` entry point (not the `maybe_spawn_extraction` callee
/// directly) with a synthesized degraded `TurnResult`. This is the test that
/// would have caught the original defect — the call site in `finalize_turn`
/// never checked `is_degraded` at all.
#[tokio::test]
async fn finalize_turn_never_spawns_extraction_for_a_degraded_turn() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    let result: crate::error::Result<crate::pipeline::TurnResult> = Ok(make_degraded_turn_result());
    actor
        .finalize_turn(
            "no-such-session",
            "user content long enough to pass the extraction gate",
            &result,
            None,
        )
        .await;

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "finalize_turn must never spawn extraction for a degraded turn"
    );
}

/// Regression test for #758/#916/#923: session ID divergence.
///
/// Verifies that when pylon creates a DB session and passes its ID to the
/// actor, the finalize stage persists messages under the SAME session ID
/// (not a newly generated one).
#[tokio::test]
async fn session_id_adoption_prevents_fk_divergence() {
    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    // WHY: SessionId requires UUID v4 format after security hardening (#1754)
    let db_session_id = "550e8400-e29b-41d4-a716-446655440000";

    // NOTE: Simulate pylon creating the session in the store
    store
        .create_session(
            db_session_id,
            "test-agent",
            "main",
            None,
            Some("test-model"),
        )
        .expect("create session");

    let store = Arc::new(tokio::sync::Mutex::new(store));
    let (handle, join, _dir) = spawn_test_actor_with_store(Arc::clone(&store));

    let result = handle
        .send_turn_with_session_id(
            "main",
            Some(db_session_id.to_owned()),
            "Hello",
            crate::handle::DEFAULT_SEND_TIMEOUT,
        )
        .await
        .expect("turn should succeed");
    assert_eq!(result.content, "Hello from actor!");

    let store_guard = store.lock().await;
    let history = store_guard
        .get_history(db_session_id, None)
        .expect("history");

    assert!(
        history.len() >= 2,
        "expected at least 2 messages under DB session ID, got {}",
        history.len()
    );

    // WHY: Verify no messages exist under a different session ID
    // (if divergence occurred, messages would be under a random ULID)
    let all_sessions = store_guard
        .list_sessions(Some("test-agent"))
        .expect("list sessions");
    assert_eq!(
        all_sessions.len(),
        1,
        "should have exactly 1 session, got {}",
        all_sessions.len()
    );
    assert_eq!(all_sessions[0].id, db_session_id);

    drop(store_guard);
    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

/// Regression test for #3103: prosoche daemon FK constraint failure.
///
/// Simulates the scenario where the daemon's "daemon:prosoche" session already
/// exists in the DB (from a previous cycle), but the actor has no in-memory
/// session for that key (e.g., after restart or LRU eviction). The daemon
/// bridge calls `send_turn` with `session_id: None`, so the actor generates a
/// new ULID — which diverges from the DB's canonical ID.
///
/// Without session-ID adoption, `find_or_create_session` returns the existing
/// DB session silently (ON CONFLICT DO NOTHING) while `finalize` calls
/// `append_message` with the actor's newly generated ID (no DB row) →
/// FOREIGN KEY constraint failure and silent data loss. The actor must adopt
/// the DB session ID returned by `find_or_create_session`.
#[tokio::test]
async fn prosoche_daemon_adopts_existing_db_session_id() {
    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    // WHY: SessionId requires UUID v4 format
    let existing_db_id = "660e8400-e29b-41d4-a716-446655440001";

    // NOTE: simulate an existing DB session for the "daemon:prosoche" key
    // (as would exist from a previous prosoche cycle).
    store
        .create_session(
            existing_db_id,
            "test-agent",
            "daemon:prosoche",
            None,
            Some("test-model"),
        )
        .expect("create pre-existing prosoche session");

    let store = Arc::new(tokio::sync::Mutex::new(store));
    // WHY: Actor has no in-memory session for "daemon:prosoche" — simulates
    // restart or eviction. The daemon bridge sends with session_id: None.
    let (handle, join, _dir) = spawn_test_actor_with_store(Arc::clone(&store));

    // NOTE: Daemon bridge calls send_turn (not send_turn_with_session_id),
    // so session_id is None — the actor must discover and adopt the DB ID.
    let result = handle
        .send_turn("daemon:prosoche", "Run your prosoche heartbeat check.")
        .await
        .expect("turn should succeed without FK constraint failure");
    assert_eq!(result.content, "Hello from actor!");

    let store_guard = store.lock().await;

    // WHY: Messages must be under the existing DB session ID, not a new ULID.
    let history = store_guard
        .get_history(existing_db_id, None)
        .expect("history under existing DB session ID");
    assert!(
        history.len() >= 2,
        "expected at least 2 messages under existing DB session ID, got {}",
        history.len()
    );

    // WHY: No orphan session should be created — only one session for
    // "daemon:prosoche" should exist.
    let all_sessions = store_guard
        .list_sessions(Some("test-agent"))
        .expect("list sessions");
    assert_eq!(
        all_sessions.len(),
        1,
        "should have exactly 1 session (no orphan), got {}",
        all_sessions.len()
    );
    assert_eq!(
        all_sessions[0].id, existing_db_id,
        "surviving session must be the original DB session ID"
    );

    drop(store_guard);
    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

// ── cross-nous panic boundary tests (#3606) ──────────────────────────────────

// ── mod.rs: skill access-count increment is a tracked background task (#5733) ─

/// Build a searchable skill fact for the in-memory knowledge store.
#[cfg(feature = "knowledge-store")]
fn make_skill_fact(id: &str, nous_id: &str, skill_name: &str) -> mneme::knowledge::Fact {
    use mneme::knowledge::{FactAccess, FactLifecycle, FactProvenance, FactTemporal};

    let content = serde_json::to_string(&mneme::skill::SkillContent {
        name: skill_name.to_owned(),
        description: format!("Skill: {skill_name}"),
        steps: vec!["step 1".to_owned()],
        tools_used: vec!["Read".to_owned()],
        domain_tags: vec![skill_name.to_owned()],
        origin: "seeded".to_owned(),
        triggers: vec![],
        always: false,
    })
    .expect("skill content serializes to JSON");

    let now = jiff::Timestamp::now();
    mneme::knowledge::Fact {
        id: mneme::id::FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        content,
        fact_type: "skill".to_owned(),
        temporal: FactTemporal {
            valid_from: now,
            valid_to: jiff::Timestamp::from_second(i64::MAX / 2).unwrap_or(now),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: mneme::knowledge::EpistemicTier::Verified,
            source_session_id: None,
            stability_hours: 2190.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    }
}

/// Seed an actor with an in-memory knowledge store holding one searchable skill.
#[cfg(feature = "knowledge-store")]
fn seed_skill_loader(actor: &mut NousActor) {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("in-memory store");
    let skill = make_skill_fact("sk-docker", "test-agent", "docker");
    store.insert_fact(&skill).expect("insert skill");
    actor.stores.skill_loader = Some(crate::skills::SkillLoader::new(store));
}

/// The access-count bump must be registered in `background_tasks`, not spawned
/// loose. Regression test for #5733: `resolve_skills` called `tokio::spawn` and
/// dropped the `JoinHandle`, so the task escaped shutdown cancellation, panic
/// accounting, and the `MAX_SPAWNED_TASKS` guard.
///
/// This asserts the `JoinSet` is what grew, which is false against the pre-fix
/// loose spawn and true once the task is admitted through the actor.
#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn skill_access_increment_is_registered_in_background_tasks() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    seed_skill_loader(&mut actor);

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "actor should start with no background tasks"
    );

    let sections = actor.resolve_skill_sections("docker").await;

    assert!(
        !sections.is_empty(),
        "seeded skill should resolve to at least one section"
    );
    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "the access-count increment must be tracked in the JoinSet, not spawned loose"
    );
}

/// The increment is subject to the same backpressure as every other background
/// task: at `MAX_SPAWNED_TASKS` it is skipped rather than spawned unbounded.
#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn skill_access_increment_respects_spawn_limit() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    seed_skill_loader(&mut actor);

    for _ in 0..MAX_SPAWNED_TASKS {
        actor
            .runtime
            .background_tasks
            .spawn(std::future::pending::<()>());
    }

    let sections = actor.resolve_skill_sections("docker").await;

    assert!(
        !sections.is_empty(),
        "skills must still resolve when the spawn limit is reached"
    );
    assert_eq!(
        actor.runtime.background_tasks.len(),
        MAX_SPAWNED_TASKS,
        "at the limit the increment is skipped, not spawned past the guard"
    );
}
