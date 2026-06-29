// kanon:ignore RUST/file-too-long — single builtin dispatch match arm; splitting would fragment logic
//! Task action execution: commands and builtins.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use koina::system::{Environment, RealSystem};
use snafu::ResultExt;
use tokio_util::sync::CancellationToken;

use crate::bridge::DaemonBridge;
use crate::cron::{evolution, graph_cleanup, reflection};
use crate::error::{self, Result};
use crate::maintenance::{
    DbMonitor, DriftDetector, InstanceBackup, InstanceBackupConfig, KnowledgeMaintenanceExecutor,
    MaintenanceConfig, RetentionExecutor, TraceRotator,
};
use crate::probe::{ProbeAuditSummary, ProbeSet, build_probe_audit_prompt};
use crate::prosoche::ProsocheCheck;
use crate::prosoche_audit::{
    BehaviorPatternSnapshot, ProsocheAuditRunner, ProsocheState, SessionSnapshot,
};
#[cfg(test)]
use crate::runner::TaskOutcome;
use crate::runner::{ExecutionResult, command_context, process_output_report};
use crate::schedule::{BuiltinTask, TaskAction};

pub(crate) struct ExecutionContext<'a> {
    pub(crate) nous_id: &'a str,
    pub(crate) bridge: Option<&'a dyn DaemonBridge>,
    pub(crate) maintenance: Option<&'a MaintenanceConfig>,
    pub(crate) retention_executor: Option<Arc<dyn RetentionExecutor>>,
    pub(crate) knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
    #[cfg(feature = "knowledge-store")]
    pub(crate) knowledge_store: Option<Arc<episteme::knowledge_store::KnowledgeStore>>,
    pub(crate) daemon_behavior: &'a taxis::config::DaemonBehaviorConfig,
    pub(crate) cancel: CancellationToken,
    /// Maximum duration a single command may run before being killed.
    ///
    /// WHY: each action arm needs an inner timeout independent of the outer
    /// 2× in-flight watchdog so hung commands are terminated promptly.
    pub(crate) timeout: Duration,
}

/// Execute a task action. Receives owned `Arc`s for executor references
/// so it can be spawned as a `'static` future.
///
/// This is the legacy entry point used by tests; it uses a fresh cancellation
/// token. The runner uses [`execute_action_with_cancel`] so it can propagate
/// cancellation into bridge-dispatched turns.
#[cfg(test)]
#[tracing::instrument(skip_all)]
pub(crate) async fn execute_action(
    action: &TaskAction,
    nous_id: &str,
    bridge: Option<&dyn DaemonBridge>,
    maintenance: Option<&MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
    daemon_behavior: &taxis::config::DaemonBehaviorConfig,
) -> Result<ExecutionResult> {
    execute_action_with_cancel(
        action,
        ExecutionContext {
            nous_id,
            bridge,
            maintenance,
            retention_executor,
            knowledge_executor,
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            daemon_behavior,
            cancel: CancellationToken::new(),
            // WHY: tests use the same default per-task timeout as production
            // task definitions so behavior is representative.
            timeout: Duration::from_mins(5),
        },
    )
    .await
}

/// Execute a task action with a cancellation token that is passed to any
/// bridge-dispatched prompt.
#[tracing::instrument(skip_all)]
pub(crate) async fn execute_action_with_cancel(
    action: &TaskAction,
    ctx: ExecutionContext<'_>,
) -> Result<ExecutionResult> {
    match action {
        TaskAction::Command(cmd) => execute_command(cmd, ctx.cancel.clone(), ctx.timeout).await,
        TaskAction::SelfPrompt(prompt) => {
            crate::self_prompt::execute_self_prompt_with_cancel(
                ctx.nous_id,
                prompt,
                ctx.bridge,
                ctx.cancel,
            )
            .await
        }
        TaskAction::Builtin(builtin) => execute_builtin_with_behavior(builtin, ctx).await,
    }
}

