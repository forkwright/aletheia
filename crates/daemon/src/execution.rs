//! Task action execution: commands and builtins.

use std::sync::Arc;

use snafu::ResultExt;

use crate::bridge::DaemonBridge;
use crate::error::{self, Result};
use crate::maintenance::{
    DbMonitor, DriftDetector, KnowledgeMaintenanceExecutor, MaintenanceConfig, RetentionExecutor,
    TraceRotator,
};
use crate::runner::ExecutionResult;
use crate::schedule::{BuiltinTask, TaskAction};

/// Execute a task action. Receives owned `Arc`s for executor references
/// so it can be spawned as a `'static` future.
pub(crate) async fn execute_action(
    action: &TaskAction,
    nous_id: &str,
    bridge: Option<&dyn DaemonBridge>,
    maintenance: Option<&MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    match action {
        TaskAction::Command(cmd) => execute_command(cmd).await,
        TaskAction::Builtin(builtin) => {
            execute_builtin(
                builtin,
                nous_id,
                bridge,
                maintenance,
                retention_executor,
                knowledge_executor,
            )
            .await
        }
    }
}

async fn execute_command(cmd: &str) -> Result<ExecutionResult> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", cmd])
        .output()
        .await
        .context(error::CommandFailedSnafu {
            command: cmd.to_owned(),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        tracing::debug!(cmd = %cmd, stdout = %stdout, "command succeeded");
        Ok(ExecutionResult {
            success: true,
            output: Some(stdout.into_owned()),
        })
    } else {
        let reason = if stderr.is_empty() {
            format!("exit code: {}", output.status)
        } else {
            stderr.into_owned()
        };

        error::TaskFailedSnafu {
            task_id: cmd.to_owned(),
            reason,
        }
        .fail()
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "match dispatch over builtin variants"
)]
pub(crate) async fn execute_builtin(
    builtin: &BuiltinTask,
    nous_id: &str,
    bridge: Option<&dyn DaemonBridge>,
    maintenance: Option<&MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    match builtin {
        BuiltinTask::Prosoche => {
            if let Some(bridge) = bridge {
                let prompt = "Run your prosoche heartbeat check per PROSOCHE.md.";
                match bridge.send_prompt(nous_id, "daemon:prosoche", prompt).await {
                    Ok(result) => {
                        tracing::debug!(
                            nous_id = %nous_id,
                            success = result.success,
                            "prosoche dispatch succeeded"
                        );
                        Ok(ExecutionResult {
                            success: true,
                            output: Some("dispatched".to_owned()),
                        })
                    }
                    Err(e) => {
                        tracing::warn!(
                            nous_id = %nous_id,
                            error = %e,
                            "prosoche dispatch failed"
                        );
                        Ok(ExecutionResult {
                            success: false,
                            output: Some(format!("dispatch failed: {e}")),
                        })
                    }
                }
            } else {
                Ok(ExecutionResult {
                    success: false,
                    output: Some("no bridge configured".to_owned()),
                })
            }
        }
        BuiltinTask::DecayRefresh
        | BuiltinTask::EntityDedup
        | BuiltinTask::GraphRecompute
        | BuiltinTask::EmbeddingRefresh
        | BuiltinTask::KnowledgeGc
        | BuiltinTask::IndexMaintenance
        | BuiltinTask::GraphHealthCheck
        | BuiltinTask::SkillDecay => {
            execute_knowledge_task(builtin, nous_id, knowledge_executor).await
        }
        BuiltinTask::TraceRotation => {
            let config = maintenance
                .map(|m| m.trace_rotation.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || TraceRotator::new(config).rotate())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "trace rotation",
                })??;

            tracing::info!(
                rotated = report.files_rotated,
                pruned = report.files_pruned,
                bytes_freed = report.bytes_freed,
                "maintenance: trace rotation complete"
            );
            Ok(ExecutionResult {
                success: true,
                output: Some(format!(
                    "{} files rotated, {} pruned, {} bytes freed",
                    report.files_rotated, report.files_pruned, report.bytes_freed
                )),
            })
        }
        BuiltinTask::DriftDetection => {
            let config = maintenance
                .map(|m| m.drift_detection.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || DriftDetector::new(config).check())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "drift detection",
                })??;

            tracing::info!(
                missing = report.missing_files.len(),
                optional_missing = report.optional_missing_files.len(),
                extra = report.extra_files.len(),
                permission_issues = report.permission_issues.len(),
                "maintenance: drift detection complete"
            );

            for path in &report.missing_files {
                tracing::warn!(
                    metric = "missing_file",
                    path = %path.display(),
                    expected = "present",
                    actual = "absent",
                    checked_at = %report.checked_at.map(|ts| ts.to_string()).as_deref().unwrap_or("unknown"),
                    "drift alert: required file missing from instance"
                );
            }
            for path in &report.optional_missing_files {
                tracing::info!(
                    metric = "optional_missing_file",
                    path = %path.display(),
                    expected = "present",
                    actual = "absent",
                    "drift: optional scaffolding file absent from instance"
                );
            }
            for path in &report.extra_files {
                tracing::warn!(
                    metric = "extra_file",
                    path = %path.display(),
                    expected = "absent",
                    actual = "present",
                    checked_at = %report.checked_at.map(|ts| ts.to_string()).as_deref().unwrap_or("unknown"),
                    "drift alert: unexpected file in instance"
                );
            }

            Ok(ExecutionResult {
                success: true,
                output: Some(format!(
                    "{} missing, {} optional missing, {} extra",
                    report.missing_files.len(),
                    report.optional_missing_files.len(),
                    report.extra_files.len()
                )),
            })
        }
        BuiltinTask::DbSizeMonitor => {
            let config = maintenance
                .map(|m| m.db_monitoring.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || DbMonitor::new(config).check())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "db size monitor",
                })??;

            let summary: Vec<String> = report
                .databases
                .iter()
                .map(|db| {
                    format!(
                        "{} {}MB ({})",
                        db.name,
                        db.size_bytes / (1024 * 1024),
                        db.status
                    )
                })
                .collect();
            tracing::info!(
                databases = %summary.join(", "),
                "maintenance: db monitor complete"
            );
            Ok(ExecutionResult {
                success: true,
                output: Some(summary.join(", ")),
            })
        }
        BuiltinTask::ChironAudit => {
            if let Some(bridge) = bridge {
                let prompt = "Run chiron self-audit: execute all registered prosoche checks.";
                match bridge
                    .send_prompt(nous_id, "daemon:chiron-audit", prompt)
                    .await
                {
                    Ok(result) => {
                        tracing::info!(
                            nous_id = %nous_id,
                            success = result.success,
                            "chiron audit dispatch succeeded"
                        );
                        Ok(ExecutionResult {
                            success: true,
                            output: Some("dispatched".to_owned()),
                        })
                    }
                    Err(e) => {
                        tracing::warn!(
                            nous_id = %nous_id,
                            error = %e,
                            "chiron audit dispatch failed"
                        );
                        Ok(ExecutionResult {
                            success: false,
                            output: Some(format!("dispatch failed: {e}")),
                        })
                    }
                }
            } else {
                Ok(ExecutionResult {
                    success: false,
                    output: Some("no bridge configured".to_owned()),
                })
            }
        }
        BuiltinTask::RetentionExecution => {
            let Some(executor) = retention_executor else {
                tracing::info!("retention execution skipped — no executor configured");
                return Ok(ExecutionResult {
                    success: true,
                    output: Some("skipped — no executor".to_owned()),
                });
            };
            let summary = tokio::task::spawn_blocking(move || executor.execute_retention())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "retention execution",
                })??;

            tracing::info!(
                sessions = summary.sessions_cleaned,
                messages = summary.messages_cleaned,
                bytes_freed = summary.bytes_freed,
                "maintenance: retention complete"
            );
            Ok(ExecutionResult {
                success: true,
                output: Some(format!(
                    "{} sessions, {} messages cleaned, {} bytes freed",
                    summary.sessions_cleaned, summary.messages_cleaned, summary.bytes_freed
                )),
            })
        }
    }
}

