//! Cron scheduling, metrics, output, and persistence tests.

use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;
use std::sync::{Arc, Mutex};

use tracing::Instrument;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::{self, MakeWriter};
use tracing_subscriber::layer::{Context, SubscriberExt};

use super::super::*;
use super::make_echo_task;

#[derive(Clone)]
struct SharedWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl SharedWriter {
    fn new(buffer: Arc<Mutex<Vec<u8>>>) -> Self {
        Self { buffer }
    }
}

struct SharedWriteGuard {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriteGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriteGuard {
            buffer: Arc::clone(&self.buffer),
        }
    }
}

impl IoWrite for SharedWriteGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.buffer.lock().expect("log buffer lock");
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct TraceCaptureLayer {
    events: Arc<Mutex<Vec<String>>>,
}

impl TraceCaptureLayer {
    fn text(&self) -> String {
        self.events.lock().expect("trace capture lock").join("\n")
    }
}

impl<S> Layer<S> for TraceCaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldCapture::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("trace capture lock")
            .push(visitor.fields);
    }
}

#[derive(Default)]
struct FieldCapture {
    fields: String,
}

impl Visit for FieldCapture {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        writeln!(&mut self.fields, "{}={value:?}", field.name())
            .expect("writing to String cannot fail");
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        writeln!(&mut self.fields, "{}={value}", field.name())
            .expect("writing to String cannot fail");
    }
}

fn buffer_text(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
    let bytes = buffer.lock().expect("log buffer lock").clone();
    String::from_utf8(bytes).expect("captured logs should be utf-8")
}

#[test]
fn missed_cron_catchup_fires_on_startup() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "hourly-task".to_owned(),
        name: "Hourly task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Cron("0 0 * * * *".to_owned()),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        catch_up: true,
        ..TaskDef::default()
    };
    runner.register(task);

    let three_hours_ago = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(3))
        .expect("timestamp arithmetic should succeed");
    runner.set_last_run("hourly-task", three_hours_ago);

    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_hours(1))
            .expect("timestamp arithmetic should succeed"),
    );

    runner.check_missed_cron_catchup();

    let next = runner.tasks[0]
        .next_run
        .expect("next_run should be set after catch-up");
    let diff = next
        .since(jiff::Timestamp::now())
        .expect("duration since should succeed")
        .get_seconds()
        .abs();
    assert!(diff < 5, "catch-up should set next_run to ~now");
}

#[test]
fn missed_cron_catchup_skips_disabled_catch_up() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "no-catchup".to_owned(),
        name: "No catch-up".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Cron("0 0 * * * *".to_owned()),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        catch_up: false,
        ..TaskDef::default()
    };
    runner.register(task);

    let three_hours_ago = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(3))
        .expect("timestamp arithmetic should succeed");
    runner.set_last_run("no-catchup", three_hours_ago);

    let future_run = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_hours(1))
        .expect("timestamp arithmetic should succeed");
    runner.tasks[0].next_run = Some(future_run);

    runner.check_missed_cron_catchup();

    assert_eq!(
        runner.tasks[0]
            .next_run
            .expect("next_run should remain unchanged"),
        future_run
    );
}

#[test]
fn task_metrics_on_success() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("metrics-task"));

    runner.record_task_completion("metrics-task", Duration::from_millis(42), 0);

    let statuses = runner.status();
    assert_eq!(statuses[0].run_count, 1);
    assert_eq!(statuses[0].consecutive_failures, 0);
}

#[test]
fn task_metrics_on_failure() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("metrics-fail"));

    runner.record_task_failure("metrics-fail", "boom");

    let statuses = runner.status();
    assert_eq!(statuses[0].consecutive_failures, 1);
    assert_eq!(statuses[0].run_count, 0);
}