async fn execute_command(
    cmd: &str,
    cancel: CancellationToken,
    timeout: Duration,
) -> Result<ExecutionResult> {
    let command = command_context(cmd);
    // WHY: race the child process against both the configured per-task timeout
    // and a graceful cancellation request. Dropping the `output()` future drops
    // the `tokio::process::Command`, which kills the child process automatically.
    let output = tokio::select! {
        biased;
        output = tokio::process::Command::new("sh").args(["-c", cmd]).output() => output,
        () = cancel.cancelled() => {
            return error::CommandCancelledSnafu {
                command: command.clone(),
            }
            .fail();
        }
        // kanon:ignore TESTING/sleep-in-test — production timeout arm in tokio::select!, not a test sleep
        () = tokio::time::sleep(timeout) => {
            return error::CommandTimedOutSnafu {
                command: command.clone(),
                timeout_secs: timeout.as_secs(),
            }
            .fail();
        }
    };

    let output = output.context(error::CommandFailedSnafu {
        command: command.clone(),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    if output.status.success() {
        tracing::debug!(
            command = %command,
            status = %output.status,
            stdout_bytes = output.stdout.len(),
            stderr_bytes = output.stderr.len(),
            "command succeeded"
        );
        Ok(ExecutionResult::success(Some(stdout.into_owned())))
    } else {
        let reason = process_output_report(output.status, &output.stdout, &output.stderr);

        error::TaskFailedSnafu {
            task_id: command,
            reason,
        }
        .fail()
    }
}

fn default_prosoche_audit_dir() -> PathBuf {
    let root = RealSystem
        .var("ALETHEIA_ROOT")
        .map_or_else(|| PathBuf::from("instance"), PathBuf::from);
    root.join("data").join("prosoche-audits")
}

fn prosoche_db_paths(maintenance: &MaintenanceConfig) -> Vec<PathBuf> {
    let data_dir = &maintenance.db_monitoring.data_dir;
    vec![data_dir.join("sessions.db"), data_dir.join("planning.db")]
}

fn build_prosoche_audit_state(nous_id: &str) -> ProsocheState {
    let total = crate::metrics::cron_executions_total();
    let successes = crate::metrics::cron_executions_ok();
    let errors = crate::metrics::cron_executions_error();
    let mut state = ProsocheState {
        nous_id: nous_id.to_owned(),
        checked_at: jiff::Timestamp::now().to_string(),
        ..ProsocheState::default()
    };

    if total == 0 {
        return state;
    }

    let session_id = format!("daemon-runtime:{nous_id}");
    let turn_count = u32::try_from(total).unwrap_or(u32::MAX);
    let error_count = u32::try_from(errors).unwrap_or(u32::MAX);
    state.sessions.push(SessionSnapshot {
        session_id: session_id.clone(),
        turn_count,
        error_count,
        completed: errors == 0,
        turn_text: format!(
            "daemon runtime task executions total={total} successes={successes} errors={errors}"
        ),
        // WHY: the synthetic daemon-runtime session is always current; age is
        // zero days so it is never treated as stale.
        session_age_days: Some(0),
    });
    state.behavior_patterns.push(BehaviorPatternSnapshot {
        session_id,
        tool_call_count: turn_count,
        tool_error_count: error_count,
        repeated_action_count: 0,
        no_progress_turns: 0,
        avoidance_markers: 0,
        confidence_claims: 0,
    });
    state
}

#[cfg(test)]
#[tracing::instrument(skip_all)]
pub(crate) async fn execute_builtin(
    builtin: &BuiltinTask,
    nous_id: &str,
    bridge: Option<&dyn DaemonBridge>,
    maintenance: Option<&MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    let daemon_behavior = taxis::config::DaemonBehaviorConfig::default();
    execute_builtin_with_behavior(
        builtin,
        ExecutionContext {
            nous_id,
            bridge,
            maintenance,
            retention_executor,
            knowledge_executor,
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            daemon_behavior: &daemon_behavior,
            cancel: CancellationToken::new(),
            timeout: Duration::from_mins(5),
        },
    )
    .await
}

#[tracing::instrument(skip_all)]
#[expect(
    clippy::too_many_lines,
    reason = "match dispatch over builtin variants"
)]
pub(crate) async fn execute_builtin_with_behavior(
    builtin: &BuiltinTask,
    ctx: ExecutionContext<'_>,
) -> Result<ExecutionResult> {
    let nous_id = ctx.nous_id;
    let bridge = ctx.bridge;
    let maintenance = ctx.maintenance;
    let retention_executor = ctx.retention_executor;
    let knowledge_executor = ctx.knowledge_executor;
    #[cfg(feature = "knowledge-store")]
    let knowledge_store = ctx.knowledge_store;
    let daemon_behavior = ctx.daemon_behavior;
    let cancel = ctx.cancel;

    match builtin {
        BuiltinTask::Prosoche => {
            if let Some(bridge) = bridge {
                let prompt = "Run your prosoche heartbeat check per PROSOCHE.md.";
                match bridge
                    .send_prompt_with_cancel(nous_id, "daemon:prosoche", prompt, cancel.clone())
                    .await
                {
                    Ok(result) => {
                        tracing::debug!(
                            nous_id = %nous_id,
                            success = result.is_success(),
                            "prosoche dispatch succeeded"
                        );
                        Ok(ExecutionResult::success(Some("dispatched".to_owned())))
                    }
                    Err(e) => {
                        tracing::warn!(
                            nous_id = %nous_id,
                            error = %e,
                            "prosoche dispatch failed"
                        );
                        Ok(ExecutionResult::failed(Some(format!(
                            "dispatch failed: {e}"
                        ))))
                    }
                }
            } else {
                let mut check = ProsocheCheck::new(nous_id).with_daemon_behavior(daemon_behavior);
                if let Some(maintenance) = maintenance {
                    check = check
                        .with_data_dir(&maintenance.db_monitoring.data_dir)
                        .with_db_paths(prosoche_db_paths(maintenance));
                }
                #[cfg(feature = "knowledge-store")]
                if let Some(store) = knowledge_store {
                    check = check.with_knowledge_store(store);
                }
                let result = check.run().await?;
                Ok(ExecutionResult::success(Some(
                    serde_json::to_string(&result)
                        .unwrap_or_else(|_| "prosoche check complete".to_owned()),
                )))
            }
        }
        BuiltinTask::DecayRefresh
        | BuiltinTask::EntityDedup
        | BuiltinTask::GraphRecompute
        | BuiltinTask::EmbeddingRefresh
        | BuiltinTask::KnowledgeGc
        | BuiltinTask::IndexMaintenance
        | BuiltinTask::GraphHealthCheck
        | BuiltinTask::SkillDecay
        | BuiltinTask::SerendipityDiscovery
        | BuiltinTask::DerivedFactsMaterialize
        | BuiltinTask::KnowledgeConsolidation => {
            execute_knowledge_task(builtin, nous_id, knowledge_executor).await
        }
        BuiltinTask::TraceRotation => {
            let config = maintenance
                .map(|m| m.trace_rotation.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("trace_rotation").entered();
                TraceRotator::new(config).rotate()
            })
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
            Ok(ExecutionResult::success(Some(format!(
                "{} files rotated, {} pruned, {} bytes freed",
                report.files_rotated, report.files_pruned, report.bytes_freed
            ))))
        }
        BuiltinTask::DriftDetection => {
            let config = maintenance
                .map(|m| m.drift_detection.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("drift_detection").entered();
                DriftDetector::new(config).check()
            })
            .await
            .context(error::BlockingJoinSnafu {
                context: "drift detection",
            })??;

            let template_display = report.template_root.display();

            if !report.template_available {
                tracing::warn!(
                    template_path = %template_display,
                    "maintenance: drift detection template unavailable"
                );
                return Ok(ExecutionResult::failed(Some(format!(
                    "drift detection template unavailable: {template_display}"
                ))));
            }

            tracing::info!(
                missing = report.missing_files.len(),
                optional_missing = report.optional_missing_files.len(),
                extra = report.extra_files.len(),
                permission_issues = report.permission_issues.len(),
                template_path = %template_display,
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

            Ok(ExecutionResult::success(Some(format!(
                "{} missing, {} optional missing, {} extra (template: {})",
                report.missing_files.len(),
                report.optional_missing_files.len(),
                report.extra_files.len(),
                template_display,
            ))))
        }
        BuiltinTask::DbSizeMonitor => {
            let config = maintenance
                .map(|m| m.db_monitoring.clone())
                .unwrap_or_default();
            let session_store_health_probe =
                maintenance.and_then(|m| m.session_store_health_probe.clone());
            let report = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("db_size_monitor").entered();
                DbMonitor::new(config)
                    .with_session_store_health_probe(session_store_health_probe)
                    .check()
            })
            .await
            .context(error::BlockingJoinSnafu {
                context: "db size monitor",
            })??;

            let summary: Vec<String> = report
                .databases
                .iter()
                .map(|db| {
                    format!(
                        "{} {}MB ({}, {}, {})",
                        db.name,
                        db.size_bytes / (1024 * 1024),
                        db.status,
                        db.shape,
                        db.health
                    )
                })
                .collect();
            tracing::info!(
                databases = %summary.join(", "),
                "maintenance: db monitor complete"
            );
            Ok(ExecutionResult::success(Some(summary.join(", "))))
        }
        BuiltinTask::RoutingStoreRefresh => {
            let Some(store) = maintenance.and_then(|config| config.after_action_store.clone())
            else {
                return Ok(ExecutionResult::skipped(Some(
                    "skipped — no after-action store configured".to_owned(),
                )));
            };

            store.refresh().await.map_err(|source| {
                error::TaskFailedSnafu {
                    task_id: "routing-store-refresh".to_owned(),
                    reason: source.to_string(),
                }
                .build()
            })?;

            tracing::info!("maintenance: routing after-action store refresh complete");
            Ok(ExecutionResult::success(Some(
                "routing after-action store refreshed".to_owned(),
            )))
        }
        BuiltinTask::SelfAudit => {
            let audit_dir = maintenance
                .map_or_else(default_prosoche_audit_dir, |m| m.prosoche_audit_dir.clone());
            let runner = ProsocheAuditRunner::default_checks(&audit_dir);
            let state = build_prosoche_audit_state(nous_id);
            let outcome = runner.run_audit(&state).await;
            let output = if let Some(err) = outcome.last_persist_error.as_deref() {
                format!(
                    "prosoche self-audit report computed but not persisted: {} findings across {} checks; persist error: {err}",
                    outcome.findings.len(),
                    outcome.check_summary.len()
                )
            } else {
                let persisted = outcome
                    .persisted_path
                    .as_ref()
                    .map(|path| format!("; report persisted to {}", path.display()))
                    .unwrap_or_default();
                format!(
                    "prosoche self-audit complete: {} findings across {} checks{persisted}",
                    outcome.findings.len(),
                    outcome.check_summary.len()
                )
            };
            if outcome.last_persist_error.is_none() {
                Ok(ExecutionResult::success(Some(output)))
            } else {
                Ok(ExecutionResult::failed(Some(output)))
            }
        }
        BuiltinTask::ProbeAudit => execute_probe_audit(nous_id, bridge, cancel.clone()).await,
        BuiltinTask::EvolutionSearch => {
            evolution::execute_evolution(nous_id, bridge, cancel.clone()).await
        }
        BuiltinTask::SelfReflection => {
            reflection::execute_reflection(nous_id, bridge, cancel.clone()).await
        }
        BuiltinTask::GraphCleanup => {
            graph_cleanup::execute_graph_cleanup(nous_id, knowledge_executor).await
        }
        BuiltinTask::OpsFactExtraction => {
            execute_ops_fact_extraction(nous_id, knowledge_executor).await
        }
        BuiltinTask::LessonExtraction => {
            execute_lesson_extraction(nous_id, knowledge_executor).await
        }
        BuiltinTask::SelfPrompt => Ok(ExecutionResult::failed(Some(
            "self-prompt must be dispatched via runner follow-up extraction".to_owned(),
        ))),
        BuiltinTask::ProposeRules => {
            let data_dir = maintenance.map_or_else(
                || {
                    let root = RealSystem.var("ALETHEIA_ROOT").map_or_else(
                        || std::path::PathBuf::from("instance"),
                        std::path::PathBuf::from,
                    );
                    root.join("data")
                },
                |m| m.propose_rules.data_dir.clone(),
            );
            tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("propose_rules").entered();
                // WHY: no live observation stream is wired here yet.
                // propose_rules operates on an empty slice, writing an empty
                // (but valid) proposals file. Future work: wire a serialized
                // observation snapshot from the knowledge store (#2296 follow-up).
                let proposals = episteme::rule_proposals::propose_rules(
                    &[],
                    episteme::rule_proposals::DEFAULT_MIN_OBSERVATIONS,
                    episteme::rule_proposals::DEFAULT_MIN_CONFIDENCE,
                );
                episteme::rule_proposals::write_proposals(&proposals, 0, &data_dir).map_err(|e| {
                    crate::error::TaskFailedSnafu {
                        task_id: "propose-rules".to_owned(),
                        reason: e.to_string(),
                    }
                    .build()
                })
            })
            .await
            .context(error::BlockingJoinSnafu {
                context: "propose-rules",
            })??;

            Ok(ExecutionResult::success(Some(
                "rule proposals written to instance/data/rule_proposals.toml".to_owned(),
            )))
        }
        BuiltinTask::InstanceBackup => {
            let config = maintenance
                .map_or_else(InstanceBackupConfig::default, |m| m.instance_backup.clone());
            let backup_metrics = maintenance.and_then(|m| m.backup_metrics.clone());
            let started = Instant::now();
            let backup_result = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("instance_backup").entered();
                InstanceBackup::new(config).create_backup()
            })
            .await
            .context(error::BlockingJoinSnafu {
                context: "whole-instance backup",
            })?;
            let duration_secs = started.elapsed().as_secs_f64();
            let report = match backup_result {
                Ok(report) => {
                    if report.backup_path.is_some()
                        && let Some(metrics) = backup_metrics.as_ref()
                    {
                        metrics.record_backup_duration(duration_secs, true);
                    }
                    report
                }
                Err(e) => {
                    if let Some(metrics) = backup_metrics.as_ref() {
                        metrics.record_backup_duration(duration_secs, false);
                    }
                    return Err(e);
                }
            };

            tracing::info!(
                files = report.files_copied,
                bytes = report.bytes_copied,
                pruned = report.backups_pruned,
                "maintenance: whole-instance backup complete"
            );
            Ok(ExecutionResult::success(Some(format!(
                "{} files copied ({} bytes), {} old backups pruned",
                report.files_copied, report.bytes_copied, report.backups_pruned
            ))))
        }
        BuiltinTask::PromptAuditRotation => {
            let config = maintenance
                .map(|m| m.prompt_audit.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("prompt_audit_rotation").entered();
                crate::maintenance::PromptAuditRotator::new(config).prune()
            })
            .await
            .context(error::BlockingJoinSnafu {
                context: "prompt audit rotation",
            })??;

            tracing::info!(
                files_pruned = report.files_pruned,
                files_retained = report.files_retained,
                malformed_files_skipped = report.malformed_files_skipped,
                fallback_files_pruned = report.fallback_files_pruned,
                bytes_freed = report.bytes_freed,
                "maintenance: prompt audit rotation complete"
            );
            Ok(ExecutionResult::success(Some(format!(
                "{} files pruned, {} retained, {} malformed skipped, {} fallback-pruned, {} bytes freed",
                report.files_pruned,
                report.files_retained,
                report.malformed_files_skipped,
                report.fallback_files_pruned,
                report.bytes_freed
            ))))
        }
        BuiltinTask::RetentionExecution => {
            let Some(executor) = retention_executor else {
                tracing::info!("retention execution skipped — no executor configured");
                return Ok(ExecutionResult::skipped(Some(
                    "skipped — no executor".to_owned(),
                )));
            };
            let summary = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("retention_execution").entered();
                executor.execute_retention()
            })
            .await
            .context(error::BlockingJoinSnafu {
                context: "retention execution",
            })??;

            tracing::info!(
                sessions = summary.sessions_cleaned,
                cap_sessions = summary.cap_sessions_cleaned,
                messages = summary.messages_cleaned,
                blackboard_entries = summary.blackboard_entries_cleaned,
                bytes_freed = summary.bytes_freed,
                "maintenance: retention complete"
            );
            Ok(ExecutionResult::success(Some(format!(
                "{} sessions ({} cap), {} messages, {} blackboard entries cleaned, {} bytes freed",
                summary.sessions_cleaned,
                summary.cap_sessions_cleaned,
                summary.messages_cleaned,
                summary.blackboard_entries_cleaned,
                summary.bytes_freed
            ))))
        }
    }
}

