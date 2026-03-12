//! `aletheia maintenance` — instance maintenance task management.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;

use aletheia_oikonomos::maintenance::{
    DbMonitor, DbMonitoringConfig, DriftDetectionConfig, DriftDetector, MaintenanceConfig,
    TraceRotationConfig, TraceRotator,
};
use aletheia_oikonomos::runner::TaskRunner;
use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Subcommand)]
pub enum Action {
    /// Show status of all maintenance tasks
    Status,
    /// Run a specific maintenance task immediately
    Run {
        /// Task name: trace-rotation, drift-detection, db-monitor, or all
        task: String,
        /// List individual files (drift-detection only)
        #[arg(long)]
        verbose: bool,
    },
}

pub fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let config = load_config(&oikos).context("failed to load config")?;
    let maint = build_config(&oikos, &config.maintenance);

    match action {
        Action::Status => {
            let token = CancellationToken::new();
            let mut runner = TaskRunner::new("system", token).with_maintenance(maint);
            runner.register_maintenance_tasks();
            let statuses = runner.status();
            println!("{}", serde_json::to_string_pretty(&statuses)?);
        }
        Action::Run { task, verbose } => {
            let tasks: Vec<&str> = if task == "all" {
                vec!["trace-rotation", "drift-detection", "db-monitor"]
            } else {
                vec![task.as_str()]
            };
            for name in tasks {
                match name {
                    "trace-rotation" => {
                        let report = TraceRotator::new(maint.trace_rotation.clone())
                            .rotate()
                            .context("trace rotation failed")?;
                        println!(
                            "trace-rotation: {} rotated, {} pruned, {} bytes freed",
                            report.files_rotated, report.files_pruned, report.bytes_freed
                        );
                    }
                    "drift-detection" => {
                        let report = DriftDetector::new(maint.drift_detection.clone())
                            .check()
                            .context("drift detection failed")?;
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
                    "db-monitor" => {
                        let report = DbMonitor::new(maint.db_monitoring.clone())
                            .check()
                            .context("db monitor failed")?;
                        for db in &report.databases {
                            println!(
                                "db-monitor: {} {}MB ({})",
                                db.name,
                                db.size_bytes / (1024 * 1024),
                                db.status
                            );
                        }
                    }
                    other => anyhow::bail!(
                        "unknown task: {other}. Valid: trace-rotation, drift-detection, db-monitor, all"
                    ),
                }
            }
        }
    }
    Ok(())
}

/// Build a `MaintenanceConfig` from the oikos layout and config settings.
///
/// Called from both the maintenance subcommand and the server startup path.
pub fn build_config(
    oikos: &Oikos,
    settings: &aletheia_taxis::config::MaintenanceSettings,
) -> MaintenanceConfig {
    MaintenanceConfig {
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
        },
        db_monitoring: DbMonitoringConfig {
            enabled: settings.db_monitoring.enabled,
            data_dir: oikos.data(),
            warn_threshold_mb: settings.db_monitoring.warn_threshold_mb,
            alert_threshold_mb: settings.db_monitoring.alert_threshold_mb,
        },
        retention: aletheia_oikonomos::maintenance::RetentionConfig {
            enabled: settings.retention.enabled,
        },
        knowledge_maintenance: aletheia_oikonomos::maintenance::KnowledgeMaintenanceConfig {
            enabled: settings.knowledge_maintenance_enabled,
        },
    }
}