#[tokio::test]
async fn in_flight_reported_in_status() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("inflight-task"));

    let handle = tokio::spawn(
        async {
            // kanon:ignore TESTING/sleep-in-test reason = "simulates an in-flight task; handle is aborted before sleep elapses"
            tokio::time::sleep(Duration::from_mins(1)).await;
            Ok(ExecutionResult {
                outcome: TaskOutcome::Success,
                errors: 0,
                output: None,
            })
        }
        .instrument(tracing::info_span!("test_inflight_task")),
    );
    runner.in_flight.insert(
        "inflight-task".to_owned(),
        InFlightTask {
            handle,
            cancel: CancellationToken::new(),
            started_at: Instant::now(),
            timeout: Duration::from_mins(5),
            warned: false,
        },
    );

    let statuses = runner.status();
    assert!(statuses[0].in_flight);

    if let Some(task) = runner.in_flight.remove("inflight-task") {
        task.handle.abort();
    }
}

#[tokio::test]
async fn unsuccessful_in_flight_result_records_failure_status_and_metrics() {
    use koina::metrics::MetricsRegistry;

    let registry = MetricsRegistry::new();
    registry.with_registry(crate::metrics::register);

    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    let task = TaskDef {
        id: "unsuccessful-task".to_owned(),
        name: "_test_unsuccessful_inflight".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::Command("echo ignored".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    let handle = tokio::spawn(async {
        Ok(ExecutionResult {
            outcome: TaskOutcome::Failed,
            errors: 0,
            output: Some("probe detected violation".to_owned()),
        })
    });
    runner.in_flight.insert(
        "unsuccessful-task".to_owned(),
        InFlightTask {
            handle,
            cancel: CancellationToken::new(),
            started_at: Instant::now(),
            timeout: Duration::from_mins(5),
            warned: false,
        },
    );

    tokio::task::yield_now().await;
    runner.check_in_flight().await;

    assert!(
        !runner.in_flight.contains_key("unsuccessful-task"),
        "finished task should be removed from in_flight"
    );
    let statuses = runner.status();
    assert_eq!(
        statuses[0].run_count, 0,
        "failed result should not increment run_count"
    );
    assert_eq!(
        statuses[0].consecutive_failures, 1,
        "failed result should increment consecutive_failures"
    );
    let last_error = statuses[0]
        .last_error
        .as_deref()
        .expect("failed result output should become summarized last_error");
    assert!(
        last_error.contains("output summary"),
        "failed result output should be summarized, got: {last_error}"
    );
    assert!(
        !last_error.contains("probe detected violation"),
        "raw failed output should not be persisted, got: {last_error}"
    );

    let mut buf = String::new();
    registry
        .encode(&mut buf)
        .expect("encoding metrics into String is infallible");
    let expected = r#"aletheia_cron_executions_total{task_name="_test_unsuccessful_inflight",status="error"} 1"#;
    assert!(
        buf.contains(expected),
        "metrics should record the result as a failure; got: {buf}"
    );
}

#[tokio::test]
async fn unsuccessful_in_flight_result_without_output_uses_fallback_error() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("unsuccessful-no-output"));

    let handle = tokio::spawn(async {
        Ok(ExecutionResult {
            outcome: TaskOutcome::Failed,
            errors: 0,
            output: None,
        })
    });
    runner.in_flight.insert(
        "unsuccessful-no-output".to_owned(),
        InFlightTask {
            handle,
            cancel: CancellationToken::new(),
            started_at: Instant::now(),
            timeout: Duration::from_mins(5),
            warned: false,
        },
    );

    tokio::task::yield_now().await;
    runner.check_in_flight().await;

    let statuses = runner.status();
    assert_eq!(
        statuses[0].last_error,
        Some("task reported failure".to_owned()),
        "missing output should use concise fallback last_error"
    );
    assert_eq!(statuses[0].consecutive_failures, 1);
}