/// Dispatch the adversarial probe audit via the bridge.
///
/// WHY: the daemon cannot call the LLM directly. We build a structured prompt
/// from the default probe set, dispatch it to the nous, then parse the response
/// to evaluate each probe's constraints locally (no extra round-trip needed).
///
/// Results are logged at INFO level. The nous is instructed (via the prompt) to
/// store the audit outcome as an operational fact in the knowledge graph.
async fn execute_probe_audit(
    nous_id: &str,
    bridge: Option<&dyn DaemonBridge>,
    cancel: CancellationToken,
) -> Result<ExecutionResult> {
    let Some(bridge) = bridge else {
        return Ok(ExecutionResult::skipped(Some(
            "no bridge configured".to_owned(),
        )));
    };

    let probe_set = ProbeSet::default_probes();
    let prompt = build_probe_audit_prompt(&probe_set);

    match bridge
        .send_prompt_with_cancel(nous_id, "daemon:probe-audit", &prompt, cancel)
        .await
    {
        Ok(dispatch_result) => {
            // Evaluate the returned text against each probe's constraints.
            // The bridge returns the full response text in `output`; if absent,
            // treat as empty (all probes that require patterns will fail).
            let response_text = dispatch_result.output.as_deref().unwrap_or_default();

            let results = probe_set.evaluate_all(|probe_id| {
                // WHY: the response contains all probe answers in a single block.
                // We check the full text for each probe's required/forbidden
                // patterns rather than trying to parse per-probe sections.
                // This tolerates formatting variation in the LLM response.
                if response_text.to_lowercase().contains(probe_id) || !response_text.is_empty() {
                    Some(response_text)
                } else {
                    None
                }
            });

            let summary = ProbeAuditSummary::from_results(results);

            tracing::info!(
                nous_id = %nous_id,
                total = summary.total,
                passed = summary.passed,
                failed = summary.failed,
                avg_confidence = summary.avg_confidence,
                "probe-audit complete"
            );

            for result in &summary.results {
                if !result.passed {
                    tracing::warn!(
                        probe_id = result.probe_id,
                        category = ?result.category,
                        confidence = result.confidence,
                        violations = ?result.violations,
                        missing_required = ?result.missing_required,
                        "probe-audit: probe failed"
                    );
                }
            }

            let outcome = if dispatch_result.is_success() {
                ExecutionResult::success(Some(summary.one_line()))
            } else {
                ExecutionResult::failed(Some(summary.one_line()))
            };
            Ok(outcome)
        }
        Err(e) => {
            tracing::warn!(
                nous_id = %nous_id,
                error = %e,
                "probe-audit dispatch failed"
            );
            Ok(ExecutionResult::failed(Some(format!(
                "probe-audit dispatch failed: {e}"
            ))))
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
            "knowledge maintenance skipped: no executor configured"
        );
        return Ok(ExecutionResult::skipped(Some(
            "no knowledge maintenance executor configured".to_owned(),
        )));
    };

    let task_name = format!("{builtin:?}");
    let nous_id_owned = nous_id.to_owned();
    let builtin_clone = *builtin;

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
            BuiltinTask::SerendipityDiscovery => {
                executor.discover_serendipitous_facts(&nous_id_owned)
            }
            BuiltinTask::DerivedFactsMaterialize => executor.materialize_derived_facts(),
            BuiltinTask::KnowledgeConsolidation => executor.consolidate_knowledge(&nous_id_owned),
            BuiltinTask::Prosoche
            | BuiltinTask::TraceRotation
            | BuiltinTask::DriftDetection
            | BuiltinTask::DbSizeMonitor
            | BuiltinTask::RetentionExecution
            | BuiltinTask::SelfAudit
            | BuiltinTask::ProbeAudit
            | BuiltinTask::EvolutionSearch
            | BuiltinTask::SelfReflection
            | BuiltinTask::GraphCleanup
            | BuiltinTask::OpsFactExtraction
            | BuiltinTask::LessonExtraction
            | BuiltinTask::SelfPrompt
            | BuiltinTask::ProposeRules
            | BuiltinTask::InstanceBackup
            | BuiltinTask::PromptAuditRotation
            | BuiltinTask::RoutingStoreRefresh => error::TaskFailedSnafu {
                task_id: format!("{builtin_clone:?}"),
                reason: "non-knowledge task routed to knowledge executor".to_owned(),
            }
            .fail(),
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

    let outcome = report.outcome();
    let output = match outcome {
        crate::maintenance::MaintenanceOutcome::Success => format!(
            "{} processed, {} modified in {}ms",
            report.items_processed, report.items_modified, report.duration_ms
        ),
        crate::maintenance::MaintenanceOutcome::Degraded => format!(
            "degraded: {} processed, {} modified, {} non-fatal errors in {}ms",
            report.items_processed, report.items_modified, report.errors, report.duration_ms
        ),
        crate::maintenance::MaintenanceOutcome::Failure => {
            format!(
                "failed: {} processed, {} modified in {}ms",
                report.items_processed, report.items_modified, report.duration_ms
            )
        }
    };

    Ok(ExecutionResult::from_maintenance_report(&report, output))
}

