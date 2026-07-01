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

// -- Self-prompt integration tests --