#[tokio::test]
async fn repeated_unsuccessful_in_flight_results_accumulate_failures() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("repeated-failure"));

    for n in 1..=2 {
        let output = format!("failure number {n}");
        let handle = tokio::spawn(async move {
            Ok(ExecutionResult {
                outcome: TaskOutcome::Failed,
                errors: 0,
                output: Some(output),
            })
        });
        runner.in_flight.insert(
            "repeated-failure".to_owned(),
            InFlightTask {
                handle,
                cancel: CancellationToken::new(),
                started_at: Instant::now(),
                timeout: Duration::from_mins(5),
                warned: false,
            },
        );

        tokio::task::yield_now().await;
        runner.check_in_flight().await;
    }

    let statuses = runner.status();
    assert_eq!(
        statuses[0].consecutive_failures, 2,
        "two failed results should yield two consecutive failures"
    );
    let last_error = statuses[0]
        .last_error
        .as_deref()
        .expect("last failure should be summarized");
    assert!(
        last_error.contains("output summary"),
        "last_error should summarize the most recent failure output, got: {last_error}"
    );
    assert!(
        !last_error.contains("failure number 2"),
        "raw failed output should not be persisted, got: {last_error}"
    );
}

/// IDs returned by `status()` for the core maintenance tasks must match the IDs
/// accepted by `aletheia maintenance run <id>`.
#[test]
fn maintenance_status_ids_accepted_by_run() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.trace_rotation.enabled = true;
    config.drift_detection.enabled = true;
    config.db_monitoring.enabled = true;
    config.instance_backup.enabled = true;
    config.prompt_audit.enabled = true;

    let capabilities = crate::maintenance::registry::MaintenanceRuntimeCapabilities::default();
    let manual_scheduled_ids: Vec<&str> = crate::maintenance::maintenance_task_registry()
        .iter()
        .filter(|definition| definition.manual_run().is_some())
        .filter(|definition| definition.scheduled_task(&config, capabilities).is_some())
        .map(crate::maintenance::MaintenanceTaskDefinition::id)
        .collect();

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let status_ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();

    for id in manual_scheduled_ids {
        assert!(
            status_ids.contains(&id),
            "run-accepted id '{id}' not found in status output — IDs are mismatched"
        );
    }
}

// -- Brief output mode tests --

#[test]
fn truncate_output_short_passes_through() {
    let short = "line 1\nline 2\nline 3";
    assert_eq!(
        truncate_output(short, None, None),
        short,
        "short output should pass through unchanged"
    );
}

#[test]
fn truncate_output_long_shows_head_and_tail() {
    let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
    let long = lines.join("\n");
    let truncated = truncate_output(&long, None, None);

    assert!(
        truncated.contains("line 1"),
        "head should include first line"
    );
    assert!(
        truncated.contains("line 5"),
        "head should include line 5 (BRIEF_HEAD_LINES=5)"
    );
    assert!(
        truncated.contains("lines omitted"),
        "should contain omission marker"
    );
    assert!(
        truncated.contains("line 20"),
        "tail should include last line"
    );
    assert!(
        truncated.contains("line 18"),
        "tail should include line 18 (BRIEF_TAIL_LINES=3)"
    );
    assert!(
        !truncated.contains("line 10"),
        "middle lines should be omitted"
    );
}

#[test]
fn summary_output_mode_omits_excerpt() {
    let raw = "api_key=synthetic-sensitive-token-4721\npath=/tmp/acme.corp/private";
    let safe = safe_output_for_mode(
        raw,
        DaemonOutputMode::Summary,
        &taxis::config::DaemonBehaviorConfig::default(),
    );

    assert!(safe.contains("output summary"));
    assert!(safe.contains("sha256="));
    assert!(
        !safe.contains("synthetic-sensitive-token-4721"),
        "summary output must not contain secrets: {safe}"
    );
    assert!(
        !safe.contains("/tmp/acme.corp/private"),
        "summary output must not contain private paths: {safe}"
    );
    assert!(
        !safe.contains("redacted_excerpt"),
        "summary output must not include excerpts: {safe}"
    );
}