/// Dispatch a knowledge maintenance task to the executor via `spawn_blocking`.
async fn execute_knowledge_task(
    builtin: &BuiltinTask,
    nous_id: &str,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    let Some(executor) = knowledge_executor else {
        tracing::warn!(
            task = ?builtin,
            "knowledge maintenance NOT_IMPLEMENTED: no executor configured — task did not run"
        );
        return Ok(ExecutionResult {
            success: false,
            output: Some("NOT_IMPLEMENTED: no executor configured".to_owned()),
        });
    };

    let task_name = format!("{builtin:?}");
    let nous_id_owned = nous_id.to_owned();
    let builtin_clone = builtin.clone();

    let report = tokio::task::spawn_blocking(move || {
        let _span = tracing::info_span!(
            "knowledge_maintenance",
            task = %task_name,
            nous_id = %nous_id_owned,
        )
        .entered();

        let start = std::time::Instant::now();
        let mut report = match builtin_clone {
            BuiltinTask::DecayRefresh => executor.refresh_decay_scores(&nous_id_owned),
            BuiltinTask::EntityDedup => executor.deduplicate_entities(&nous_id_owned),
            BuiltinTask::GraphRecompute => executor.recompute_graph_scores(&nous_id_owned),
            BuiltinTask::EmbeddingRefresh => executor.refresh_embeddings(&nous_id_owned),
            BuiltinTask::KnowledgeGc => executor.garbage_collect(&nous_id_owned),
            BuiltinTask::IndexMaintenance => executor.maintain_indexes(&nous_id_owned),
            BuiltinTask::GraphHealthCheck => executor.health_check(&nous_id_owned),
            BuiltinTask::SkillDecay => executor.run_skill_decay(&nous_id_owned),
            _ => unreachable!("non-knowledge task routed to execute_knowledge_task"),
        }?;

        report.duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        tracing::info!(
            items_processed = report.items_processed,
            items_modified = report.items_modified,
            duration_ms = report.duration_ms,
            errors = report.errors,
            "knowledge maintenance complete"
        );

        Ok(report)
    })
    .await
    .context(error::BlockingJoinSnafu {
        context: format!("knowledge maintenance: {builtin:?}"),
    })??;

    Ok(ExecutionResult {
        success: true,
        output: Some(format!(
            "{} processed, {} modified in {}ms",
            report.items_processed, report.items_modified, report.duration_ms
        )),
    })
}
