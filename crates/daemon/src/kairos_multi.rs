//! KAIROS multi-project dispatch with attention allocation and trust boundaries.
//!
//! Phase 4: extends the single-project `run_cycle` to manage a portfolio
//! of projects using dianoia's [`AttentionAllocator`] for fair scheduling
//! and the trust boundary layer for permission enforcement.
//!
//! WHY: Autonomous operation across multiple projects needs:
//! 1. Fair attention distribution (not just round-robin)
//! 2. Per-project trust boundaries (different projects, different permissions)
//! 3. Intent-aware scheduling (operator directives influence allocation)

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use aletheia_dianoia::attention::{AttentionAllocator, AttentionBudget};

use crate::bridge::DaemonBridge;
use crate::error::Result;
use crate::kairos::{KairosConfig, KairosCycleResult, run_cycle};
use crate::trust::{TrustPolicy, load_policy};

// ---------------------------------------------------------------------------
// Multi-project configuration
// ---------------------------------------------------------------------------

/// Configuration for multi-project KAIROS dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MultiProjectConfig {
    /// Per-project configurations, keyed by project name.
    pub projects: HashMap<String, ProjectEntry>,
    /// Total dispatch slots per cycle across all projects.
    pub total_slots: u32,
    /// Priority ordering (project name → rank, lower = higher priority).
    pub priorities: HashMap<String, u32>,
}

/// A project entry in the multi-project config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProjectEntry {
    /// Path to the project workspace.
    pub workspace_path: PathBuf,
    /// Nous ID for dispatching to this project.
    pub nous_id: String,
    /// Per-project KAIROS config overrides.
    #[serde(default)]
    pub config: KairosConfig,
}

impl Default for MultiProjectConfig {
    fn default() -> Self {
        Self {
            projects: HashMap::new(),
            total_slots: 10,
            priorities: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-project cycle result
// ---------------------------------------------------------------------------

/// Result of a multi-project KAIROS dispatch cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MultiCycleResult {
    /// Per-project results.
    pub results: HashMap<String, ProjectCycleResult>,
    /// Attention budget that was computed.
    pub budgets: Vec<AttentionBudget>,
    /// Projects that were skipped (disabled, trust denied, etc.).
    pub skipped_projects: Vec<(String, String)>,
}

/// Per-project result within a multi-project cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProjectCycleResult {
    /// The single-project cycle result.
    pub cycle: KairosCycleResult,
    /// Slots allocated to this project.
    pub slots_allocated: u32,
    /// Trust policy that was applied.
    pub trust_enabled: bool,
}

// ---------------------------------------------------------------------------
// Multi-project dispatch
// ---------------------------------------------------------------------------

/// Execute one multi-project KAIROS dispatch cycle.
///
/// 1. Load trust policies for all projects
/// 2. Compute attention allocation across projects
/// 3. For each project with allocated slots: run a single-project cycle
/// 4. Collect and return results
///
/// Projects without `kairos.toml` (trust disabled) are skipped.
pub async fn run_multi_cycle(
    config: &MultiProjectConfig,
    bridge: &dyn DaemonBridge,
) -> Result<MultiCycleResult> {
    let project_names: Vec<String> = config.projects.keys().cloned().collect();

    // Phase 1: load trust policies.
    let mut policies: HashMap<String, TrustPolicy> = HashMap::new();
    let mut skipped = Vec::new();

    for (name, entry) in &config.projects {
        match load_policy(&entry.workspace_path) {
            Ok(policy) => {
                if !policy.enabled {
                    skipped.push((name.clone(), "trust policy disabled".to_owned()));
                } else {
                    policies.insert(name.clone(), policy);
                }
            }
            Err(e) => {
                warn!(project = %name, error = %e, "failed to load trust policy, skipping");
                skipped.push((name.clone(), format!("policy load error: {e}")));
            }
        }
    }

    let enabled_projects: Vec<String> = policies.keys().cloned().collect();

    if enabled_projects.is_empty() {
        info!("no projects with enabled trust policies — skipping cycle");
        return Ok(MultiCycleResult {
            results: HashMap::new(),
            budgets: Vec::new(),
            skipped_projects: skipped,
        });
    }

    // Phase 2: compute attention allocation.
    let mut allocator = AttentionAllocator::new(config.total_slots);
    for (project, rank) in &config.priorities {
        allocator.set_priority(project, *rank);
    }
    let budgets = allocator.allocate(&enabled_projects);

    info!(
        projects = enabled_projects.len(),
        total_slots = config.total_slots,
        "attention allocated across projects"
    );

    // Phase 3: dispatch per project.
    let mut results = HashMap::new();

    for budget in &budgets {
        let Some(entry) = config.projects.get(&budget.project) else {
            continue;
        };

        if budget.slots == 0 {
            skipped.push((budget.project.clone(), "zero slots allocated".to_owned()));
            continue;
        }

        let mut project_config = entry.config.clone();
        project_config.enabled = true;
        project_config.workspace_path = entry.workspace_path.clone();
        project_config.nous_id = entry.nous_id.clone();
        // WHY: cap dispatches at allocated slots for this project.
        project_config.max_dispatches_per_cycle = budget.slots;

        info!(
            project = %budget.project,
            slots = budget.slots,
            share = format!("{:.1}%", budget.share * 100.0),
            "dispatching project cycle"
        );

        match run_cycle(&project_config, bridge).await {
            Ok(cycle_result) => {
                results.insert(
                    budget.project.clone(),
                    ProjectCycleResult {
                        cycle: cycle_result,
                        slots_allocated: budget.slots,
                        trust_enabled: true,
                    },
                );
            }
            Err(e) => {
                warn!(project = %budget.project, error = %e, "project cycle failed");
                skipped.push((budget.project.clone(), format!("cycle error: {e}")));
            }
        }
    }

    Ok(MultiCycleResult {
        results,
        budgets,
        skipped_projects: skipped,
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = MultiProjectConfig::default();
        assert!(config.projects.is_empty());
        assert_eq!(config.total_slots, 10);
    }

    #[test]
    fn config_roundtrip() {
        let mut config = MultiProjectConfig::default();
        config.projects.insert(
            "kanon".to_owned(),
            ProjectEntry {
                workspace_path: PathBuf::from("/home/ck/aletheia/repos/kanon"),
                nous_id: "kairos".to_owned(),
                config: KairosConfig::default(),
            },
        );
        config.priorities.insert("kanon".to_owned(), 0);

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MultiProjectConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.projects.len(), 1);
        assert_eq!(deserialized.priorities["kanon"], 0);
    }

    #[test]
    fn multi_cycle_result_structure() {
        let result = MultiCycleResult {
            results: HashMap::new(),
            budgets: Vec::new(),
            skipped_projects: vec![("test".to_owned(), "disabled".to_owned())],
        };
        assert!(result.results.is_empty());
        assert_eq!(result.skipped_projects.len(), 1);
    }
}