#[tokio::test]
async fn task_output_boundary_sanitizes_subscribers_and_persisted_state() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    let token = CancellationToken::new();
    let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
    let mut runner = TaskRunner::new("test-nous", token)
        .with_output_mode(DaemonOutputMode::Brief)
        .with_state_store(store);
    runner.register(make_echo_task("sensitive-output"));

    let secret = "synthetic-sensitive-token-4721";
    let private_path = "/tmp/acme.corp/private/session.txt";
    let output = format!("probe failed\napi_key={secret}\nprivate_path={private_path}\n");
    let handle = tokio::spawn(async move {
        Ok(ExecutionResult {
            outcome: TaskOutcome::Failed,
            errors: 0,
            output: Some(output),
        })
    });
    runner.in_flight.insert(
        "sensitive-output".to_owned(),
        InFlightTask {
            handle,
            cancel: CancellationToken::new(),
            started_at: Instant::now(),
            timeout: Duration::from_mins(5),
            warned: false,
        },
    );

    let console_buffer = Arc::new(Mutex::new(Vec::new()));
    let file_json_buffer = Arc::new(Mutex::new(Vec::new()));
    let trace_capture = TraceCaptureLayer::default();
    let subscriber = tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_target(false)
                .with_writer(SharedWriter::new(Arc::clone(&console_buffer))),
        )
        .with(
            fmt::layer()
                .json()
                .with_ansi(false)
                .with_target(true)
                .with_writer(SharedWriter::new(Arc::clone(&file_json_buffer))),
        )
        .with(trace_capture.clone());

    let guard = tracing::subscriber::set_default(subscriber);
    tokio::task::yield_now().await;
    runner.check_in_flight().await;
    drop(guard);

    let console = buffer_text(&console_buffer);
    let file_json = buffer_text(&file_json_buffer);
    let trace_ingest = trace_capture.text();
    let states = runner
        .state_store
        .as_ref()
        .expect("runner should keep attached store")
        .load_all()
        .expect("load persisted state");
    let persisted = states
        .iter()
        .find(|state| state.task_id == "sensitive-output")
        .and_then(|state| state.last_error.as_deref())
        .expect("failed task should persist safe last_error")
        .to_owned();

    for (surface, text) in [
        ("console", console),
        ("file_json", file_json),
        ("trace_ingest", trace_ingest),
        ("persisted", persisted),
    ] {
        assert!(
            !text.contains(secret),
            "{surface} leaked synthetic API key: {text}"
        );
        assert!(
            !text.contains(private_path),
            "{surface} leaked private path: {text}"
        );
        assert!(
            text.contains("***"),
            "{surface} should contain a redaction marker: {text}"
        );
        assert!(
            text.contains("[PATH REDACTED]"),
            "{surface} should contain a path redaction marker: {text}"
        );
        assert!(
            text.contains("sha256="),
            "{surface} should include stable output digest metadata: {text}"
        );
    }
}

#[test]
fn with_output_mode_sets_mode() {
    let token = CancellationToken::new();
    let runner = TaskRunner::new("test-nous", token).with_output_mode(DaemonOutputMode::Brief);
    assert_eq!(runner.output_mode, DaemonOutputMode::Brief);
}

#[test]
fn default_output_mode_is_summary() {
    let token = CancellationToken::new();
    let runner = TaskRunner::new("test-nous", token);
    assert_eq!(runner.output_mode, DaemonOutputMode::Summary);
}

// -- Jitter integration test in runner --

#[test]
fn register_task_with_jitter_shifts_next_run() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "jittered-task".to_owned(),
        name: "Jittered".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_hours(1)),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        jitter: Some(jiff::SignedDuration::from_secs(600)),
        ..TaskDef::default()
    };

    let before = jiff::Timestamp::now();
    runner.register(task);

    let statuses = runner.status();
    let next_run: jiff::Timestamp = statuses[0]
        .next_run
        .as_ref()
        .expect("should have next_run")
        .parse()
        .expect("valid timestamp");

    // NOTE: with jitter, next_run should be >= now + interval (base) since
    // jitter is additive and non-negative.
    assert!(
        next_run > before,
        "jittered next_run should be in the future"
    );
}

// -- State persistence integration test --

