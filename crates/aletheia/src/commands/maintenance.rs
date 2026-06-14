//! `aletheia maintenance`: instance maintenance task management.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;
use snafu::prelude::*;

use oikonomos::maintenance::{
    AutoDreamConfig, DbMonitor, DbMonitoringConfig, DerivedRulesConfig, DriftDetectionConfig,
    DriftDetector, FjallBackupConfig, InstanceBackupConfig, KnowledgeMaintenanceConfig,
    KnowledgeMaintenanceExecutor, MaintenanceConfig, MaintenanceRuntimeCapabilities,
    MaintenanceTaskAvailability, MaintenanceTaskDefinition, ManualMaintenanceTask,
    PromptAuditRetentionConfig, PromptAuditRotator, ProposeRulesConfig, TraceRotationConfig,
    TraceRotator, maintenance_task_by_id, maintenance_task_registry, manual_maintenance_task_ids,
    manual_maintenance_tasks,
};
use oikonomos::prosoche_audit::{ProsocheAuditRunner, ProsocheState};
use oikonomos::runner::TaskRunner;
use taxis::config::AletheiaConfig;
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

    match action {
        Action::Status { json } => {
            let knowledge_executor = build_knowledge_executor(&oikos, &config)?;
            let statuses = collect_statuses(&maint, knowledge_executor.as_ref());
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&statuses)
                        .whatever_context("failed to serialize status")?
                );
            } else {
                print_status_table(&statuses);
            }
        }
        Action::Run { task, verbose } => {
            if task == "all" {
                let knowledge_executor = build_knowledge_executor(&oikos, &config)?;
                let capabilities = maintenance_capabilities(knowledge_executor.as_ref());
                let tasks: Vec<&'static MaintenanceTaskDefinition> = manual_maintenance_tasks()
                    .filter(|definition| {
                        matches!(
                            definition.manual_availability(capabilities),
                            MaintenanceTaskAvailability::Available
                        )
                    })
                    .collect();
                for definition in tasks {
                    run_task(definition, &maint, knowledge_executor.as_ref(), verbose).await?;
                }
            } else {
                let needs_executor = maintenance_task_by_id(&task)
                    .is_some_and(MaintenanceTaskDefinition::manual_run_requires_knowledge_executor);
                let knowledge_executor = if needs_executor {
                    build_knowledge_executor(&oikos, &config)?
                } else {
                    None
                };
                let capabilities = maintenance_capabilities(knowledge_executor.as_ref());
                let definition = resolve_task(&task, capabilities)?;
                run_task(definition, &maint, knowledge_executor.as_ref(), verbose).await?;
            }
        }
    }
    Ok(())
}

#[cfg(feature = "recall")]
fn build_knowledge_executor(
    oikos: &Oikos,
    config: &AletheiaConfig,
) -> Result<Option<Arc<dyn KnowledgeMaintenanceExecutor>>> {
    let store =
        crate::runtime::open_shared_knowledge_store(oikos, &config.embedding, &config.knowledge)
            .whatever_context("failed to open knowledge store for maintenance")?;
    let provider: Arc<dyn mneme::embedding::EmbeddingProvider> = Arc::new(
        mneme::embedding::DegradedEmbeddingProvider::new(config.embedding.dimension),
    );
    let tuning =
        crate::knowledge_maintenance::tuning_from_behavior(&config.agents.defaults.behavior);
    let executor = crate::knowledge_maintenance::KnowledgeMaintenanceAdapter::new(store)
        .with_embedding_provider(provider)
        .with_tuning(tuning);
    Ok(Some(Arc::new(executor)))
}

#[cfg(not(feature = "recall"))]
fn build_knowledge_executor(
    _oikos: &Oikos,
    _config: &AletheiaConfig,
) -> Result<Option<Arc<dyn KnowledgeMaintenanceExecutor>>> {
    Ok(None)
}

