//! KAIROS autonomous dispatch task.
//!
//! KAIROS (καιρός): "the right moment." A daemon task that reads directives
//! from the dianoia [`Orchestrator`] and dispatches them. Each cycle:
//! 1. Load project state from workspace
//! 2. Ask orchestrator for next directives
//! 3. For each `DispatchPlan` directive: send prompt via bridge
//! 4. Record outcomes back to orchestrator
//! 5. Persist updated state
//!
//! WHY: The daemon already has cron scheduling, watchdog, systemd integration.
//! The orchestrator already has wave-aware plan dispatch with dependency
//! resolution. KAIROS connects them — it's the loop that turns plans into
//! executed work.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::bridge::DaemonBridge;
use crate::error::Result;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the KAIROS autonomous dispatch task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KairosConfig {
    /// Whether KAIROS is enabled.
    pub enabled: bool,
    /// Path to the project workspace (contains PROJECT.json, phases/).
    pub workspace_path: PathBuf,
    /// Nous ID to use for prompt dispatch via bridge.
    pub nous_id: String,
    /// Session key for KAIROS conversations.
    pub session_key: String,
    /// Maximum plans to dispatch per cycle (prevents runaway).
    pub max_dispatches_per_cycle: u32,
    /// Whether to process only the current wave (true) or all ready plans (false).
    pub wave_gated: bool,
}

impl Default for KairosConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            workspace_path: PathBuf::from("instance"),
            nous_id: "kairos".to_owned(),
            session_key: "kairos-dispatch".to_owned(),
            max_dispatches_per_cycle: 5,
            wave_gated: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Cycle result
// ---------------------------------------------------------------------------

/// Result of a single KAIROS dispatch cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KairosCycleResult {
    /// Number of plans dispatched.
    pub dispatched: u32,
    /// Number of plans that succeeded.
    pub succeeded: u32,
    /// Number of plans that failed.
    pub failed: u32,
    /// Number of plans skipped (wave gate, budget, etc.).
    pub skipped: u32,
    /// Whether a phase verification was triggered.
    pub verification_triggered: bool,
    /// Whether a phase was blocked (failed/stuck plans).
    pub phase_blocked: bool,
}

// ---------------------------------------------------------------------------
// KAIROS cycle
// ---------------------------------------------------------------------------