#[test]
fn with_state_store_persists_across_restarts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    // First runner: register, complete a task, persist
    {
        let token = CancellationToken::new();
        let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
        let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
        runner.register(make_echo_task("persist-task"));
        runner.record_task_completion("persist-task", Duration::from_millis(10), 0);

        let statuses = runner.status();
        assert_eq!(statuses[0].run_count, 1);
    }

    // Second runner: restore state from same DB
    {
        let token = CancellationToken::new();
        let store = crate::state::TaskStateStore::open(&db_path).expect("reopen store");
        let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
        runner.register(make_echo_task("persist-task"));
        runner.restore_state();

        let statuses = runner.status();
        assert_eq!(
            statuses[0].run_count, 1,
            "run_count should be restored from store"
        );
    }
}

// -- #7206: hydration semantics for a disabled task --

/// The runner's own 3-consecutive-failure auto-disable must not stay
/// disabled forever purely because it survives a restart: hydration re-arms
/// it for exactly one retry, scheduled promptly, while leaving
/// `consecutive_failures` at its persisted value so a further failure
/// re-disables immediately (not after three fresh strikes). #5130's intent
/// -- a restart must not silently forget an auto-disable happened -- is
/// still honored: the failure count and `last_error` survive; only `enabled`
/// and `disable_cause` change, and only because a real hydration is giving
/// the task a chance to prove the underlying condition is fixed.
#[test]
fn restore_state_rearms_auto_disabled_task_for_one_retry_and_clears_cause() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    // First runner: fail the task 3 times to trigger a real auto-disable
    // through the normal tracking path, not a hand-crafted TaskState.
    {
        let token = CancellationToken::new();
        let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
        let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
        runner.register(make_echo_task("flaky-task"));
        for _ in 0..3 {
            runner.record_task_failure("flaky-task", "connection refused");
        }

        let statuses = runner.status();
        assert!(
            !statuses[0].enabled,
            "task must be auto-disabled after 3 consecutive failures"
        );
        assert_eq!(
            statuses[0].disable_cause,
            Some(crate::state::DisableCause::AutoFailure)
        );
    }

    // Second runner (simulated restart): restore_state must re-arm the
    // auto-disabled task for one retry, not leave it disabled forever.
    {
        let token = CancellationToken::new();
        let store = crate::state::TaskStateStore::open(&db_path).expect("reopen store");
        let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
        runner.register(make_echo_task("flaky-task"));
        runner.restore_state();

        let statuses = runner.status();
        assert!(
            statuses[0].enabled,
            "auto-disabled task must be re-armed for one retry on hydration"
        );
        assert_eq!(
            statuses[0].disable_cause, None,
            "cause must clear while re-armed -- enabled:true with a stale cause reads as a contradiction"
        );
        assert_eq!(
            statuses[0].consecutive_failures, 3,
            "failure count must not reset: a further failure must re-disable after ONE \
             more strike, not three fresh ones"
        );

        let next_run: jiff::Timestamp = statuses[0]
            .next_run
            .as_deref()
            .expect("next_run must be set")
            .parse()
            .expect("next_run must be a valid timestamp");
        let now = jiff::Timestamp::now();
        assert!(
            next_run <= now,
            "retry must be scheduled promptly (at/before now), not deferred: {next_run}"
        );
        assert!(
            now.duration_since(next_run).as_nanos() < i128::from(5u32) * 1_000_000_000,
            "retry must be scheduled within a few seconds of hydration, not wait out the \
             task's normal cadence"
        );
    }
}

/// #5130's acceptance criterion -- "without re-enabling auto-disabled tasks
/// unless an explicit operator reset exists" -- must still hold exactly for
/// an operator-caused disable: hydration must never re-arm it. Only the
/// admin API's own `enable`/`retry` (or the `aletheia maintenance reset`
/// CLI) re-enables it.
#[test]
fn restore_state_never_rearms_operator_disabled_task() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    {
        let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
        store
            .save(&crate::state::TaskState {
                task_id: "quiet-task".to_owned(),
                enabled: Some(false),
                disable_cause: Some(crate::state::DisableCause::Operator),
                consecutive_failures: 0,
                ..crate::state::TaskState::default()
            })
            .expect("seed operator-disabled task state");
    }

    let token = CancellationToken::new();
    let store = crate::state::TaskStateStore::open(&db_path).expect("reopen store");
    let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
    runner.register(make_echo_task("quiet-task"));
    runner.restore_state();

    let statuses = runner.status();
    assert!(
        !statuses[0].enabled,
        "an operator's explicit disable must survive hydration unchanged"
    );
    assert_eq!(
        statuses[0].disable_cause,
        Some(crate::state::DisableCause::Operator)
    );
}