/// Execute lesson extraction from training data JSONL files.
///
/// Reads `workflow/training/violations.jsonl` and `lint.jsonl`, extracts
/// patterns from PR outcomes, and logs the results. The training data path
/// is resolved relative to the current working directory.
///
/// WHY: blocking I/O (file reads + JSON parsing) is done on the blocking
/// pool to avoid starving the async scheduler.
async fn execute_lesson_extraction(
    nous_id: &str,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    let Some(executor) = knowledge_executor else {
        return error::TaskFailedSnafu {
            task_id: "lesson-extraction".to_owned(),
            reason: "no knowledge executor configured for fact persistence".to_owned(),
        }
        .fail();
    };

    let nous_id = nous_id.to_owned();
    let result = tokio::task::spawn_blocking(move || {
        // WHY: training data lives at repo root under workflow/training/.
        // The daemon runs from the instance directory, so we look for the
        // training dir relative to cwd first, then fall back to an absolute path.
        let candidates = [
            std::path::PathBuf::from("workflow/training"),
            std::path::PathBuf::from("../workflow/training"),
        ];

        let training_dir = candidates.iter().find(|p| p.exists());

        let Some(training_dir) = training_dir else {
            return Ok(ExecutionResult::skipped(Some(
                "skipped: no training data directory found".to_owned(),
            )));
        };

        execute_lesson_extraction_from_dir(&nous_id, training_dir, executor.as_ref())
    })
    .await
    .context(error::BlockingJoinSnafu {
        context: "lesson extraction",
    })??;

    Ok(result)
}