fn maintenance_capabilities(
    knowledge_executor: Option<&Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> MaintenanceRuntimeCapabilities {
    MaintenanceRuntimeCapabilities {
        has_retention_executor: false,
        has_knowledge_executor: knowledge_executor.is_some(),
        has_bridge: false,
    }
}

/// Status row emitted by `maintenance status`.
///
/// Includes enough structured information for both operator tables and JSON
/// consumers to tell whether a task is scheduled, why it is not scheduled, and
/// its last-run state.
#[derive(Debug, Clone, serde::Serialize)]
struct CliTaskStatus {
    id: String,
    name: String,
    scheduled: bool,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    next_run: Option<String>,
    last_run: Option<String>,
    run_count: u64,
    consecutive_failures: u32,
}

fn collect_statuses(
    maint: &MaintenanceConfig,
    knowledge_executor: Option<&Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Vec<CliTaskStatus> {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("system", token).with_maintenance(maint.clone());
    if let Some(executor) = knowledge_executor {
        runner = runner.with_knowledge_maintenance(Arc::clone(executor));
    }
    runner.register_maintenance_tasks();

    let runner_statuses = runner.status();
    let registered_by_id: HashMap<&str, &oikonomos::schedule::TaskStatus> = runner_statuses
        .iter()
        .map(|status| (status.id.as_str(), status))
        .collect();

    let capabilities = maintenance_capabilities(knowledge_executor);

    maintenance_task_registry()
        .iter()
        .map(|definition| {
            if let Some(status) = registered_by_id.get(definition.id()) {
                CliTaskStatus {
                    id: status.id.clone(),
                    name: status.name.clone(),
                    scheduled: true,
                    enabled: status.enabled,
                    reason: None,
                    next_run: status.next_run.clone(),
                    last_run: status.last_run.clone(),
                    run_count: status.run_count,
                    consecutive_failures: status.consecutive_failures,
                }
            } else {
                build_unscheduled_status(definition, maint, capabilities)
            }
        })
        .collect()
}

fn build_unscheduled_status(
    definition: &MaintenanceTaskDefinition,
    maint: &MaintenanceConfig,
    capabilities: MaintenanceRuntimeCapabilities,
) -> CliTaskStatus {
    let reason = if definition.implementation_status()
        == oikonomos::maintenance::MaintenanceTaskImplementationStatus::Planned
    {
        Some("task is planned but not yet implemented".to_owned())
    } else if definition.is_manual_only() {
        Some("manual only".to_owned())
    } else {
        match definition.availability(maint, capabilities) {
            MaintenanceTaskAvailability::Available => Some("not scheduled".to_owned()),
            MaintenanceTaskAvailability::Unavailable { reason } => Some(reason.to_owned()),
        }
    };

    CliTaskStatus {
        id: definition.id().to_owned(),
        name: definition.name().to_owned(),
        scheduled: false,
        enabled: false,
        reason,
        next_run: None,
        last_run: None,
        run_count: 0,
        consecutive_failures: 0,
    }
}

fn print_status_table(statuses: &[CliTaskStatus]) {
    let name_w = statuses
        .iter()
        .map(|s| s.name.len())
        .max()
        .unwrap_or(4)
        .max("Task".len());
    let status_w = statuses
        .iter()
        .map(|s| {
            if s.scheduled {
                if s.enabled {
                    "scheduled".len()
                } else {
                    "disabled".len()
                }
            } else {
                s.reason.as_deref().unwrap_or("unavailable").len()
            }
        })
        .max()
        .unwrap_or(11)
        .max("Status".len());
    let runs_w = statuses
        .iter()
        .map(|s| s.run_count.to_string().len())
        .max()
        .unwrap_or(1)
        .max("Runs".len());

    println!(
        "{:<name_w$} {:<status_w$} {:<runs_w$} Last Run",
        "Task", "Status", "Runs"
    );
    println!("{}", "-".repeat(name_w + 1 + status_w + 1 + runs_w + 1 + 8));
    for s in statuses {
        let status = if s.scheduled {
            if s.enabled { "scheduled" } else { "disabled" }
        } else {
            s.reason.as_deref().unwrap_or("unavailable")
        };
        let last = s.last_run.as_deref().unwrap_or("never");
        println!(
            "{:<name_w$} {:<status_w$} {:<runs_w$} {}",
            s.name, status, s.run_count, last
        );
    }
}

fn resolve_task(
    name: &str,
    capabilities: MaintenanceRuntimeCapabilities,
) -> Result<&'static MaintenanceTaskDefinition> {
    let Some(definition) = maintenance_task_by_id(name) else {
        let valid = manual_maintenance_task_ids().join(", ");
        whatever!("unknown task: {name}. Valid: {valid}, all")
    };

    match definition.manual_availability(capabilities) {
        MaintenanceTaskAvailability::Available => Ok(definition),
        MaintenanceTaskAvailability::Unavailable { reason } => {
            whatever!("task '{name}' is not available: {reason}")
        }
    }
}

/// Execute a single maintenance task by name.
async fn run_task(
    definition: &MaintenanceTaskDefinition,
    maint: &MaintenanceConfig,
    knowledge_executor: Option<&Arc<dyn KnowledgeMaintenanceExecutor>>,
    verbose: bool,
) -> Result<()> {
    let Some(manual_task) = definition.manual_run() else {
        whatever!(
            "task '{}' is not supported for manual execution",
            definition.id()
        )
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
        ManualMaintenanceTask::DecayRefresh => {
            let executor = require_knowledge_executor(definition.id(), knowledge_executor)?;
            let executor = Arc::clone(executor);
            let report = run_knowledge_blocking(definition.id(), move || {
                executor.refresh_decay_scores("system")
            })
            .await?;
            print_knowledge_report(definition.id(), &report);
        }
        ManualMaintenanceTask::EntityDedup => {
            let executor = require_knowledge_executor(definition.id(), knowledge_executor)?;
            let executor = Arc::clone(executor);
            let report = run_knowledge_blocking(definition.id(), move || {
                executor.deduplicate_entities("system")
            })
            .await?;
            print_knowledge_report(definition.id(), &report);
        }
        ManualMaintenanceTask::GraphRecompute => {
            let executor = require_knowledge_executor(definition.id(), knowledge_executor)?;
            let executor = Arc::clone(executor);
            let report = run_knowledge_blocking(definition.id(), move || {
                executor.recompute_graph_scores("system")
            })
            .await?;
            print_knowledge_report(definition.id(), &report);
        }
        ManualMaintenanceTask::SkillDecay => {
            let executor = require_knowledge_executor(definition.id(), knowledge_executor)?;
            let executor = Arc::clone(executor);
            let report =
                run_knowledge_blocking(definition.id(), move || executor.run_skill_decay("system"))
                    .await?;
            print_knowledge_report(definition.id(), &report);
        }
        ManualMaintenanceTask::DerivedFactsMaterialize => {
            let executor = require_knowledge_executor(definition.id(), knowledge_executor)?;
            let executor = Arc::clone(executor);
            let report = run_knowledge_blocking(definition.id(), move || {
                executor.materialize_derived_facts()
            })
            .await?;
            print_knowledge_report(definition.id(), &report);
        }
        ManualMaintenanceTask::SerendipityDiscovery => {
            let executor = require_knowledge_executor(definition.id(), knowledge_executor)?;
            let executor = Arc::clone(executor);
            let report = run_knowledge_blocking(definition.id(), move || {
                executor.discover_serendipitous_facts("system")
            })
            .await?;
            print_knowledge_report(definition.id(), &report);
        }
    }
    Ok(())
}

fn require_knowledge_executor<'a>(
    id: &str,
    executor: Option<&'a Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<&'a Arc<dyn KnowledgeMaintenanceExecutor>> {
    match executor {
        Some(e) => Ok(e),
        None => whatever!("task '{id}' requires a knowledge maintenance executor"),
    }
}

async fn run_knowledge_blocking<F>(
    task_id: &str,
    f: F,
) -> Result<oikonomos::maintenance::MaintenanceReport>
where
    F: FnOnce() -> oikonomos::error::Result<oikonomos::maintenance::MaintenanceReport>
        + Send
        + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .whatever_context(format!("{task_id}: blocking task panicked"))?
        .whatever_context(format!("{task_id} failed"))
}