/// A disabled record written before `disable_cause` existed carries no cause
/// at all. Every such record predates the admin API this field was added
/// alongside, so it was -- by construction -- always an auto-disable; it
/// must get the same one-retry treatment as an explicit `AutoFailure`, not
/// be mistaken for (or defensively treated as) an operator's decision.
#[test]
fn restore_state_treats_legacy_disabled_record_as_auto_failure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    {
        let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
        store
            .save(&crate::state::TaskState {
                task_id: "legacy-task".to_owned(),
                enabled: Some(false),
                disable_cause: None,
                consecutive_failures: 3,
                ..crate::state::TaskState::default()
            })
            .expect("seed legacy disabled task state");
    }

    let token = CancellationToken::new();
    let store = crate::state::TaskStateStore::open(&db_path).expect("reopen store");
    let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
    runner.register(make_echo_task("legacy-task"));
    runner.restore_state();

    let statuses = runner.status();
    assert!(
        statuses[0].enabled,
        "a legacy disabled record (no cause on file) must be re-armed like AutoFailure"
    );
}

// -- #7206: live external-write sync (no restart) --

/// A periodic [`TaskRunner::sync_external_state`] sync -- unlike a
/// hydration -- must NOT re-arm a still-disabled auto-failure task on its
/// own: only a hydration (restart) gets the automatic one-retry treatment.
/// Otherwise auto-disable would never actually stick between restarts.
#[test]
fn sync_external_state_does_not_rearm_auto_disabled_task_on_its_own() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    let token = CancellationToken::new();
    let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
    let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
    runner.register(make_echo_task("flaky-task"));
    for _ in 0..3 {
        runner.record_task_failure("flaky-task", "connection refused");
    }
    assert!(!runner.status()[0].enabled, "precondition: auto-disabled");

    // A sync while nothing external touched the store must be a no-op.
    runner.sync_external_state();

    let statuses = runner.status();
    assert!(
        !statuses[0].enabled,
        "an untouched auto-disabled task must stay disabled between restarts"
    );
    assert_eq!(
        statuses[0].disable_cause,
        Some(crate::state::DisableCause::AutoFailure)
    );
}

/// The one channel that crosses the process boundary between the pylon
/// daemon-task admin API (or the `aletheia maintenance reset` CLI) and this
/// live runner is the shared [`crate::state::TaskStateStore`]. A write made
/// there -- simulated here without going through pylon -- must take effect
/// live, promptly, without requiring a restart (#7206).
#[test]
fn sync_external_state_picks_up_an_external_enable_and_schedules_immediately() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    let token = CancellationToken::new();
    let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
    let mut runner = TaskRunner::new("test-nous", token.clone()).with_state_store(store.clone());
    runner.register(make_echo_task("flaky-task"));
    for _ in 0..3 {
        runner.record_task_failure("flaky-task", "connection refused");
    }
    assert!(!runner.status()[0].enabled, "precondition: auto-disabled");

    // Simulate an external write to the SAME store -- exactly what the
    // pylon `enable`/`retry` handlers and `aletheia maintenance reset` do.
    let mut external_write = store.load_all().expect("load persisted state")[0].clone();
    external_write.enabled = Some(true);
    external_write.disable_cause = None;
    store
        .save(&external_write)
        .expect("persist external enable");

    runner.sync_external_state();

    let statuses = runner.status();
    assert!(
        statuses[0].enabled,
        "an external enable write must take effect live, without a restart"
    );
    let next_run: jiff::Timestamp = statuses[0]
        .next_run
        .as_deref()
        .expect("next_run must be set")
        .parse()
        .expect("next_run must be a valid timestamp");
    assert!(
        next_run <= jiff::Timestamp::now(),
        "an external enable must schedule an immediate attempt, not wait out the cadence"
    );
}

// -- Self-prompt integration tests --
