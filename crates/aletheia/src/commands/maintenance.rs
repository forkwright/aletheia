//! `aletheia maintenance`: instance maintenance task management.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;
use snafu::prelude::*;

use oikonomos::maintenance::{
    AutoDreamConfig, DbMonitor, DbMonitoringConfig, DerivedRulesConfig, DriftDetectionConfig,
    DriftDetector, FjallBackupConfig, InstanceBackupConfig, KnowledgeMaintenanceConfig,
    KnowledgeMaintenanceExecutor, MaintenanceConfig, MaintenanceConfigSection,
    MaintenanceRuntimeCapabilities, MaintenanceTaskDefinition, MaintenanceTaskImplementationStatus,
    MaintenanceTaskOwner, ManualMaintenanceTask, PromptAuditRetentionConfig, PromptAuditRotator,
    ProposeRulesConfig, TraceRotationConfig, TraceRotator, maintenance_task_by_id,
    maintenance_task_registry, manual_maintenance_task_ids, manual_maintenance_tasks,
};
use oikonomos::prosoche_audit::{ProsocheAuditRunner, ProsocheState};
use oikonomos::runner::TaskRunner;
use oikonomos::schedule::TaskStatus;
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
}

pub(crate) async fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let config = load_config(&oikos).whatever_context("failed to load config")?;
    let maint = build_config(&oikos, &config.maintenance, &config.prompt_audit);
    let knowledge_executor = build_knowledge_executor(&oikos);

    match action {
        Action::Status { json } => {
            let token = CancellationToken::new();
            let mut runner = TaskRunner::new("system", token)
                .with_maintenance(maint.clone())
                .with_knowledge_maintenance_opt(knowledge_executor.clone());
            runner.register_maintenance_tasks();
            let statuses = merge_unavailable_tasks(runner.status(), &maint, &runner);
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
                    "{:<name_w$} {:<12} {:<runs_w$} Last Run",
                    "Task", "Status", "Runs"
                );
                println!("{}", "-".repeat(name_w + 1 + 12 + 1 + runs_w + 1 + 8));
                for s in &statuses {
                    let last = s.last_run.as_deref().unwrap_or("never");
                    let status = if !s.available {
                        "unavailable"
                    } else if s.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    };
                    println!(
                        "{:<name_w$} {:<12} {:<runs_w$} {}",
                        s.name, status, s.run_count, last
                    );
                    if let Some(reason) = &s.reason {
                        println!("  ({reason})");
                    }
                }
            }
        }
        Action::Run { task, verbose } => {
            let tasks: Vec<&'static MaintenanceTaskDefinition> = if task == "all" {
                manual_maintenance_tasks().collect()
            } else {
                vec![manual_task_definition(&task, knowledge_executor.is_some())?]
            };
            for definition in tasks {
                if task == "all"
                    && definition.owner() == MaintenanceTaskOwner::KnowledgeGraph
                    && knowledge_executor.is_none()
                {
                    println!(
                        "{}: skipped (no knowledge executor configured)",
                        definition.id()
                    );
                    continue;
                }
                run_task(definition, &maint, knowledge_executor.as_ref(), verbose).await?;
            }
        }
    }
    Ok(())
}

fn manual_task_definition(
    name: &str,
    has_knowledge_executor: bool,
) -> Result<&'static MaintenanceTaskDefinition> {
    let Some(definition) = maintenance_task_by_id(name) else {
        let valid = manual_maintenance_task_ids().join(", ");
        whatever!("unknown task: {name}. Valid: {valid}, all")
    };

    if definition.manual_run().is_some() {
        return Ok(definition);
    }

    // Documented knowledge tasks should return a structured reason instead of
    // the generic "unknown task" error.
    if definition.owner() == MaintenanceTaskOwner::KnowledgeGraph {
        match definition.implementation_status() {
            MaintenanceTaskImplementationStatus::Planned => {
                whatever!("{name}: not scheduled (task is planned but not implemented)")
            }
            MaintenanceTaskImplementationStatus::Implemented if !has_knowledge_executor => {
                whatever!("{name}: unavailable (no knowledge executor configured)")
            }
            MaintenanceTaskImplementationStatus::Implemented => {
                whatever!("{name}: not scheduled for manual run")
            }
            _ => whatever!("{name}: unavailable (unknown implementation status)"),
        }
    }

    let valid = manual_maintenance_task_ids().join(", ");
    whatever!("unknown task: {name}. Valid: {valid}, all")
}

/// Execute a single maintenance task by name.
async fn run_task(
    definition: &MaintenanceTaskDefinition,
    maint: &MaintenanceConfig,
    knowledge_executor: Option<&Arc<dyn KnowledgeMaintenanceExecutor>>,
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
        ManualMaintenanceTask::DecayRefresh
        | ManualMaintenanceTask::EntityDedup
        | ManualMaintenanceTask::GraphRecompute
        | ManualMaintenanceTask::SkillDecay
        | ManualMaintenanceTask::DerivedFactsMaterialize
        | ManualMaintenanceTask::SerendipityDiscovery => {
            run_knowledge_task(definition, knowledge_executor).await?;
        }
        _ => whatever!("{}: not scheduled for manual run", definition.id()),
    }
    Ok(())
}

