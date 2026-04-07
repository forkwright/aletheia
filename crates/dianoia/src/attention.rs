//! Cross-project attention allocation.
//!
//! Sits above individual project [`Orchestrator`](crate::orchestrate::Orchestrator)s
//! and decides how to distribute dispatch capacity across the portfolio.
//! Without this, autonomous operation defaults to round-robin or
//! last-touched-wins — neither reflects actual priority.
//!
//! WHY: KAIROS needs to autonomously decide which project to work on.
//! Each project's PromptDag operates independently. This module provides
//! the cross-project arbitration layer that makes scheduling decisions
//! before dispatch.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Attention budget
// ---------------------------------------------------------------------------

/// Attention allocation for a single project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AttentionBudget {
    /// Project identifier.
    pub project: String,
    /// Fraction of total dispatch capacity allocated (0.0–1.0).
    pub share: f64,
    /// Number of dispatch slots allocated this cycle.
    pub slots: u32,
    /// Why this allocation was made.
    pub reason: AllocationReason,
}

/// Why a project received its attention share.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AllocationReason {
    /// Operator-assigned priority (from intent system).
    Priority {
        /// The priority rank (lower = higher priority).
        rank: u32,
    },
    /// Velocity decay: project hasn't had activity recently.
    VelocityDecay {
        /// Days since last dispatch.
        days_idle: u32,
    },
    /// Dependency gate: this project blocks others.
    DependencyGate {
        /// Projects waiting on this one.
        blocked_projects: Vec<String>,
    },
    /// Fair share: no special signal, distribute evenly.
    FairShare,
    /// Starvation prevention: project hasn't received attention in N cycles.
    StarvationPrevention {
        /// Cycles since last allocation.
        cycles_starved: u32,
    },
}

// ---------------------------------------------------------------------------
// Allocator
// ---------------------------------------------------------------------------

/// Cross-project attention allocator.
///
/// Given a set of projects with their priorities, velocities, and dependency
/// relationships, produces an [`AttentionBudget`] for each project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AttentionAllocator {
    /// Total dispatch slots available per cycle.
    pub total_slots: u32,
    /// Minimum slots any project receives (starvation prevention).
    pub min_slots: u32,
    /// Maximum fraction any single project can receive.
    pub max_share: f64,
    /// Priority ordering (project name → rank, lower = higher priority).
    pub priorities: HashMap<String, u32>,
    /// Cycles since each project last received attention.
    pub starvation_counters: HashMap<String, u32>,
}

impl Default for AttentionAllocator {
    fn default() -> Self {
        Self {
            total_slots: 10,
            min_slots: 1,
            max_share: 0.5,
            priorities: HashMap::new(),
            starvation_counters: HashMap::new(),
        }
    }
}

impl AttentionAllocator {
    /// Create an allocator with the given total slot budget.
    #[must_use]
    pub fn new(total_slots: u32) -> Self {
        Self {
            total_slots,
            ..Self::default()
        }
    }

    /// Set the priority rank for a project (lower = higher priority).
    pub fn set_priority(&mut self, project: impl Into<String>, rank: u32) {
        self.priorities.insert(project.into(), rank);
    }

