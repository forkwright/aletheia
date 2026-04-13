//! In-flight task monitoring: completion polling, timeout detection, self-prompting.

use tracing::Instrument;

use crate::schedule::TaskStatus;

use super::{DaemonOutputMode, ExecutionResult, TaskRunner, truncate_output};

impl TaskRunner {
    /// Get status of all registered tasks.
    ///
    /// # Complexity
    ///
    /// O(t) where t is the number of registered tasks.
    pub fn status(&self) -> Vec<TaskStatus> {
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
                    "hung task detected  -  cancelling (exceeded 2x timeout)"
                );
                in_flight.handle.abort();

                self.in_flight.remove(&task_id);
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
                        self.record_task_completion(&task_id, duration);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            task_id = %task_id,
                            error = %e,
                            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                            "spawned task failed"
                        );
                        self.record_task_failure(&task_id, &e.to_string());
                    }
                    Err(e) => {
                        tracing::warn!(
                            task_id = %task_id,
                            error = %e,
                            "spawned task panicked or was cancelled"
                        );
                        self.record_task_failure(&task_id, &e.to_string());
                    }
                }
            }
        }
    }

    /// Log task result, applying brief-mode truncation if configured.
    fn log_result(&self, task_id: &str, result: &ExecutionResult) {
        let Some(output) = result.output.as_deref() else {
            return;
        };

        match self.output_mode {
            DaemonOutputMode::Full => {
                tracing::debug!(task_id = %task_id, output = %output, "task output");
            }
            DaemonOutputMode::Brief => {
                let truncated = truncate_output(output, None, None);
                tracing::info!(task_id = %task_id, output = %truncated, "task output (brief)");
            }
        }
    }

    /// Check if a completed task's output contains a `## Follow-up` section
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

        self.self_prompt_limiter.record(&self.nous_id);

        let bridge = self.bridge.clone();
        let nous_id = self.nous_id.clone();
        let task_id_owned = task_id.to_owned();

        // WHY: spawn as a detached task. Self-prompt execution should not block
        // the main scheduler loop. Failures are logged but do not affect the
        // originating task's status.
        let task_name = "self_prompt";
        tokio::spawn(
            async move {
                tracing::info!(
                    nous_id = %nous_id,
                    source_task = %task_id_owned,
                    prompt_len = follow_up.len(),
                    "dispatching self-prompt from follow-up"
                );
                let result = crate::self_prompt::execute_self_prompt(
                    &nous_id,
                    &follow_up,
                    bridge.as_deref(),
                )
                .await;
                match result {
                    Ok(r) if r.success => {
                        tracing::info!(
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            "self-prompt dispatched successfully"
                        );
                    }
                    Ok(r) => {
                        tracing::warn!(
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            output = ?r.output,
                            "self-prompt dispatch returned failure"
                        );
                        crate::metrics::record_background_failure(&nous_id, task_name);
                    }
                    Err(e) => {
                        tracing::error!(
                            task = task_name,
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            error = %e,
                            "background task failed"
                        );
                        crate::metrics::record_background_failure(&nous_id, task_name);
                    }
                }
            }
            .instrument(tracing::info_span!("background_task", task = task_name)),
        );
    }
}
