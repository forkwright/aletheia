//! `aletheia maintenance`: instance maintenance task management.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;
use snafu::prelude::*;

use oikonomos::maintenance::{
    AutoDreamConfig, DbMonitor, DbMonitoringConfig, DerivedRulesConfig, DriftDetectionConfig,
    DriftDetector, FjallBackupConfig, InstanceBackupConfig, MaintenanceConfig,
    MaintenanceTaskDefinition, ManualMaintenanceTask, PromptAuditRetentionConfig,
    PromptAuditRotator, ProposeRulesConfig, TraceRotationConfig, TraceRotator,
    maintenance_task_by_id, manual_maintenance_task_ids, manual_maintenance_tasks,
};
use oikonomos::prosoche_audit::{ProsocheAuditRunner, ProsocheState};
use oikonomos::runner::TaskRunner;
use taxis::loader::load_config;
use taxis::oikos::Oikos;
use tokio_util::sync::CancellationToken;

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Show status of all maintenance tasks
    Status {
        /// Output as JSON instead of human-readable table
        #[arg(long)]
        json: bool,
    },
    /// Run a specific maintenance task immediately
    Run {
        /// Task name from the daemon maintenance registry, or "all" to run every manual task.
        ///
        /// Use `maintenance status` to inspect scheduled daemon tasks.
        task: String,
        /// List individual files (drift-detection only)
        #[arg(long)]
        verbose: bool,
    },
    /// Clear the persisted failure/backoff/disable state for a task so it
    /// becomes eligible to run again. (#5130)
    Reset {
        /// Task ID whose persisted state should be reset.
        task: String,
    },
}

pub(crate) async fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let config = load_config(&oikos).whatever_context("failed to load config")?;
    let maint = build_config(&oikos, &config.maintenance, &config.prompt_audit);

    match action {
        Action::Status { json } => {
            let token = CancellationToken::new();
            let mut runner = TaskRunner::new("system", token).with_maintenance(maint);
            runner.register_maintenance_tasks();

            // WHY(#5131): the daemon persists task state to disk. The CLI runs
            // in a separate process with a fresh in-memory runner, so without
            // restoring the persisted state, `status` always reported zero runs
            // and never reflected backoff or auto-disable. Load it if present.
            let state_root = oikos
                .data()
                .join("daemon-task-state")
                .join("system");
            if state_root.exists()
                && let Ok(store) = oikonomos::state::TaskStateStore::open(&state_root)
            {
                runner = runner.with_state_store(store);
                runner.restore_state();
            }

            let statuses = runner.status();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&statuses)
                        .whatever_context("failed to serialize status")?
                );
            } else {
                let name_w = statuses
                    .iter()
                    .map(|s| s.name.len())
                    .max()
                    .unwrap_or(4)
                    .max("Task".len());
                let runs_w = statuses
                    .iter()
                    .map(|s| s.run_count.to_string().len())
                    .max()
                    .unwrap_or(1)
                    .max("Runs".len());
                println!(
                    "{:<name_w$} {:<8} {:<runs_w$} Last Run",
                    "Task", "Enabled", "Runs"
                );
                println!("{}", "-".repeat(name_w + 1 + 8 + 1 + runs_w + 1 + 8));
                for s in &statuses {
                    let last = s.last_run.as_deref().unwrap_or("never");
                    let enabled = if s.enabled { "yes" } else { "no" };
                    println!(
                        "{:<name_w$} {:<8} {:<runs_w$} {}",
                        s.name, enabled, s.run_count, last
                    );
                }
            }
        }
        Action::Run { task, verbose } => {
            let tasks: Vec<&'static MaintenanceTaskDefinition> = if task == "all" {
                manual_maintenance_tasks().collect()
            } else {
                vec![manual_task_definition(&task)?]
            };
            for definition in tasks {
                run_task(definition, &maint, verbose).await?;
            }
        }
        Action::Reset { task } => reset_task_state(&oikos, &task)?,
    }
    Ok(())
}

/// Clear the persisted failure/backoff/disable state for a single task. (#5130)
///
/// Re-enables the task, zeroes the consecutive-failure counter, and clears the
/// backoff deadline and last error so the daemon will schedule it again on its
/// next start.
fn reset_task_state(oikos: &Oikos, task_id: &str) -> Result<()> {
    let state_root = oikos.data().join("daemon-task-state").join("system");
    if !state_root.exists() {
        whatever!(
            "no persisted task state found at {}",
            state_root.display()
        );
    }

    let store = oikonomos::state::TaskStateStore::open(&state_root)
        .whatever_context("failed to open task-state store")?;
    let states = store
        .load_all()
        .whatever_context("failed to load task state")?;

    let Some(mut state) = states.into_iter().find(|s| s.task_id == task_id) else {
        whatever!("no persisted state for task '{task_id}'");
    };

    state.enabled = Some(true);
    state.consecutive_failures = 0;
    state.backoff_until_ts = None;
    state.last_error = None;
    state.schema_version = oikonomos::state::TASK_STATE_SCHEMA_VERSION;

    store
        .save(&state)
        .whatever_context("failed to persist reset task state")?;

    println!("Reset persisted state for task '{task_id}' (re-enabled, backoff cleared).");
    Ok(())
}