/// Execute one KAIROS dispatch cycle.
///
/// Loads the project, asks the orchestrator for directives, dispatches plans
/// via the bridge, and records outcomes. Called by the daemon's cron scheduler
/// on a configurable interval.
///
/// # Errors
///
/// Returns errors from workspace I/O, bridge failures, or orchestrator state
/// issues. Individual plan failures are recorded as outcomes, not propagated.
pub async fn run_cycle(
    config: &KairosConfig,
    bridge: &dyn DaemonBridge,
) -> Result<KairosCycleResult> {
    use aletheia_dianoia::intent::IntentStore;
    use aletheia_dianoia::orchestrate::{Directive, Orchestrator, PlanOutcome};
    use aletheia_dianoia::stuck::StuckDetector;
    use aletheia_dianoia::workspace::ProjectWorkspace;

    let workspace = ProjectWorkspace::open(&config.workspace_path).map_err(|e| {
        crate::error::KairosSnafu {
            message: format!("failed to open workspace: {e}"),
        }
        .build()
    })?;

    let project = workspace.load_project().map_err(|e| {
        crate::error::KairosSnafu {
            message: format!("failed to load project: {e}"),
        }
        .build()
    })?;

    let intent_path = config.workspace_path.join("nous").join(&config.nous_id).join("intents.json");
    let intent_store = IntentStore::new(intent_path);
    let stuck_detector = StuckDetector::new(aletheia_dianoia::stuck::StuckConfig::default());

    let mut orchestrator = Orchestrator::new(project, workspace, intent_store, stuck_detector);
    let directives = orchestrator.next_directives();

    let mut result = KairosCycleResult {
        dispatched: 0,
        succeeded: 0,
        failed: 0,
        skipped: 0,
        verification_triggered: false,
        phase_blocked: false,
    };

    if directives.is_empty() {
        info!("kairos: no directives — project idle or complete");
        return Ok(result);
    }

    let mut current_wave: Option<u32> = None;

    for directive in &directives {
        match directive {
            Directive::DispatchPlan {
                plan_id,
                title,
                wave,
                ..
            } => {
                // WHY: wave gating ensures we finish one wave before starting the next.
                if config.wave_gated {
                    match current_wave {
                        None => current_wave = Some(*wave),
                        Some(w) if w != *wave => {
                            info!(
                                plan = %title,
                                wave = wave,
                                current_wave = w,
                                "kairos: skipping plan from future wave"
                            );
                            result.skipped += 1;
                            continue;
                        }
                        _ => {}
                    }
                }

                if result.dispatched >= config.max_dispatches_per_cycle {
                    info!(
                        plan = %title,
                        max = config.max_dispatches_per_cycle,
                        "kairos: max dispatches per cycle reached, deferring"
                    );
                    result.skipped += 1;
                    continue;
                }

                info!(plan = %title, wave = wave, "kairos: dispatching plan");
                result.dispatched += 1;

                let prompt = format!(
                    "Execute the following plan:\n\n## {title}\n\n\
                     This is an automated dispatch from KAIROS. \
                     Complete the plan and report the outcome."
                );

                match bridge
                    .send_prompt(&config.nous_id, &config.session_key, &prompt)
                    .await
                {
                    Ok(exec_result) if exec_result.success => {
                        info!(plan = %title, "kairos: plan succeeded");
                        result.succeeded += 1;
                        let outcome = PlanOutcome::Success {
                            achievements: exec_result
                                .output
                                .into_iter()
                                .collect(),
                        };
                        if let Err(e) = orchestrator.record_plan_outcome(*plan_id, outcome) {
                            warn!(plan = %title, error = %e, "kairos: failed to record success");
                        }
                    }
                    Ok(exec_result) => {
                        let reason = exec_result
                            .output
                            .unwrap_or_else(|| "unknown failure".to_owned());
                        warn!(plan = %title, reason = %reason, "kairos: plan failed");
                        result.failed += 1;
                        let outcome = PlanOutcome::Failed { reason };
                        if let Err(e) = orchestrator.record_plan_outcome(*plan_id, outcome) {
                            warn!(plan = %title, error = %e, "kairos: failed to record failure");
                        }
                    }
                    Err(e) => {
                        warn!(plan = %title, error = %e, "kairos: bridge error");
                        result.failed += 1;
                        let outcome = PlanOutcome::Failed {
                            reason: format!("bridge error: {e}"),
                        };
                        if let Err(e) = orchestrator.record_plan_outcome(*plan_id, outcome) {
                            warn!(plan = %title, error = %e, "kairos: failed to record error");
                        }
                    }
                }
            }
            Directive::VerifyPhase { phase_id } => {
                info!(phase_id = %phase_id, "kairos: phase verification triggered");
                result.verification_triggered = true;
            }
            Directive::PhaseBlocked {
                phase_id,
                failed_plans,
            } => {
                warn!(
                    phase_id = %phase_id,
                    failed_count = failed_plans.len(),
                    "kairos: phase blocked — escalating to operator"
                );
                result.phase_blocked = true;
            }
            Directive::SynthesizeResearch { phase_id } => {
                info!(phase_id = %phase_id, "kairos: research synthesis recommended");
            }
            // WHY: Directive is #[non_exhaustive]; future variants handled gracefully.
            _ => {
                info!("kairos: unrecognized directive, skipping");
            }
        }
    }

    info!(
        dispatched = result.dispatched,
        succeeded = result.succeeded,
        failed = result.failed,
        skipped = result.skipped,
        "kairos: cycle complete"
    );

    Ok(result)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = KairosConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_dispatches_per_cycle, 5);
        assert!(config.wave_gated);
        assert_eq!(config.nous_id, "kairos");
    }

    #[test]
    fn config_roundtrip() {
        let config = KairosConfig {
            enabled: true,
            workspace_path: PathBuf::from("/tmp/test"),
            nous_id: "test-nous".to_owned(),
            session_key: "test-key".to_owned(),
            max_dispatches_per_cycle: 10,
            wave_gated: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: KairosConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.max_dispatches_per_cycle, 10);
    }

    #[test]
    fn cycle_result_default() {
        let result = KairosCycleResult {
            dispatched: 0,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            verification_triggered: false,
            phase_blocked: false,
        };
        assert_eq!(result.dispatched, 0);
    }
}
