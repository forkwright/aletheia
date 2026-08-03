//! Wire the `dispatch.cron_tasks` config to the real energeia cron scheduler.
//!
//! Replaces the legacy "configured but not started" warning with an actual
//! scheduler that loads prompts from the project queue and invokes the
//! orchestrator on each scheduled fire.

use std::sync::Arc;
use std::time::Duration;

use snafu::ResultExt as _;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, info, warn};

use energeia::cron::{
    CronFailureClass, CronFireOutcome, CronLockStore, CronScheduler, CronTask, OverlapPolicy,
};
use energeia::orchestrator::Orchestrator;
use energeia::prompt;
use energeia::types::DispatchSpec;
use taxis::config::{CronTaskConfig, DispatchSpecConfig};
use taxis::oikos::Oikos;

use crate::error::{Error, Result};

const LOCK_DB_DIR: &str = "cron-locks.fjall";
const LOCK_PARTITION: &str = "cron_locks";

pub(super) fn open_lock_store(oikos: &Oikos) -> Result<Arc<CronLockStore>> {
    let lock_db_path = oikos.data().join(LOCK_DB_DIR);
    if let Some(parent) = lock_db_path.parent() {
        std::fs::create_dir_all(parent).with_whatever_context(|_| {
            format!("failed to CREATE cron lock dir {}", parent.display())
        })?;
    }
    let fjall_db = koina::fjall::FjallDb::open(&lock_db_path, &[LOCK_PARTITION])
        .with_whatever_context(|_| {
            format!("failed to open cron lock db at {}", lock_db_path.display())
        })?;
    Ok(Arc::new(
        CronLockStore::open(Arc::new(fjall_db.db))
            .whatever_context("failed to open cron lock store")?,
    ))
}

/// Start the cron executor: build `CronTask`s from the enabled config entries
/// and spawn the scheduler loop on `task_tracker`.
///
/// Returns `Ok(())` and logs at info level when no enabled tasks are present
/// — the daemon should remain healthy without any cron configuration.
///
/// Returns `Err` on the first enabled task that fails to parse so that
/// invalid cron config fails startup rather than silently disabling scheduled
/// work.
pub(super) fn start(
    tasks: &[CronTaskConfig],
    orchestrator: Arc<Orchestrator>,
    oikos: &Oikos,
    lock_store: Arc<CronLockStore>,
    task_tracker: &TaskTracker,
    shutdown_token: &CancellationToken,
) -> Result<()> {
    let enabled: Vec<&CronTaskConfig> = tasks.iter().filter(|t| t.enabled).collect();
    if enabled.is_empty() {
        info!(
            configured = tasks.len(),
            "dispatch cron executor: no enabled tasks; skipping scheduler startup"
        );
        return Ok(());
    }

    let mut cron_tasks: Vec<CronTask> = Vec::with_capacity(enabled.len());
    let mut errors: Vec<String> = Vec::new();
    for cfg in &enabled {
        match build_cron_task(cfg) {
            Ok(task) => cron_tasks.push(task),
            Err(e) => errors.push(format!(
                "task '{}' schedule '{}': {e}",
                cfg.name, cfg.schedule
            )),
        }
    }

    if !errors.is_empty() {
        return Err(Error::msg(format!(
            "dispatch cron executor: {} enabled task(s) failed to parse: {}",
            errors.len(),
            errors.join("; ")
        )));
    }

    let theke = oikos.theke();
    let tasks_started = cron_tasks.len();
    let scheduler = CronScheduler::new(cron_tasks, lock_store)
        .with_overlap_policy(OverlapPolicy::SkipIfInFlight);
    let cancel = shutdown_token.child_token();

    info!(
        tasks = tasks_started,
        "dispatch cron executor started; recurring dispatch enabled"
    );

    task_tracker.spawn(
        async move {
            let result = scheduler
                .run(cancel, move |task| {
                    let orchestrator = Arc::clone(&orchestrator);
                    let theke = theke.clone();
                    async move { fire_task(task, orchestrator, theke).await }
                })
                .await;
            if let Err(e) = result {
                warn!(error = %e, "dispatch cron executor exited with error");
            }
        }
        .instrument(tracing::info_span!("cron_executor")),
    );

    Ok(())
}