    /// Allocate attention across the given projects.
    ///
    /// Algorithm:
    /// 1. Assign minimum slots to all projects (starvation prevention)
    /// 2. Distribute remaining slots by weighted priority
    /// 3. Cap each project at `max_share` of total
    /// 4. Redistribute excess from capped projects
    /// 5. Update starvation counters
    #[must_use]
    pub fn allocate(&mut self, projects: &[String]) -> Vec<AttentionBudget> {
        if projects.is_empty() {
            return Vec::new();
        }

        let n = projects.len() as u32;
        let min_total = self.min_slots.saturating_mul(n);
        let effective_total = self.total_slots.max(min_total);

        // Phase 1: everyone gets minimum.
        let mut allocations: HashMap<&str, u32> = projects
            .iter()
            .map(|p| (p.as_str(), self.min_slots))
            .collect();

        let remaining = effective_total.saturating_sub(min_total);

        if remaining > 0 {
            // Phase 2: distribute by priority weight.
            // Lower rank = higher weight. Unranked projects get rank = n.
            let weights: Vec<(&str, f64)> = projects
                .iter()
                .map(|p| {
                    let rank = self.priorities.get(p.as_str()).copied().unwrap_or(n);
                    let starvation = self
                        .starvation_counters
                        .get(p.as_str())
                        .copied()
                        .unwrap_or(0);
                    // WHY: inverse rank + starvation bonus. Starved projects
                    // get boosted to prevent indefinite neglect.
                    let weight = 1.0 / (1.0 + rank as f64) + (starvation as f64 * 0.1);
                    (p.as_str(), weight)
                })
                .collect();

            let total_weight: f64 = weights.iter().map(|(_, w)| w).sum();

            if total_weight > 0.0 {
                for (project, weight) in &weights {
                    let share = weight / total_weight;
                    let extra = (share * remaining as f64).round() as u32;
                    *allocations.get_mut(project).expect("project in map") += extra;
                }
            }

            // Phase 3: cap at max_share.
            let max_slots = (self.max_share * effective_total as f64).ceil() as u32;
            let mut excess = 0_u32;
            for slots in allocations.values_mut() {
                if *slots > max_slots {
                    excess += *slots - max_slots;
                    *slots = max_slots;
                }
            }

            // Phase 4: redistribute excess evenly to under-cap projects.
            if excess > 0 {
                let under_cap: Vec<&str> = allocations
                    .iter()
                    .filter(|(_, s)| **s < max_slots)
                    .map(|(&p, _)| p)
                    .collect();
                if !under_cap.is_empty() {
                    let per_project = excess / under_cap.len() as u32;
                    for p in under_cap {
                        *allocations.get_mut(p).expect("project in map") += per_project;
                    }
                }
            }
        }

        // Phase 5: update starvation counters.
        for project in projects {
            let slots = allocations.get(project.as_str()).copied().unwrap_or(0);
            if slots > self.min_slots {
                // Project received attention — reset starvation.
                self.starvation_counters.insert(project.clone(), 0);
            } else {
                // Project only got minimum — increment starvation.
                let counter = self.starvation_counters.entry(project.clone()).or_insert(0);
                *counter += 1;
            }
        }

        // Build results.
        projects
            .iter()
            .map(|p| {
                let slots = allocations.get(p.as_str()).copied().unwrap_or(0);
                let share = if effective_total > 0 {
                    slots as f64 / effective_total as f64
                } else {
                    0.0
                };

                let reason = if let Some(&rank) = self.priorities.get(p.as_str()) {
                    AllocationReason::Priority { rank }
                } else if self
                    .starvation_counters
                    .get(p.as_str())
                    .copied()
                    .unwrap_or(0)
                    > 3
                {
                    AllocationReason::StarvationPrevention {
                        cycles_starved: self.starvation_counters[p.as_str()],
                    }
                } else {
                    AllocationReason::FairShare
                };

                AttentionBudget {
                    project: p.clone(),
                    share,
                    slots,
                    reason,
                }
            })
            .collect()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn empty_projects() {
        let mut alloc = AttentionAllocator::new(10);
        let budgets = alloc.allocate(&[]);
        assert!(budgets.is_empty());
    }

    #[test]
    fn single_project_gets_all_slots() {
        let mut alloc = AttentionAllocator::new(10);
        let budgets = alloc.allocate(&["kanon".to_owned()]);
        assert_eq!(budgets.len(), 1);
        assert!(budgets[0].slots >= 1);
    }

    #[test]
    fn higher_priority_gets_more() {
        let mut alloc = AttentionAllocator::new(20);
        alloc.set_priority("kanon", 0); // highest
        alloc.set_priority("aletheia", 1);
        alloc.set_priority("harmonia", 2);

        let budgets = alloc.allocate(&[
            "kanon".to_owned(),
            "aletheia".to_owned(),
            "harmonia".to_owned(),
        ]);

        let kanon = budgets.iter().find(|b| b.project == "kanon").unwrap();
        let harmonia = budgets.iter().find(|b| b.project == "harmonia").unwrap();
        assert!(
            kanon.slots >= harmonia.slots,
            "kanon ({}) should get >= harmonia ({})",
            kanon.slots,
            harmonia.slots
        );
    }

    #[test]
    fn starvation_prevention() {
        let mut alloc = AttentionAllocator::new(10);
        alloc.set_priority("kanon", 0);
        alloc.starvation_counters.insert("harmonia".to_owned(), 10);

        let budgets = alloc.allocate(&["kanon".to_owned(), "harmonia".to_owned()]);

        let harmonia = budgets.iter().find(|b| b.project == "harmonia").unwrap();
        // Starved project should get more than just min_slots.
        assert!(harmonia.slots >= alloc.min_slots);
    }

    #[test]
    fn max_share_cap() {
        let mut alloc = AttentionAllocator::new(10);
        alloc.max_share = 0.4;
        alloc.set_priority("kanon", 0);

        let budgets = alloc.allocate(&[
            "kanon".to_owned(),
            "aletheia".to_owned(),
            "harmonia".to_owned(),
        ]);

        let kanon = budgets.iter().find(|b| b.project == "kanon").unwrap();
        assert!(kanon.slots <= 4, "kanon ({}) should be capped at 4", kanon.slots);
    }

    #[test]
    fn all_projects_get_minimum() {
        let mut alloc = AttentionAllocator::new(10);
        alloc.min_slots = 2;

        let budgets = alloc.allocate(&[
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
        ]);

        for budget in &budgets {
            assert!(budget.slots >= 2, "{} got {} slots", budget.project, budget.slots);
        }
    }
}