/// Extract operational metrics into knowledge graph facts.
///
/// Collects a snapshot of current cron execution counters and converts
/// them into `Fact` values via `OpsFactExtractor`, then persists every
/// fact through the daemon knowledge executor.
async fn execute_ops_fact_extraction(
    nous_id: &str,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    let Some(executor) = knowledge_executor else {
        return error::TaskFailedSnafu {
            task_id: "ops-fact-extraction".to_owned(),
            reason: "no knowledge executor configured for fact persistence".to_owned(),
        }
        .fail();
    };

    let nous_id = nous_id.to_owned();
    tokio::task::spawn_blocking(move || {
        execute_ops_fact_extraction_blocking(&nous_id, executor.as_ref())
    })
    .await
    .context(error::BlockingJoinSnafu {
        context: "ops-fact-extraction",
    })?
}

fn execute_ops_fact_extraction_blocking(
    nous_id: &str,
    executor: &dyn KnowledgeMaintenanceExecutor,
) -> Result<ExecutionResult> {
    use episteme::ops_facts::{OpsFactExtractor, OpsSnapshot};

    // WHY: the daemon's cron execution counters are the source of truth for
    // runtime task-call totals. Shadow counters in `metrics.rs` make the
    // aggregate values readable despite prometheus-client's write-only API.
    let snapshot = OpsSnapshot {
        nous_id: nous_id.to_owned(),
        active_session_count: 0, // NOTE: populated by caller when session store is available
        tool_call_total: crate::metrics::cron_executions_total(),
        tool_call_successes: crate::metrics::cron_executions_ok(),
        error_count: crate::metrics::cron_executions_error(),
        avg_task_latency_ms: 0,
        task_sample_count: 0,
    };

    let facts = OpsFactExtractor::extract(&snapshot, episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS)
        .map_err(|e| {
        error::TaskFailedSnafu {
            task_id: String::from("ops-fact-extraction"),
            reason: e.to_string(),
        }
        .build()
    })?;

    let count = facts.len();
    for ops_fact in &facts {
        tracing::debug!(
            nous_id = %nous_id,
            fact_type = %ops_fact.fact.fact_type,
            content = %ops_fact.fact.content,
            confidence = ops_fact.fact.provenance.confidence,
            "operational fact extracted"
        );
    }

    persist_facts(
        executor,
        facts.iter().map(|ops_fact| &ops_fact.fact),
        "ops-fact-extraction",
    )?;

    tracing::info!(
        nous_id = %nous_id,
        facts_extracted = count,
        facts_inserted = count,
        "operational fact extraction complete"
    );

    Ok(ExecutionResult::success(Some(format!(
        "{count} operational facts extracted, {count} inserted"
    ))))
}