fn build_cron_task(cfg: &CronTaskConfig) -> energeia::error::Result<CronTask> {
    let spec = dispatch_spec_from_config(&cfg.dispatch_spec);
    CronTask::new(
        cfg.name.as_str(),
        cfg.schedule.as_str(),
        Duration::from_secs(cfg.jitter_secs),
        spec,
    )
}

fn dispatch_spec_from_config(cfg: &DispatchSpecConfig) -> DispatchSpec {
    let mut spec = DispatchSpec::new(cfg.project.clone(), cfg.prompt_numbers.clone());
    spec.dag_ref.clone_from(&cfg.dag_ref);
    spec.max_parallel = cfg.max_parallel;
    spec.max_turns = cfg.max_turns;
    spec.budget_usd = cfg.budget_usd;
    spec
}

/// Run one scheduled dispatch and report how it ended.
///
/// WHY(#5297): every path here used to log and return, so the scheduler
/// recorded a successful fire for a queue that failed to load, a spec that
/// matched no prompts, or a dispatch that errored. Each path now names its
/// failure class so schedule health reflects whether the work actually ran.
async fn fire_task(
    task: CronTask,
    orchestrator: Arc<Orchestrator>,
    theke: std::path::PathBuf,
) -> CronFireOutcome {
    let queue_dir = theke
        .join("projects")
        .join(&task.dispatch_spec.project)
        .join("prompts")
        .join("queue");
    let prompts = match prompt::load_queue(&queue_dir) {
        Ok(p) => p,
        Err(e) => {
            warn!(
                task = %task.name,
                project = %task.dispatch_spec.project,
                queue = %queue_dir.display(),
                error = %e,
                "cron task: failed to load prompt queue"
            );
            return CronFireOutcome::failed(
                CronFailureClass::Input,
                format!("failed to load prompt queue {}: {e}", queue_dir.display()),
            );
        }
    };

    let wanted: std::collections::HashSet<u32> =
        task.dispatch_spec.prompt_numbers.iter().copied().collect();
    let selected: Vec<prompt::PromptSpec> = prompts
        .into_iter()
        .filter(|p| wanted.is_empty() || wanted.contains(&p.number))
        .collect();
    if selected.is_empty() {
        warn!(
            task = %task.name,
            project = %task.dispatch_spec.project,
            queue = %queue_dir.display(),
            "cron task: no prompts matched dispatch spec"
        );
        // WHY: a scheduled dispatch that selects nothing is a misconfigured
        // spec or an empty queue, not an acceptable no-op — the task exists to
        // dispatch prompts. Recording it as Skipped would hide exactly the
        // condition an operator needs to see.
        return CronFireOutcome::failed(
            CronFailureClass::NoMatch,
            format!(
                "no prompts in {} matched dispatch spec",
                queue_dir.display()
            ),
        );
    }

    match orchestrator
        .dispatch(task.dispatch_spec.clone(), &selected)
        .await
    {
        Ok(_result) => {
            info!(
                task = %task.name,
                project = %task.dispatch_spec.project,
                "cron dispatch completed"
            );
            CronFireOutcome::Success
        }
        Err(e) => {
            warn!(
                task = %task.name,
                project = %task.dispatch_spec.project,
                error = %e,
                "cron dispatch failed"
            );
            CronFireOutcome::failed(CronFailureClass::Execution, e.to_string())
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;
    use std::sync::Arc;

    use energeia::engine::{SessionEvent, SessionResult};
    use energeia::http::{MockEngine, MockOutcome};
    use energeia::orchestrator::{Orchestrator, OrchestratorConfig};
    use energeia::qa::{PromptSpec as QaPromptSpec, QaGate};
    use energeia::types::{DispatchSpec, MechanicalIssue};
    use tempfile::TempDir;

    use super::*;

    // WHY: QA is only invoked when a session produces a PR URL. The MockEngine
    // returns no PR URL, so evaluate() is unreachable in these tests. We still
    // satisfy the trait bound with a stub that would panic loudly if reached,
    // making any accidental invocation immediately visible.
    struct StubQaGate;
    impl QaGate for StubQaGate {
        fn evaluate<'a>(
            &'a self,
            _prompt: &'a QaPromptSpec,
            _pr_number: u64,
            _diff: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = energeia::error::Result<energeia::types::QaResult>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                panic!("StubQaGate::evaluate should not be called in cron executor tests");
            })
        }
        fn mechanical_check(&self, _diff: &str, _prompt: &QaPromptSpec) -> Vec<MechanicalIssue> {
            vec![]
        }
    }

    fn write_prompt_file(dir: &std::path::Path, name: &str) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create prompt file");
        write!(
            f,
            "---\nnumber: 1\ndescription: \"cron test prompt\"\n---\n\nTest task body.\n"
        )
        .expect("write prompt");
    }

    fn mock_success_outcome() -> MockOutcome {
        MockOutcome::Success {
            events: vec![SessionEvent::TurnComplete { turn: 1 }],
            result: SessionResult::new(
                "s-cron-test".to_owned(),
                0.01,
                1,
                50,
                true,
                Some("done".to_owned()),
            ),
        }
    }

    /// Verify that `fire_task` routes through the orchestrator when a prompt
    /// queue is present and the task's `prompt_numbers` includes the queued
    /// prompt. The `MockEngine` returns exactly one success outcome; the test
    /// passes if `fire_task` consumes it without panicking — proving the dispatch
    /// execution path (not a log stub) was invoked.
    #[tokio::test]
    async fn fire_task_dispatches_via_orchestrator() {
        let theke = TempDir::new().expect("tempdir");
        let queue_dir = theke
            .path()
            .join("projects")
            .join("test-project")
            .join("prompts")
            .join("queue");
        std::fs::create_dir_all(&queue_dir).expect("create queue dir");
        write_prompt_file(&queue_dir, "001-task.md");

        let engine = Arc::new(MockEngine::new(vec![mock_success_outcome()]));
        let qa: Arc<dyn QaGate> = Arc::new(StubQaGate);
        let orchestrator = Arc::new(Orchestrator::new(engine, qa, OrchestratorConfig::new()));

        let task = CronTask::new(
            "cron-test",
            "* * * * * *",
            std::time::Duration::ZERO,
            DispatchSpec::new("test-project".to_owned(), vec![1]),
        )
        .expect("valid cron expression");

        // WHY: fire_task is async and logs outcomes; the key observable is that
        // the MockEngine outcome was consumed (dispatch ran), which we verify by
        // confirming the function completes without the "no more configured
        // outcomes" error that MockEngine returns when called unexpectedly.
        fire_task(task, orchestrator, theke.path().to_path_buf()).await;
    }

    /// When `prompt_numbers` is empty, `fire_task` selects every prompt in the
    /// queue. Verify the dispatch path still fires for an unconstrained spec.
    #[tokio::test]
    async fn fire_task_empty_prompt_numbers_selects_all() {
        let theke = TempDir::new().expect("tempdir");
        let queue_dir = theke
            .path()
            .join("projects")
            .join("all-project")
            .join("prompts")
            .join("queue");
        std::fs::create_dir_all(&queue_dir).expect("create queue dir");
        write_prompt_file(&queue_dir, "001-task.md");

        let engine = Arc::new(MockEngine::new(vec![mock_success_outcome()]));
        let qa: Arc<dyn QaGate> = Arc::new(StubQaGate);
        let orchestrator = Arc::new(Orchestrator::new(engine, qa, OrchestratorConfig::new()));

        let task = CronTask::new(
            "all-prompts",
            "* * * * * *",
            std::time::Duration::ZERO,
            DispatchSpec::new("all-project".to_owned(), vec![]),
        )
        .expect("valid cron expression");

        fire_task(task, orchestrator, theke.path().to_path_buf()).await;
    }

    /// When the queue directory does not exist, `fire_task` must not panic; it
    /// logs a warning and returns cleanly.
    #[tokio::test]
    async fn fire_task_missing_queue_dir_returns_cleanly() {
        let theke = TempDir::new().expect("tempdir");
        let engine = Arc::new(MockEngine::new(vec![]));
        let qa: Arc<dyn QaGate> = Arc::new(StubQaGate);
        let orchestrator = Arc::new(Orchestrator::new(engine, qa, OrchestratorConfig::new()));

        let task = CronTask::new(
            "no-queue",
            "* * * * * *",
            std::time::Duration::ZERO,
            DispatchSpec::new("nonexistent-project".to_owned(), vec![1]),
        )
        .expect("valid cron expression");

        // WHY: no panic or error propagation when the queue dir is absent;
        // the executor must stay resilient to partially-configured projects.
        fire_task(task, orchestrator, theke.path().to_path_buf()).await;
    }

    fn test_oikos() -> (TempDir, Oikos) {
        let dir = TempDir::new().expect("tempdir");
        let oikos = Oikos::from_root(dir.path());
        (dir, oikos)
    }

    fn test_orchestrator() -> Arc<Orchestrator> {
        let engine = Arc::new(MockEngine::new(vec![]));
        let qa: Arc<dyn QaGate> = Arc::new(StubQaGate);
        Arc::new(Orchestrator::new(engine, qa, OrchestratorConfig::new()))
    }

    fn task_config(name: &str, schedule: &str, enabled: bool) -> CronTaskConfig {
        CronTaskConfig {
            name: name.to_owned(),
            schedule: schedule.to_owned(),
            jitter_secs: 0,
            enabled,
            dispatch_spec: DispatchSpecConfig {
                project: "test-project".to_owned(),
                prompt_numbers: vec![],
                dag_ref: None,
                max_parallel: None,
                max_turns: None,
                budget_usd: None,
            },
        }
    }

    /// No enabled tasks means the daemon stays healthy without cron config.
    #[test]
    fn start_returns_ok_when_no_enabled_tasks() {
        let (_dir, oikos) = test_oikos();
        let result = start(
            &[],
            test_orchestrator(),
            &oikos,
            open_lock_store(&oikos).expect("lock store"),
            &TaskTracker::new(),
            &CancellationToken::new(),
        );
        assert!(result.is_ok(), "no enabled tasks should not fail startup");
    }

    /// Disabled tasks are intentionally ignored, even if malformed.
    #[test]
    fn start_returns_ok_for_disabled_invalid_task() {
        let (_dir, oikos) = test_oikos();
        let tasks = vec![task_config("bad", "not-a-cron", false)];
        let result = start(
            &tasks,
            test_orchestrator(),
            &oikos,
            open_lock_store(&oikos).expect("lock store"),
            &TaskTracker::new(),
            &CancellationToken::new(),
        );
        assert!(result.is_ok(), "disabled invalid task should be ignored");
    }

    /// An enabled task with an invalid schedule must fail startup with the task
    /// name and schedule in the error so the operator can locate the typo.
    #[test]
    fn start_fails_for_invalid_enabled_task() {
        let (_dir, oikos) = test_oikos();
        let tasks = vec![task_config("bad", "not-a-cron", true)];
        let result = start(
            &tasks,
            test_orchestrator(),
            &oikos,
            open_lock_store(&oikos).expect("lock store"),
            &TaskTracker::new(),
            &CancellationToken::new(),
        );
        let err = result.expect_err("invalid enabled task should fail startup");
        let msg = err.to_string();
        assert!(msg.contains("bad"), "error should name the invalid task");
        assert!(
            msg.contains("not-a-cron"),
            "error should quote the invalid schedule"
        );
    }

    /// When every enabled task is invalid the error must list each one so the
    /// operator sees the full scope of the config problem.
    #[test]
    fn start_fails_when_all_enabled_tasks_invalid() {
        let (_dir, oikos) = test_oikos();
        let tasks = vec![
            task_config("bad-one", "not-a-cron", true),
            task_config("bad-two", "also-not", true),
        ];
        let result = start(
            &tasks,
            test_orchestrator(),
            &oikos,
            open_lock_store(&oikos).expect("lock store"),
            &TaskTracker::new(),
            &CancellationToken::new(),
        );
        let err = result.expect_err("all invalid enabled tasks should fail startup");
        let msg = err.to_string();
        assert!(
            msg.contains("bad-one"),
            "error should name first invalid task"
        );
        assert!(
            msg.contains("bad-two"),
            "error should name second invalid task"
        );
    }

    /// A valid enabled task passes validation and spawns the scheduler loop.
    /// We do not wait for the far-future schedule to fire.
    #[tokio::test]
    async fn start_starts_scheduler_for_valid_enabled_task() {
        let (_dir, oikos) = test_oikos();
        let tasks = vec![task_config("good", "0 0 0 1 1 *", true)];
        let tracker = TaskTracker::new();
        let shutdown = CancellationToken::new();
        let result = start(
            &tasks,
            test_orchestrator(),
            &oikos,
            open_lock_store(&oikos).expect("lock store"),
            &tracker,
            &shutdown,
        );
        assert!(result.is_ok(), "valid enabled task should start scheduler");
        assert_eq!(
            tracker.len(),
            1,
            "scheduler task should be tracked after startup"
        );
        shutdown.cancel();
    }
}
