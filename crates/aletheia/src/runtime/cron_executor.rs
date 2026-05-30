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

use energeia::cron::{CronLockStore, CronScheduler, CronTask, OverlapPolicy};
use energeia::orchestrator::Orchestrator;
use energeia::prompt;
use energeia::types::DispatchSpec;
use taxis::config::{CronTaskConfig, DispatchSpecConfig};
use taxis::oikos::Oikos;

use crate::error::Result;

const LOCK_DB_DIR: &str = "cron-locks.fjall";
const LOCK_PARTITION: &str = "cron_locks";

/// Start the cron executor: open the lock store, build `CronTask`s from the
/// enabled config entries, and spawn the scheduler loop on `task_tracker`.
///
/// Returns `Ok(())` and logs at info level when no enabled tasks are present
/// — the daemon should remain healthy without any cron configuration.
pub(super) fn start(
    tasks: &[CronTaskConfig],
    orchestrator: Arc<Orchestrator>,
    oikos: &Oikos,
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
    for cfg in &enabled {
        match build_cron_task(cfg) {
            Ok(task) => cron_tasks.push(task),
            Err(e) => warn!(
                task = %cfg.name,
                schedule = %cfg.schedule,
                error = %e,
                "dispatch cron executor: invalid task — skipping"
            ),
        }
    }
    if cron_tasks.is_empty() {
        warn!(
            configured = tasks.len(),
            "dispatch cron executor: all enabled tasks failed to parse — scheduler not started"
        );
        return Ok(());
    }

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
    let lock_store = Arc::new(
        CronLockStore::open(Arc::new(fjall_db.db))
            .whatever_context("failed to open cron lock store")?,
    );

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
                    async move {
                        fire_task(task, orchestrator, theke).await;
                    }
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
    spec.dag_ref = cfg.dag_ref.clone();
    spec.max_parallel = cfg.max_parallel;
    spec.max_turns = cfg.max_turns;
    spec
}

async fn fire_task(task: CronTask, orchestrator: Arc<Orchestrator>, theke: std::path::PathBuf) {
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
            return;
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
        return;
    }

    match orchestrator
        .dispatch(task.dispatch_spec.clone(), &selected)
        .await
    {
        Ok(_result) => info!(
            task = %task.name,
            project = %task.dispatch_spec.project,
            "cron dispatch completed"
        ),
        Err(e) => warn!(
            task = %task.name,
            project = %task.dispatch_spec.project,
            error = %e,
            "cron dispatch failed"
        ),
    }
}
