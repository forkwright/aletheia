//! In-flight task monitoring: completion polling, timeout detection, self-prompting.

use tracing::Instrument;

use crate::schedule::TaskStatus;

use super::{DaemonOutputMode, ExecutionResult, TaskOutcome, TaskRunner};

impl TaskRunner {
    /// Get status of all registered tasks.
    ///
    /// # Complexity
    ///
    /// O(t) where t is the number of registered tasks.
    pub fn status(&self) -> Vec<TaskStatus> {
        // WHY(#5131): when a state store is attached the displayed numbers were
        // restored from disk, so label them "persisted" to disambiguate from a
        // fresh in-memory runner that has never executed a task.
        let data_source = if self.state_store.is_some() {
            "persisted"
        } else {
            "live"
        };
        self.tasks
            .iter()
            .map(|t| TaskStatus {
                id: t.def.id.clone(),
                name: t.def.name.clone(),
                enabled: t.def.enabled,
                next_run: t.next_run.map(|ts| ts.to_string()),
                last_run: t.last_run.map(|ts| ts.to_string()),
                run_count: t.run_count,
                consecutive_failures: t.consecutive_failures,
                in_flight: self.in_flight.contains_key(&t.def.id),
                last_error: t.last_error.clone(),
                data_source: data_source.to_owned(),
                as_of: t.last_run.map(|ts| ts.to_string()),
                last_errors: t.last_errors,
                available: true,
                reason: None,
            })
            .collect()
    }

    /// Check in-flight tasks for completion, timeout warnings, and hung task cancellation.
    ///
    /// # Complexity
    ///
    /// O(i) where i is the number of in-flight tasks.
    pub(super) async fn check_in_flight(&mut self) {
        let task_ids: Vec<String> = self.in_flight.keys().cloned().collect();

        for task_id in task_ids {
            let Some(in_flight) = self.in_flight.get_mut(&task_id) else {
                continue;
            };

            let elapsed = in_flight.started_at.elapsed();

            if elapsed > in_flight.timeout * 2 {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed.as_secs(),
                    timeout_secs = in_flight.timeout.as_secs(),
                    cancelled = true,
                    "hung task detected  -  cancelling (exceeded 2x timeout)"
                );
                in_flight.cancel.cancel();
                in_flight.handle.abort();

                self.in_flight.remove(&task_id);
                self.unregister_watchdog_process(&task_id);
                self.record_task_failure(&task_id, "cancelled: exceeded 2x timeout");
                continue;
            }

            if elapsed > in_flight.timeout && !in_flight.warned {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed.as_secs(),
                    timeout_secs = in_flight.timeout.as_secs(),
                    "task running longer than configured timeout"
                );
                in_flight.warned = true;
            }

            if in_flight.handle.is_finished() {
                let Some(in_flight) = self.in_flight.remove(&task_id) else {
                    continue;
                };
                let duration = in_flight.started_at.elapsed();

                match in_flight.handle.await {
                    Ok(Ok(result)) => {
                        self.log_result(&task_id, &result);
                        self.maybe_queue_self_prompt(&task_id, &result);
                        match result.outcome {
                            TaskOutcome::Success => {
                                self.record_task_completion(&task_id, duration, result.errors);
                            }
                            TaskOutcome::Failed => {
                                let reason = result
                                    .output
                                    .as_deref()
                                    .filter(|s| !s.is_empty())
                                    .map_or_else(
                                        || "task reported failure".to_owned(),
                                        |output| {
                                            super::safe_output_for_mode(
                                                output,
                                                self.output_mode,
                                                &self.daemon_behavior,
                                            )
                                        },
                                    );
                                self.record_task_failure(&task_id, &reason);
                            }
                            TaskOutcome::Skipped => {
                                self.record_task_skip(&task_id);
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        let error = super::redact_task_text(&e.to_string());
                        tracing::warn!(
                            task_id = %task_id,
                            error = %error,
                            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                            "spawned task failed"
                        );
                        self.record_task_failure(&task_id, &error);
                    }
                    Err(e) => {
                        let error = super::redact_task_text(&e.to_string());
                        self.report_watchdog_exit(&task_id, &error);
                        tracing::warn!(
                            task_id = %task_id,
                            error = %error,
                            "spawned task panicked or was cancelled"
                        );
                        self.record_task_failure(&task_id, &error);
                    }
                }
                self.unregister_watchdog_process(&task_id);
            }
        }
    }