fn manual_task_definition(name: &str) -> Result<&'static MaintenanceTaskDefinition> {
    if let Some(definition) = maintenance_task_by_id(name)
        && definition.manual_run().is_some()
    {
        return Ok(definition);
    }

    let valid = manual_maintenance_task_ids().join(", ");
    whatever!("unknown task: {name}. Valid: {valid}, all")
}

/// Execute a single maintenance task by name.
async fn run_task(
    definition: &MaintenanceTaskDefinition,
    maint: &MaintenanceConfig,
    verbose: bool,
) -> Result<()> {
    let Some(manual_task) = definition.manual_run() else {
        let valid = manual_maintenance_task_ids().join(", ");
        whatever!("unknown task: {}. Valid: {valid}, all", definition.id())
    };

    match manual_task {
        ManualMaintenanceTask::TraceRotation => {
            let report = TraceRotator::new(maint.trace_rotation.clone())
                .rotate()
                .whatever_context("trace rotation failed")?;
            println!(
                "trace-rotation: {} rotated, {} pruned, {} bytes freed",
                report.files_rotated, report.files_pruned, report.bytes_freed
            );
        }
        ManualMaintenanceTask::DriftDetection => {
            let report = DriftDetector::new(maint.drift_detection.clone())
                .check()
                .whatever_context("drift detection failed")?;
            let missing = report.missing_files.len();
            let extra = report.extra_files.len();
            if missing == 0 && extra == 0 {
                println!("drift-detection: clean");
            } else if verbose {
                println!("drift-detection: {missing} missing, {extra} extra");
                for path in &report.missing_files {
                    println!("  missing: {}", path.display());
                }
                for path in &report.extra_files {
                    println!("  extra:   {}", path.display());
                }
            } else {
                println!(
                    "drift-detection: {missing} missing, {extra} extra  \
                     (use --verbose to list files)"
                );
            }
        }
        ManualMaintenanceTask::DbMonitor => {
            let report = DbMonitor::new(maint.db_monitoring.clone())
                .check()
                .whatever_context("db monitor failed")?;
            for db in &report.databases {
                println!(
                    "db-monitor: {} {}MB ({})",
                    db.name,
                    db.size_bytes / (1024 * 1024),
                    db.status
                );
            }
        }
        ManualMaintenanceTask::FjallBackup => {
            let manager =
                oikonomos::maintenance::InstanceBackup::new(maint.instance_backup.clone());
            let report = manager
                .create_backup()
                .whatever_context("whole-instance backup failed")?;
            match report.backup_path {
                Some(path) => println!(
                    "fjall-backup: {} files copied ({} bytes) to {}, {} old backups pruned",
                    report.files_copied,
                    report.bytes_copied,
                    path.display(),
                    report.backups_pruned,
                ),
                None => println!("fjall-backup: skipped (source directory not found)"),
            }
        }
        ManualMaintenanceTask::PromptAuditRotation => {
            let report = PromptAuditRotator::new(maint.prompt_audit.clone())
                .prune()
                .whatever_context("prompt audit rotation failed")?;
            println!(
                "prompt-audit-rotation: {} files pruned, {} retained, {} malformed skipped, {} fallback-pruned, {} bytes freed",
                report.files_pruned,
                report.files_retained,
                report.malformed_files_skipped,
                report.fallback_files_pruned,
                report.bytes_freed
            );
        }
        ManualMaintenanceTask::NousSelfAudit => run_self_audit(),
        ManualMaintenanceTask::ProsocheSelfAudit => run_prosoche_self_audit(maint).await,
        _ => whatever!(
            "manual task {} is not supported by this CLI",
            definition.id()
        ),
    }
    Ok(())
}

fn run_self_audit() {
    use nous::self_audit::{AuditTrigger, CheckContext, SelfAuditor};
    let mut auditor = SelfAuditor::new();
    auditor.register_defaults();
    let ctx = CheckContext {
        nous_id: String::from("system"),
        ..Default::default()
    };
    let report = auditor.run_audit(&ctx, AuditTrigger::Manual);
    for r in &report.results {
        println!(
            "  {}: {} (score: {:.2})",
            r.check_name, r.result.status, r.result.score,
        );
        if r.result.status != nous::self_audit::CheckStatus::Pass {
            println!("    evidence: {}", r.result.evidence);
        }
    }
}

async fn run_prosoche_self_audit(maint: &MaintenanceConfig) {
    let runner = ProsocheAuditRunner::default_checks(&maint.prosoche_audit_dir);
    let state = ProsocheState {
        nous_id: String::from("system"),
        checked_at: jiff::Timestamp::now().to_string(),
        ..ProsocheState::default()
    };
    let (report, persist_result) = runner.run_audit(&state).await;
    println!(
        "prosoche-self-audit: {} findings across {} checks",
        report.findings.len(),
        report.check_summary.len()
    );
    if let Err(e) = persist_result {
        eprintln!("prosoche-self-audit: warning: report persist failed: {e}");
    }
}

