//! Task action execution: commands and builtins.

use std::sync::Arc;

use snafu::ResultExt;
use tracing::Instrument;

use crate::bridge::DaemonBridge;
use crate::cron::{evolution, graph_cleanup, reflection};
use crate::error::{self, Result};
use crate::maintenance::{
    DbMonitor, DriftDetector, KnowledgeMaintenanceExecutor, MaintenanceConfig, RetentionExecutor,
    TraceRotator,
};
use crate::probe::{ProbeAuditSummary, ProbeSet, build_probe_audit_prompt};
use crate::runner::ExecutionResult;
use crate::schedule::{BuiltinTask, TaskAction};

/// Execute a task action. Receives owned `Arc`s for executor references
/// so it can be spawned as a `'static` future.
#[tracing::instrument(skip_all)]
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
    // NOTE: tokio::process::Child kills the child on Drop, providing the same
    // orphan-prevention guarantee as ProcessGuard. The .output() method spawns,
    // waits, and collects in one step -- if the future is cancelled, the child
    // is killed automatically by tokio's drop semantics.
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

#[tracing::instrument(skip_all)]
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
            let report = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("drift_detection").entered();
                DriftDetector::new(config).check()
            })
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
            let report = tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("db_size_monitor").entered();
                DbMonitor::new(config).check()
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
        BuiltinTask::SelfAudit => {
            if let Some(bridge) = bridge {
                let prompt = "Run self-audit: execute all registered prosoche checks.";
                match bridge
                    .send_prompt(nous_id, "daemon:self-audit", prompt)
                    .await
                {
                    Ok(result) => {
                        tracing::info!(
                            nous_id = %nous_id,
                            success = result.success,
                            "self-audit dispatch succeeded"
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
                            "self-audit dispatch failed"
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
        BuiltinTask::ProbeAudit => execute_probe_audit(nous_id, bridge).await,
        BuiltinTask::EvolutionSearch => evolution::execute_evolution(nous_id, bridge).await,
        BuiltinTask::SelfReflection => reflection::execute_reflection(nous_id, bridge).await,
        BuiltinTask::GraphCleanup => {
            graph_cleanup::execute_graph_cleanup(nous_id, knowledge_executor).await
        }
        BuiltinTask::OpsFactExtraction => execute_ops_fact_extraction(nous_id).await,
        BuiltinTask::LessonExtraction => execute_lesson_extraction().await,
        BuiltinTask::SelfPrompt => {
            // NOTE: SelfPrompt is dispatched inline by the runner after
            // extracting a follow-up from prosoche output. This arm handles
            // the case where it's registered as a standalone task (should not
            // happen in normal operation).
            Ok(ExecutionResult {
                success: false,
                output: Some(
                    "self-prompt must be dispatched via runner follow-up extraction".to_owned(),
                ),
            })
        }
        BuiltinTask::ProposeRules => {
            let data_dir = maintenance
                .map(|m| m.propose_rules.data_dir.clone())
                .unwrap_or_else(|| {
                    let root = std::env::var("ALETHEIA_ROOT")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::path::PathBuf::from("instance"));
                    root.join("data")
                });
            tokio::task::spawn_blocking(move || {
                let _span = tracing::info_span!("propose_rules").entered();
                // WHY: no live observation stream is wired here yet.
                // propose_rules operates on an empty slice, writing an empty
                // (but valid) proposals file. Future work: wire a serialized
                // observation snapshot from the knowledge store (#2296 follow-up).
                let proposals = aletheia_episteme::rule_proposals::propose_rules(&[]);
                aletheia_episteme::rule_proposals::write_proposals(
                    &proposals,
                    0,
                    &data_dir,
                )
                .map_err(|e| crate::error::TaskFailedSnafu {
                    task_id: "propose-rules".to_owned(),
                    reason: e.to_string(),
                }.build())
            })
            .await
            .context(error::BlockingJoinSnafu {
                context: "propose-rules",
            })??;

            Ok(ExecutionResult {
                success: true,
                output: Some("rule proposals written to instance/data/rule_proposals.toml".to_owned()),
            })
        }
        BuiltinTask::RetentionExecution => {
            let Some(executor) = retention_executor else {
                tracing::info!("retention execution skipped — no executor configured");
                return Ok(ExecutionResult {
                    success: true,
                    output: Some("skipped — no executor".to_owned()),
                });
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
) -> Result<ExecutionResult> {
    let Some(bridge) = bridge else {
        return Ok(ExecutionResult {
            success: false,
            output: Some("no bridge configured".to_owned()),
        });
    };

    let probe_set = ProbeSet::default_probes();
    let prompt = build_probe_audit_prompt(&probe_set);

    match bridge
        .send_prompt(nous_id, "daemon:probe-audit", &prompt)
        .await
    {
        Ok(dispatch_result) => {
            // Evaluate the returned text against each probe's constraints.
            // The bridge returns the full response text in `output`; if absent,
            // treat as empty (all probes that require patterns will fail).
            let response_text = dispatch_result
                .output
                .as_deref()
                .unwrap_or_default();

            let results = probe_set.evaluate_all(|probe_id| {
                // WHY: the response contains all probe answers in a single block.
                // We check the full text for each probe's required/forbidden
                // patterns rather than trying to parse per-probe sections.
                // This is robust against formatting variation in the LLM response.
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

            Ok(ExecutionResult {
                success: dispatch_result.success,
                output: Some(summary.one_line()),
            })
        }
        Err(e) => {
            tracing::warn!(
                nous_id = %nous_id,
                error = %e,
                "probe-audit dispatch failed"
            );
            Ok(ExecutionResult {
                success: false,
                output: Some(format!("probe-audit dispatch failed: {e}")),
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
            | BuiltinTask::ProposeRules => {
                unreachable!("non-knowledge task routed to execute_knowledge_task")
            }
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

/// Execute lesson extraction from training data JSONL files.
///
/// Reads `workflow/training/violations.jsonl` and `lint.jsonl`, extracts
/// patterns from PR outcomes, and logs the results. The training data path
/// is resolved relative to the current working directory.
///
/// WHY: blocking I/O (file reads + JSON parsing) is done on the blocking
/// pool to avoid starving the async scheduler.
async fn execute_lesson_extraction() -> Result<ExecutionResult> {
    let result = tokio::task::spawn_blocking(|| {
        // WHY: training data lives at repo root under workflow/training/.
        // The daemon runs from the instance directory, so we look for the
        // training dir relative to cwd first, then fall back to an absolute path.
        let candidates = [
            std::path::PathBuf::from("workflow/training"),
            std::path::PathBuf::from("../workflow/training"),
        ];

        let training_dir = candidates.iter().find(|p| p.exists());

        let Some(training_dir) = training_dir else {
            return Ok(ExecutionResult {
                success: true,
                output: Some("skipped: no training data directory found".to_owned()),
            });
        };

        let extraction =
            aletheia_episteme::extract::training::extract_from_training_data(training_dir)
                .context(error::MaintenanceIoSnafu {
                    context: "lesson extraction",
                })?;

        let lesson_count = extraction.lessons.len();
        let facts = aletheia_episteme::extract::training::lessons_to_facts(&extraction.lessons);

        tracing::info!(
            violations_read = extraction.violations_read,
            lint_summaries_read = extraction.lint_summaries_read,
            lessons_extracted = lesson_count,
            facts_produced = facts.len(),
            records_skipped = extraction.records_skipped,
            "lesson extraction complete"
        );

        Ok(ExecutionResult {
            success: true,
            output: Some(format!(
                "{lesson_count} lessons extracted, {} facts produced ({} violations, {} lint summaries read)",
                facts.len(),
                extraction.violations_read,
                extraction.lint_summaries_read,
            )),
        })
    })
    .await
    .context(error::BlockingJoinSnafu {
        context: "lesson extraction",
    })??;

    Ok(result)
}

/// Extract operational metrics into knowledge graph facts.
///
/// Collects a snapshot of current Prometheus counters and converts them
/// into `Fact` values via `OpsFactExtractor`. The facts are logged for
/// now; insertion into the knowledge store happens when the caller has
/// a store handle (daemon bridge integration).
#[expect(
    clippy::unused_async,
    reason = "async signature required by execute_builtin dispatch which awaits all arms"
)]
async fn execute_ops_fact_extraction(nous_id: &str) -> Result<ExecutionResult> {
    use aletheia_episteme::ops_facts::{OpsFactExtractor, OpsSnapshot};

    // WHY: Prometheus global registry is the source of truth for runtime
    // counters. We read the current values to build a point-in-time snapshot.
    let snapshot = OpsSnapshot {
        nous_id: nous_id.to_owned(),
        active_session_count: 0, // NOTE: populated by caller when session store is available
        tool_call_total: read_prometheus_counter("aletheia_cron_executions_total"),
        tool_call_successes: read_prometheus_counter_with_label(
            "aletheia_cron_executions_total",
            "status",
            "ok",
        ),
        error_count: read_prometheus_counter_with_label(
            "aletheia_cron_executions_total",
            "status",
            "error",
        ),
        avg_task_latency_ms: 0,
        task_sample_count: 0,
    };

    let facts = OpsFactExtractor::extract(&snapshot).map_err(|e| {
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

    tracing::info!(
        nous_id = %nous_id,
        facts_extracted = count,
        "operational fact extraction complete"
    );

    Ok(ExecutionResult {
        success: true,
        output: Some(format!("{count} operational facts extracted")),
    })
}

/// Read the total value of a Prometheus counter by metric name.
///
/// Returns 0 if the metric is not found or not readable.
fn read_prometheus_counter(name: &str) -> u64 {
    let families = prometheus::default_registry().gather();
    for family in &families {
        if family.name() == name {
            let mut total = 0.0_f64;
            for metric in family.get_metric() {
                // WHY: `get_counter()` returns `&MessageField<Counter>`;
                // protobuf's `.value()` on `Counter` gives the f64 count.
                total += metric.get_counter().value();
            }
            // SAFETY: Prometheus counter values are non-negative f64 totals from
            // monotonically increasing counters; practical counts are well within
            // u64 range and f64 mantissa (2^53). Truncation is intentional.
            #[expect(
                clippy::as_conversions,
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "f64->u64: counter is non-negative and fits in u64 for practical values"
            )]
            return total as u64;
        }
    }
    0
}

/// Read a Prometheus counter filtered by a specific label value.
///
/// Returns 0 if the metric or label is not found.
fn read_prometheus_counter_with_label(name: &str, label_name: &str, label_value: &str) -> u64 {
    let families = prometheus::default_registry().gather();
    for family in &families {
        if family.name() == name {
            let mut total = 0.0_f64;
            for metric in family.get_metric() {
                let matches = metric
                    .get_label()
                    .iter()
                    .any(|lp| lp.name() == label_name && lp.value() == label_value);
                if matches {
                    total += metric.get_counter().value();
                }
            }
            // SAFETY: Prometheus counter values are non-negative f64 totals from
            // monotonically increasing counters; practical counts are well within
            // u64 range and f64 mantissa (2^53). Truncation is intentional.
            #[expect(
                clippy::as_conversions,
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "f64->u64: counter is non-negative and fits in u64 for practical values"
            )]
            return total as u64;
        }
    }
    0
}