fn execute_lesson_extraction_from_dir(
    nous_id: &str,
    training_dir: &Path,
    executor: &dyn KnowledgeMaintenanceExecutor,
) -> Result<ExecutionResult> {
    let extraction = episteme::extract::training::extract_from_training_data(training_dir)
        .context(error::MaintenanceIoSnafu {
            context: "lesson extraction",
        })?;

    let lesson_count = extraction.lessons.len();
    let extracted_facts = episteme::extract::training::lessons_to_facts(&extraction.lessons);
    let durable_facts = extracted_facts_to_facts(
        nous_id,
        &extracted_facts,
        "daemon:lesson-extraction",
        jiff::Timestamp::now(),
    )?;
    let inserted = durable_facts.len();
    persist_facts(executor, durable_facts.iter(), "lesson-extraction")?;

    tracing::info!(
        violations_read = extraction.violations_read,
        lint_summaries_read = extraction.lint_summaries_read,
        lessons_extracted = lesson_count,
        facts_produced = durable_facts.len(),
        facts_inserted = inserted,
        records_skipped = extraction.records_skipped,
        "lesson extraction complete"
    );

    Ok(ExecutionResult::success(Some(format!(
        "{lesson_count} lessons extracted, {} facts produced, {inserted} inserted ({} violations, {} lint summaries read)",
        durable_facts.len(),
        extraction.violations_read,
        extraction.lint_summaries_read,
    ))))
}