/// Build a `MaintenanceConfig` from the oikos layout and config settings.
///
/// Called from both the maintenance subcommand and the server startup path.
pub(crate) fn build_config(
    oikos: &Oikos,
    settings: &taxis::config::MaintenanceSettings,
    prompt_audit: &taxis::config::PromptAuditSettings,
) -> MaintenanceConfig {
    MaintenanceConfig {
        after_action_store: Some(Arc::new(aletheia_routing::AfterActionStore::new(
            oikos.logs().join("after-actions"),
        ))),
        trace_rotation: TraceRotationConfig {
            enabled: settings.trace_rotation.enabled,
            trace_dir: oikos.traces(),
            archive_dir: oikos.trace_archive(),
            max_age_days: settings.trace_rotation.max_age_days,
            max_total_size_mb: settings.trace_rotation.max_total_size_mb,
            compress: settings.trace_rotation.compress,
            max_archives: settings.trace_rotation.max_archives,
        },
        drift_detection: DriftDetectionConfig {
            enabled: settings.drift_detection.enabled,
            instance_root: oikos.root().to_path_buf(),
            example_root: std::path::PathBuf::from("instance.example"),
            alert_on_missing: settings.drift_detection.alert_on_missing,
            ignore_patterns: settings.drift_detection.ignore_patterns.clone(),
            optional_patterns: settings.drift_detection.optional_patterns.clone(),
        },
        db_monitoring: DbMonitoringConfig {
            enabled: settings.db_monitoring.enabled,
            data_dir: oikos.data(),
            warn_threshold_mb: settings.db_monitoring.warn_threshold_mb,
            alert_threshold_mb: settings.db_monitoring.alert_threshold_mb,
        },
        retention: oikonomos::maintenance::RetentionConfig {
            enabled: settings.retention.enabled,
        },
        knowledge_maintenance: oikonomos::maintenance::KnowledgeMaintenanceConfig {
            enabled: settings.knowledge_maintenance_enabled,
            auto_dream: AutoDreamConfig::default(),
            derived_rules: DerivedRulesConfig::default(),
            serendipity: oikonomos::maintenance::SerendipityMaintenanceConfig {
                enabled: settings.knowledge_maintenance_serendipity.enabled,
                cadence: settings.knowledge_maintenance_serendipity.cadence.clone(),
            },
        },
        fjall_backup: FjallBackupConfig {
            enabled: settings.backup.enabled,
            source_dir: oikos.knowledge_db(),
            backup_dir: oikos.backups().join("fjall"),
            interval_hours: settings.backup.backup_interval_hours,
            retention_count: settings.backup.backup_retention_count,
        },
        instance_backup: InstanceBackupConfig {
            enabled: settings.backup.enabled,
            instance_root: oikos.root().to_path_buf(),
            backup_dir: oikos.backups().join("instance"),
            interval_hours: settings.backup.backup_interval_hours,
            retention_count: settings.backup.backup_retention_count,
            additional_workspaces: Vec::new(),
        },
        backup_metrics: None,
        prosoche_audit_dir: oikos.data().join("prosoche-audits"),
        propose_rules: ProposeRulesConfig::default(),
        prompt_audit: PromptAuditRetentionConfig {
            enabled: prompt_audit.enabled,
            log_dir: prompt_audit
                .log_dir
                .clone()
                .unwrap_or_else(|| oikos.logs().join("prompt-audit")),
            retention_days: prompt_audit.retention_days,
        },
        cron: oikonomos::cron::CronConfig {
            evolution: oikonomos::cron::CronEvolutionConfig {
                enabled: settings.cron_tasks.evolution.enabled,
                interval: std::time::Duration::from_secs(
                    settings.cron_tasks.evolution.interval_secs,
                ),
            },
            reflection: oikonomos::cron::CronReflectionConfig {
                enabled: settings.cron_tasks.reflection.enabled,
                interval: std::time::Duration::from_secs(
                    settings.cron_tasks.reflection.interval_secs,
                ),
            },
            graph_cleanup: oikonomos::cron::CronGraphCleanupConfig {
                enabled: settings.cron_tasks.graph_cleanup.enabled,
                interval: std::time::Duration::from_secs(
                    settings.cron_tasks.graph_cleanup.interval_secs,
                ),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use oikonomos::maintenance::{maintenance_task_by_id, manual_maintenance_task_ids};

    #[test]
    fn all_expansion_comes_from_registry_manual_tasks() {
        let ids = manual_maintenance_task_ids();
        assert!(!ids.is_empty(), "manual task registry must not be empty");

        let unique: BTreeSet<_> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "manual task IDs must be unique");

        for id in ids {
            let Some(definition) = maintenance_task_by_id(id) else {
                panic!("manual id '{id}' resolves");
            };
            assert!(
                definition.manual_run().is_some(),
                "manual task '{id}' must carry a manual run handler"
            );
        }
    }
}