    /// Log task result, applying brief-mode truncation if configured.
    fn log_result(&self, task_id: &str, result: &ExecutionResult) {
        let Some(output) = result.output.as_deref() else {
            return;
        };

        let output = super::safe_output_for_mode(output, self.output_mode, &self.daemon_behavior);
        match self.output_mode {
            DaemonOutputMode::Summary => {
                tracing::info!(task_id = %task_id, output = %output, "task output summary");
            }
            DaemonOutputMode::Full => {
                tracing::debug!(task_id = %task_id, output = %output, "task output (full redacted)");
            }
            DaemonOutputMode::Brief => {
                tracing::info!(task_id = %task_id, output = %output, "task output (brief)");
            }
        }
    }

    /// Check if a completed task output contains a Follow-up section
    /// and, if self-prompting is enabled and rate-allowed, spawn a self-prompt.
    ///
    /// WHY: self-prompting closes the feedback loop. A prosoche check that finds
    /// something wrong can request a follow-up action without human intervention.
    /// Rate limiting ensures this never runs away.
    pub(super) fn maybe_queue_self_prompt(&mut self, task_id: &str, result: &ExecutionResult) {
        if !self.self_prompt_config.enabled {
            return;
        }

        let Some(output) = result.output.as_deref() else {
            return;
        };

        let Some(follow_up) = crate::self_prompt::extract_follow_up(output) else {
            return;
        };

        if !self.self_prompt_limiter.is_allowed(&self.nous_id) {
            tracing::info!(
                nous_id = %self.nous_id,
                task_id = %task_id,
                "self-prompt rate limited  -  skipping follow-up"
            );
            return;
        }

        let bridge = self.bridge.clone();
        let nous_id = self.nous_id.clone();
        let task_id_owned = task_id.to_owned();
        let cancel = self.shutdown.child_token();

        // WHY: spawn as a tracked task. Self-prompt execution should not block
        // the main scheduler loop, but the handle must be retained so a panic
        // surfaces as a JoinError instead of disappearing silently.
        let task_name = "self_prompt";
        self.self_prompt_tasks.spawn(
            async move {
                tracing::info!(
                    nous_id = %nous_id,
                    source_task = %task_id_owned,
                    prompt_len = follow_up.len(),
                    "dispatching self-prompt from follow-up"
                );
                let result = crate::self_prompt::execute_self_prompt_with_cancel(
                    &nous_id,
                    &follow_up,
                    bridge.as_deref(),
                    cancel,
                )
                .await;
                match result {
                    Ok(r) if r.is_success() => {
                        tracing::info!(
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            "self-prompt dispatched successfully"
                        );
                    }
                    Ok(r) => {
                        let output = r.output.as_deref().map(super::redact_task_text);
                        tracing::warn!(
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            output = ?output,
                            "self-prompt dispatch returned failure"
                        );
                        crate::metrics::record_background_failure(&nous_id, task_name);
                    }
                    Err(e) => {
                        let error = super::redact_task_text(&e.to_string());
                        tracing::error!(
                            task = task_name,
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            error = %error,
                            "background task failed"
                        );
                        crate::metrics::record_background_failure(&nous_id, task_name);
                    }
                }
            }
            .instrument(tracing::info_span!("background_task", task = task_name)),
        );

        // WHY: record the rate-limit slot only after the task has been stored
        // in the tracked set. If spawn itself fails, the slot is not consumed.
        self.self_prompt_limiter.record(&self.nous_id);
    }
}