fn print_knowledge_report(id: &str, report: &oikonomos::maintenance::MaintenanceReport) {
    let mut output = format!(
        "{} processed, {} modified, {} errors in {}ms",
        report.items_processed, report.items_modified, report.errors, report.duration_ms
    );
    if let Some(detail) = &report.detail {
        output.push_str(&format!(": {detail}"));
    }
    println!("{id}: {output}");
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
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::BTreeSet;

    use oikonomos::maintenance::{
        MaintenanceReport, MaintenanceRuntimeCapabilities, MaintenanceTaskAvailability,
        MaintenanceTaskImplementationStatus, maintenance_task_by_id, manual_maintenance_task_ids,
    };

    use super::*;

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

    #[test]
    fn knowledge_tasks_are_known_without_executor() {
        let definition =
            maintenance_task_by_id("decay-refresh").expect("decay-refresh in registry");
        let capabilities = MaintenanceRuntimeCapabilities {
            has_knowledge_executor: false,
            ..Default::default()
        };
        assert_eq!(
            definition.manual_availability(capabilities),
            MaintenanceTaskAvailability::Unavailable {
                reason: "no knowledge maintenance executor configured",
            }
        );
    }

    #[test]
    fn knowledge_tasks_are_available_with_executor() {
        let definition =
            maintenance_task_by_id("decay-refresh").expect("decay-refresh in registry");
        let capabilities = MaintenanceRuntimeCapabilities {
            has_knowledge_executor: true,
            ..Default::default()
        };
        assert_eq!(
            definition.manual_availability(capabilities),
            MaintenanceTaskAvailability::Available
        );
    }

    #[test]
    fn planned_knowledge_tasks_report_not_implemented() {
        let definition =
            maintenance_task_by_id("embedding-refresh").expect("embedding-refresh in registry");
        assert_eq!(
            definition.implementation_status(),
            MaintenanceTaskImplementationStatus::Planned
        );
        assert_eq!(
            definition.manual_availability(MaintenanceRuntimeCapabilities::default()),
            MaintenanceTaskAvailability::Unavailable {
                reason: "task is planned but not yet implemented",
            }
        );
    }

    #[test]
    fn scheduled_only_tasks_report_not_supported_for_manual_run() {
        let definition =
            maintenance_task_by_id("retention-execution").expect("retention-execution in registry");
        assert!(definition.manual_run().is_none());
        assert_eq!(
            definition.manual_availability(MaintenanceRuntimeCapabilities::default()),
            MaintenanceTaskAvailability::Unavailable {
                reason: "task is not supported for manual execution",
            }
        );
    }

    #[test]
    fn resolve_task_rejects_unknown_id() {
        let capabilities = MaintenanceRuntimeCapabilities::default();
        let err = resolve_task("not-a-task", capabilities).expect_err("unknown task errors");
        assert!(
            err.to_string().contains("unknown task: not-a-task"),
            "error names the task: {err}"
        );
    }

    #[test]
    fn resolve_task_rejects_knowledge_task_without_executor() {
        let capabilities = MaintenanceRuntimeCapabilities::default();
        let err = resolve_task("decay-refresh", capabilities).expect_err("unavailable task errors");
        assert!(
            err.to_string()
                .contains("no knowledge maintenance executor configured"),
            "error explains missing executor: {err}"
        );
    }

    #[test]
    fn resolve_task_accepts_knowledge_task_with_executor() {
        let capabilities = MaintenanceRuntimeCapabilities {
            has_knowledge_executor: true,
            ..Default::default()
        };
        let definition =
            resolve_task("decay-refresh", capabilities).expect("available task resolves");
        assert_eq!(definition.id(), "decay-refresh");
    }

    struct MockKnowledgeExecutor;

    impl KnowledgeMaintenanceExecutor for MockKnowledgeExecutor {
        fn insert_fact(&self, _fact: &episteme::knowledge::Fact) -> oikonomos::error::Result<()> {
            Ok(())
        }

        fn refresh_decay_scores(
            &self,
            _nous_id: &str,
        ) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport {
                items_processed: 12,
                items_modified: 3,
                detail: Some("decay refreshed".to_owned()),
                ..Default::default()
            })
        }

        fn deduplicate_entities(
            &self,
            _nous_id: &str,
        ) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn recompute_graph_scores(
            &self,
            _nous_id: &str,
        ) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn refresh_embeddings(
            &self,
            _nous_id: &str,
        ) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn garbage_collect(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn maintain_indexes(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn health_check(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn run_skill_decay(&self, _nous_id: &str) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn materialize_derived_facts(&self) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn discover_serendipitous_facts(
            &self,
            _nous_id: &str,
        ) -> oikonomos::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }
    }

    #[tokio::test]
    async fn run_task_executes_knowledge_task_with_executor() {
        let maint = MaintenanceConfig::default();
        let executor: Arc<dyn KnowledgeMaintenanceExecutor> = Arc::new(MockKnowledgeExecutor);
        let definition =
            maintenance_task_by_id("decay-refresh").expect("decay-refresh in registry");
        run_task(definition, &maint, Some(&executor), false)
            .await
            .expect("decay-refresh runs with mock executor");
    }

    #[tokio::test]
    async fn run_task_rejects_knowledge_task_without_executor() {
        let maint = MaintenanceConfig::default();
        let definition =
            maintenance_task_by_id("decay-refresh").expect("decay-refresh in registry");
        let err = run_task(definition, &maint, None, false)
            .await
            .expect_err("decay-refresh fails without executor");
        assert!(
            err.to_string()
                .contains("requires a knowledge maintenance executor"),
            "error explains missing executor: {err}"
        );
    }
}