async fn run_knowledge_task(
    definition: &MaintenanceTaskDefinition,
    knowledge_executor: Option<&Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<()> {
    let task_id = definition.id().to_owned();
    let Some(executor) = knowledge_executor else {
        whatever!("{task_id}: unavailable (no knowledge executor configured)")
    };

    let builtin = definition
        .builtin()
        .whatever_context("knowledge task has no builtin binding")?;
    let report = tokio::task::spawn_blocking({
        let executor = Arc::clone(executor);
        let task_id = task_id.clone();
        let nous_id = "system".to_owned();
        move || match builtin {
            oikonomos::schedule::BuiltinTask::DecayRefresh => {
                executor.refresh_decay_scores(&nous_id)
            }
            oikonomos::schedule::BuiltinTask::EntityDedup => {
                executor.deduplicate_entities(&nous_id)
            }
            oikonomos::schedule::BuiltinTask::GraphRecompute => {
                executor.recompute_graph_scores(&nous_id)
            }
            oikonomos::schedule::BuiltinTask::SkillDecay => executor.run_skill_decay(&nous_id),
            oikonomos::schedule::BuiltinTask::DerivedFactsMaterialize => {
                executor.materialize_derived_facts()
            }
            oikonomos::schedule::BuiltinTask::SerendipityDiscovery => {
                executor.discover_serendipitous_facts(&nous_id)
            }
            _ => Err(oikonomos::error::TaskFailedSnafu {
                task_id,
                reason: format!("{builtin:?} is not a manual knowledge maintenance task"),
            }
            .build()),
        }
    })
    .await
    .whatever_context("knowledge task panicked")?;

    let report = report.whatever_context("knowledge task failed")?;
    let outcome = report.outcome();
    match outcome {
        oikonomos::maintenance::MaintenanceOutcome::Success => {
            println!(
                "{}: {} processed, {} modified in {}ms",
                definition.id(),
                report.items_processed,
                report.items_modified,
                report.duration_ms
            );
        }
        oikonomos::maintenance::MaintenanceOutcome::Degraded => {
            println!(
                "{}: degraded — {} processed, {} modified, {} non-fatal errors in {}ms",
                definition.id(),
                report.items_processed,
                report.items_modified,
                report.errors,
                report.duration_ms
            );
        }
        oikonomos::maintenance::MaintenanceOutcome::Failure => {
            whatever!(
                "{}: failed — {} processed, {} modified in {}ms",
                definition.id(),
                report.items_processed,
                report.items_modified,
                report.duration_ms
            )
        }
    }
    if let Some(detail) = &report.detail {
        println!("  {detail}");
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
    let report = runner.run_audit(&state).await;
    println!(
        "prosoche-self-audit: {} findings across {} checks",
        report.findings.len(),
        report.check_summary.len()
    );
}

#[cfg(feature = "recall")]
fn build_knowledge_executor(oikos: &Oikos) -> Option<Arc<dyn KnowledgeMaintenanceExecutor>> {
    use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

    let kb_path = oikos.knowledge_db();
    if !kb_path.exists() {
        return None;
    }
    let store = KnowledgeStore::open_fjall(&kb_path, KnowledgeConfig::default()).ok()?;
    Some(Arc::new(
        crate::knowledge_maintenance::KnowledgeMaintenanceAdapter::new(store),
    ))
}

#[cfg(not(feature = "recall"))]
fn build_knowledge_executor(_oikos: &Oikos) -> Option<Arc<dyn KnowledgeMaintenanceExecutor>> {
    None
}

/// Merge registered task statuses with registry entries that could not be
/// scheduled because a required executor is unavailable.
fn merge_unavailable_tasks(
    mut statuses: Vec<TaskStatus>,
    maint: &MaintenanceConfig,
    runner: &TaskRunner,
) -> Vec<TaskStatus> {
    use oikonomos::maintenance::SkippedMaintenanceWarning;

    let capabilities = MaintenanceRuntimeCapabilities {
        has_retention_executor: runner.has_retention_executor(),
        has_knowledge_executor: runner.has_knowledge_executor(),
        has_bridge: runner.has_bridge(),
    };

    let mut unavailable: Vec<TaskStatus> = Vec::new();
    for definition in maintenance_task_registry() {
        if definition.manual_run().is_none() {
            continue;
        }
        if statuses.iter().any(|s| s.id == definition.id()) {
            continue;
        }

        let reason = definition
            .skipped_warning(maint, capabilities)
            .map(|SkippedMaintenanceWarning { reason, .. }| reason.to_owned())
            .or_else(|| match definition.implementation_status() {
                MaintenanceTaskImplementationStatus::Planned => {
                    Some("task is planned but not implemented".to_owned())
                }
                _ => None,
            })
            .or_else(|| {
                if definition.owner() == MaintenanceTaskOwner::KnowledgeGraph
                    && !capabilities.has_knowledge_executor
                {
                    Some("no knowledge executor configured".to_owned())
                } else {
                    None
                }
            })
            .or_else(|| {
                if definition.config_section()
                    == Some(MaintenanceConfigSection::KnowledgeMaintenance)
                    && !maint.knowledge_maintenance.enabled
                {
                    Some("knowledge maintenance is disabled".to_owned())
                } else {
                    None
                }
            });

        unavailable.push(TaskStatus {
            id: definition.id().to_owned(),
            name: definition.name().to_owned(),
            enabled: false,
            next_run: None,
            last_run: None,
            run_count: 0,
            consecutive_failures: 0,
            in_flight: false,
            last_error: None,
            last_errors: 0,
            available: reason.is_none(),
            reason,
        });
    }

    statuses.append(&mut unavailable);
    statuses
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
        knowledge_maintenance: KnowledgeMaintenanceConfig {
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