fn extracted_facts_to_facts(
    nous_id: &str,
    facts: &[episteme::extract::ExtractedFact],
    source: &str,
    now: jiff::Timestamp,
) -> Result<Vec<episteme::knowledge::Fact>> {
    facts
        .iter()
        .enumerate()
        .map(|(index, fact)| extracted_fact_to_fact(nous_id, fact, source, now, index))
        .collect()
}

fn extracted_fact_to_fact(
    nous_id: &str,
    fact: &episteme::extract::ExtractedFact,
    source: &str,
    now: jiff::Timestamp,
    index: usize,
) -> Result<episteme::knowledge::Fact> {
    use episteme::knowledge::{
        EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal, FactType,
        MemoryScope, Visibility, far_future,
    };

    let content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
    let id = episteme::id::FactId::new(format!("daemon-fact-{}-{index}", koina::ulid::Ulid::new()))
        .map_err(|e| {
            error::TaskFailedSnafu {
                task_id: "lesson-extraction".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;
    let classified_type = fact
        .fact_type
        .as_deref()
        .map_or_else(|| FactType::classify(&content), FactType::from_str_lossy);

    Ok(Fact {
        id,
        nous_id: nous_id.to_owned(),
        fact_type: classified_type.as_str().to_owned(),
        content,
        scope: Some(MemoryScope::Project),
        project_id: None,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: fact.confidence,
            tier: EpistemicTier::Inferred,
            source_session_id: Some(source.to_owned()),
            stability_hours: classified_type.base_stability_hours(),
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
        sensitivity: episteme::knowledge::FactSensitivity::Public,
        visibility: Visibility::Private,
    })
}

fn persist_facts<'a>(
    executor: &dyn KnowledgeMaintenanceExecutor,
    facts: impl IntoIterator<Item = &'a episteme::knowledge::Fact>,
    task_id: &str,
) -> Result<()> {
    for fact in facts {
        executor.insert_fact(fact).map_err(|e| {
            error::TaskFailedSnafu {
                task_id: task_id.to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod execution_tests;

#[cfg(test)]
#[path = "execution_knowledge_tests.rs"]
mod execution_knowledge_tests;
