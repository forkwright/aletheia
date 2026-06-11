//! Executable plans within a phase.

use std::collections::{HashMap, HashSet};

use koina::ulid::Ulid;
use serde::{Deserialize, Serialize};
use snafu::ensure;

use crate::error::{self, Result};
use crate::research::{FindingStatus, ResearchOutput};

/// Default maximum planning iterations.
///
/// Callers with access to the resolved taxis config should use
/// `AgentBehaviorDefaults::planning_max_iterations` instead of this fallback.
pub const DEFAULT_MAX_ITERATIONS: u32 = 10;

/// An executable plan within a phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "PlanRaw")]
pub struct Plan {
    /// Unique plan identifier.
    pub id: Ulid,
    /// Short title for the plan.
    pub title: String,
    /// Detailed description of what this plan accomplishes.
    pub description: String,
    /// Execution wave: plans in the same wave can run in parallel.
    pub wave: u32,
    /// Plan IDs that must complete before this plan can start.
    pub depends_on: Vec<Ulid>,
    /// Current lifecycle state.
    pub state: PlanState,
    /// Maximum allowed iterations before the plan is marked stuck.
    pub max_iterations: u32,
    /// Number of iterations executed so far.
    pub iterations: u32,
    /// Blockers discovered during execution.
    pub blockers: Vec<Blocker>,
    /// Notable accomplishments recorded during execution.
    pub achievements: Vec<String>,
}

/// Raw deserialization type for [`Plan`].
#[derive(Debug, Clone, Deserialize)]
struct PlanRaw {
    id: Ulid,
    title: String,
    description: String,
    wave: u32,
    depends_on: Vec<Ulid>,
    state: PlanState,
    max_iterations: u32,
    iterations: u32,
    blockers: Vec<Blocker>,
    achievements: Vec<String>,
}

impl From<PlanRaw> for Plan {
    fn from(raw: PlanRaw) -> Self {
        Self {
            id: raw.id,
            title: raw.title,
            description: raw.description,
            wave: raw.wave,
            depends_on: raw.depends_on,
            state: raw.state,
            max_iterations: raw.max_iterations,
            iterations: raw.iterations,
            blockers: raw.blockers,
            achievements: raw.achievements,
        }
    }
}

/// Plan lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlanState {
    /// Plan has not been evaluated for readiness yet.
    Pending,
    /// All dependencies are satisfied and the plan can execute.
    Ready,
    /// Plan is currently being executed.
    Executing,
    /// Plan finished successfully.
    Complete,
    /// Plan execution failed.
    Failed,
    /// Plan was intentionally skipped (e.g., no longer relevant).
    Skipped,
    /// Plan exceeded its iteration limit without completing.
    Stuck,
}

/// A blocker discovered during plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blocker {
    /// What is blocking progress.
    pub description: String,
    /// The plan that is blocked.
    pub plan_id: Ulid,
    /// When the blocker was identified.
    pub detected_at: jiff::Timestamp,
}

impl Plan {
    /// Create a new plan in the `Pending` state with default iteration limits.
    #[must_use]
    pub fn new(title: String, description: String, wave: u32) -> Self {
        Self {
            id: Ulid::new(),
            title,
            description,
            wave,
            depends_on: Vec::new(),
            state: PlanState::Pending,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            iterations: 0,
            blockers: Vec::new(),
            achievements: Vec::new(),
        }
    }

    /// Create plans from a [`ResearchOutput`].
    ///
    /// One plan is generated per finding with [`FindingStatus::Complete`] or
    /// [`FindingStatus::Partial`] status.  Failed or timed-out findings are
    /// skipped.  Wave is set to `0` because research-derived plans have not
    /// yet been ordered.
    #[must_use]
    pub fn from_research(research: &ResearchOutput) -> Vec<Self> {
        research
            .findings
            .iter()
            .filter(|f| matches!(f.status, FindingStatus::Complete | FindingStatus::Partial))
            .map(|f| Self::new(f.domain.heading().into(), f.content.clone(), 0))
            .collect()
    }

    /// Create a new plan from a completed plan, treating it as a template.
    ///
    /// Copies title and description, resets state to [`PlanState::Pending`],
    /// clears blockers, achievements, and dependencies, and uses the supplied
    /// `next_wave`.
    #[must_use]
    pub fn from_template(completed: &Plan, next_wave: u32) -> Self {
        Self {
            id: Ulid::new(),
            title: completed.title.clone(),
            description: completed.description.clone(),
            wave: next_wave,
            depends_on: Vec::new(),
            state: PlanState::Pending,
            max_iterations: completed.max_iterations,
            iterations: 0,
            blockers: Vec::new(),
            achievements: Vec::new(),
        }
    }

    /// Set the maximum iteration limit (builder pattern).
    ///
    /// Use to override [`DEFAULT_MAX_ITERATIONS`] with a value from
    /// `taxis::config::AgentBehaviorDefaults::planning_max_iterations`.
    #[must_use]
    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    /// Set dependencies for this plan, validating against all plans in the set
    /// to detect circular dependencies.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::CircularDependency`] if adding these dependencies
    /// would create a cycle in the dependency graph.
    pub fn set_depends_on(&mut self, deps: Vec<Ulid>, all_plans: &[Plan]) -> Result<()> {
        self.depends_on = deps;
        detect_cycles(all_plans, self)
    }

    /// Check if all dependencies are satisfied given completed plan IDs.
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "WIP: plan execution lifecycle"))]
    pub(crate) fn is_ready(&self, completed: &[Ulid]) -> bool {
        self.depends_on.iter().all(|dep| completed.contains(dep))
    }

    /// Record an iteration. Returns `Err` if `max_iterations` exceeded.
    #[cfg_attr(not(test), expect(dead_code, reason = "WIP: plan execution lifecycle"))]
    pub(crate) fn record_iteration(&mut self) -> Result<()> {
        self.iterations += 1;
        if self.iterations > self.max_iterations {
            self.state = PlanState::Stuck;
            return error::PlanStuckSnafu {
                plan_id: self.id.to_string(),
                iterations: self.iterations,
            }
            .fail();
        }
        Ok(())
    }

    /// Mark as stuck with a blocker.
    #[cfg_attr(not(test), expect(dead_code, reason = "WIP: planning orchestration"))]
    pub(crate) fn mark_stuck(&mut self, blocker: Blocker) {
        self.state = PlanState::Stuck;
        self.blockers.push(blocker);
    }
}

/// Detect circular dependencies in the plan graph.
///
/// Builds an adjacency list from `all_plans` (with `updated_plan` overriding any
/// plan with the same ID), then runs DFS-based cycle detection. Returns `Ok(())`
/// if the graph is acyclic, or an error describing the first cycle found.
///
/// WHY separate function: called from `Plan::set_depends_on` and available for
/// bulk validation of an entire phase's plan set.
pub fn detect_cycles(all_plans: &[Plan], updated_plan: &Plan) -> Result<()> {
    let mut adj: HashMap<Ulid, Vec<Ulid>> = HashMap::new();
    let mut titles: HashMap<Ulid, &str> = HashMap::new();

    for plan in all_plans {
        if plan.id == updated_plan.id {
            adj.insert(updated_plan.id, updated_plan.depends_on.clone());
            titles.insert(updated_plan.id, &updated_plan.title);
        } else {
            adj.insert(plan.id, plan.depends_on.clone());
            titles.insert(plan.id, &plan.title);
        }
    }
    // WHY: updated_plan may not be in all_plans yet (a new plan being added);
    // include it so its dependencies participate in cycle detection.
    adj.entry(updated_plan.id)
        .or_insert_with(|| updated_plan.depends_on.clone());
    titles.entry(updated_plan.id).or_insert(&updated_plan.title);

    let mut visited = HashSet::new();
    let mut on_stack = HashSet::new();
    let mut path = Vec::new();

    let ids: Vec<Ulid> = adj.keys().copied().collect();
    for &start in &ids {
        if !visited.contains(&start)
            && dfs_find_cycle(start, &adj, &mut visited, &mut on_stack, &mut path)
        {
            // INVARIANT: `path` contains exactly the cycle when dfs_find_cycle
            // returns true; format it with titles for the error message.
            let cycle_str = path
                .iter()
                .map(|id| titles.get(id).copied().unwrap_or("unknown"))
                .collect::<Vec<_>>()
                .join(" -> ");
            ensure!(false, error::CircularDependencySnafu { cycle: cycle_str });
        }
    }

    Ok(())
}

/// DFS helper: returns `true` if a cycle is found, populating `path` with the cycle.
fn dfs_find_cycle(
    node: Ulid,
    adj: &HashMap<Ulid, Vec<Ulid>>,
    visited: &mut HashSet<Ulid>,
    on_stack: &mut HashSet<Ulid>,
    path: &mut Vec<Ulid>,
) -> bool {
    visited.insert(node);
    on_stack.insert(node);
    path.push(node);

    if let Some(deps) = adj.get(&node) {
        for &dep in deps {
            if !visited.contains(&dep) {
                if dfs_find_cycle(dep, adj, visited, on_stack, path) {
                    return true;
                }
            } else if on_stack.contains(&dep) {
                // WHY: trim path to start at the cycle entry point so the
                // reported cycle excludes the acyclic prefix.
                if let Some(pos) = path.iter().position(|&id| id == dep) {
                    path.drain(..pos);
                }
                path.push(dep); // Close the cycle: A -> B -> A
                return true;
            }
        }
    }

    on_stack.remove(&node);
    path.pop();
    false
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices are asserted correct by len checks above"
)]
mod tests {
    use super::*;

    #[test]
    fn dependency_check() {
        let dep1 = Ulid::new();
        let dep2 = Ulid::new();
        let mut plan = Plan::new("task".into(), "do a thing".into(), 1);
        plan.depends_on = vec![dep1, dep2];

        assert!(!plan.is_ready(&[]));
        assert!(!plan.is_ready(&[dep1]));
        assert!(plan.is_ready(&[dep1, dep2]));
        assert!(plan.is_ready(&[dep2, dep1, Ulid::new()]));
    }

    #[test]
    fn iteration_tracking_and_stuck() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        plan.max_iterations = 3;

        plan.record_iteration().unwrap(); // 1
        plan.record_iteration().unwrap(); // 2
        plan.record_iteration().unwrap(); // 3
        let result = plan.record_iteration(); // 4 > 3
        assert!(result.is_err());
        assert_eq!(plan.state, PlanState::Stuck);
    }

    #[test]
    fn wave_ordering() {
        let first = Plan::new("first".into(), "wave 1".into(), 1);
        let second = Plan::new("second".into(), "wave 1".into(), 1);
        let mut dependent = Plan::new("dependent".into(), "wave 2".into(), 2);
        dependent.depends_on = vec![first.id, second.id];

        assert!(first.is_ready(&[]));
        assert!(second.is_ready(&[]));

        assert!(!dependent.is_ready(&[]));
        assert!(!dependent.is_ready(&[first.id]));
        assert!(dependent.is_ready(&[first.id, second.id]));
    }

    #[test]
    fn new_plan_defaults() {
        let plan = Plan::new("task".into(), "desc".into(), 1);
        assert_eq!(plan.title, "task");
        assert_eq!(plan.description, "desc");
        assert_eq!(plan.wave, 1);
        assert_eq!(plan.state, PlanState::Pending);
        assert_eq!(plan.iterations, 0);
        assert_eq!(plan.max_iterations, 10);
        assert!(plan.depends_on.is_empty());
        assert!(plan.blockers.is_empty());
        assert!(plan.achievements.is_empty());
    }

    #[test]
    fn is_ready_no_deps() {
        let plan = Plan::new("task".into(), "desc".into(), 1);
        assert!(plan.is_ready(&[]));
        assert!(plan.is_ready(&[Ulid::new()]));
    }

    #[test]
    fn mark_stuck_sets_state_and_blocker() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        let blocker = Blocker {
            description: "blocked on review".into(),
            plan_id: plan.id,
            detected_at: jiff::Timestamp::now(),
        };
        plan.mark_stuck(blocker);
        assert_eq!(plan.state, PlanState::Stuck);
        assert_eq!(plan.blockers.len(), 1);
        assert_eq!(plan.blockers[0].description, "blocked on review");
    }

    #[test]
    fn record_iteration_within_limit() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        plan.max_iterations = 5;
        for _ in 0..5 {
            assert!(plan.record_iteration().is_ok());
        }
        assert_eq!(plan.iterations, 5);
        assert_eq!(plan.state, PlanState::Pending);
    }

    #[test]
    fn blocker_creation() {
        let plan_id = Ulid::new();
        let now = jiff::Timestamp::now();
        let blocker = Blocker {
            description: "needs API design decision".into(),
            plan_id,
            detected_at: now,
        };
        assert_eq!(blocker.description, "needs API design decision");
        assert_eq!(blocker.plan_id, plan_id);
        assert_eq!(blocker.detected_at, now);
    }

    #[test]
    fn plan_state_serde_roundtrip() {
        let states = [
            PlanState::Pending,
            PlanState::Ready,
            PlanState::Executing,
            PlanState::Complete,
            PlanState::Failed,
            PlanState::Skipped,
            PlanState::Stuck,
        ];
        for state in &states {
            let json = serde_json::to_string(state).unwrap();
            let back: PlanState = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, state, "roundtrip failed for {state:?}");
        }
    }

    #[test]
    fn plan_serde_roundtrip_preserves_fields() {
        let mut plan = Plan::new("test task".into(), "do the thing".into(), 2);
        plan.depends_on = vec![Ulid::new()];
        plan.achievements = vec!["milestone reached".into()];

        let json = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, plan.id);
        assert_eq!(back.title, "test task");
        assert_eq!(back.description, "do the thing");
        assert_eq!(back.wave, 2);
        assert_eq!(back.depends_on, plan.depends_on);
        assert_eq!(back.achievements, vec!["milestone reached"]);
        assert_eq!(back.max_iterations, 10);
    }

    #[test]
    fn mark_stuck_then_add_another_blocker() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        let blocker1 = Blocker {
            description: "first blocker".into(),
            plan_id: plan.id,
            detected_at: jiff::Timestamp::now(),
        };
        plan.mark_stuck(blocker1);
        assert_eq!(plan.state, PlanState::Stuck);
        assert_eq!(plan.blockers.len(), 1);

        plan.blockers.push(Blocker {
            description: "second blocker".into(),
            plan_id: plan.id,
            detected_at: jiff::Timestamp::now(),
        });
        assert_eq!(plan.blockers.len(), 2);
    }

    // ── Cycle detection ───────────────────────────────────────────────────────

    #[test]
    fn no_cycle_linear_chain() {
        let mut a = Plan::new("A".into(), "plan A".into(), 1);
        let mut b = Plan::new("B".into(), "plan B".into(), 1);
        let c = Plan::new("C".into(), "plan C".into(), 1);

        b.depends_on = vec![a.id];
        a.depends_on = vec![];

        let all = vec![a.clone(), b.clone(), c.clone()];
        // Set C to depend on B — no cycle: A <- B <- C
        let mut c_mut = c;
        let result = c_mut.set_depends_on(vec![b.id], &all);
        assert!(result.is_ok(), "linear chain should have no cycle");
    }

    #[test]
    fn direct_cycle_detected() {
        let a = Plan::new("A".into(), "plan A".into(), 1);
        let mut b = Plan::new("B".into(), "plan B".into(), 1);
        b.depends_on = vec![a.id];

        let all = vec![a.clone(), b.clone()];

        // Set A to depend on B — creates A -> B -> A cycle.
        let mut a_mut = a;
        let result = a_mut.set_depends_on(vec![b.id], &all);
        assert!(result.is_err(), "A -> B -> A should be detected as a cycle");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains('A') && err.contains('B'),
            "error should name plans in cycle, got: {err}"
        );
    }

    #[test]
    fn transitive_cycle_detected() {
        let a = Plan::new("A".into(), "plan A".into(), 1);
        let mut b = Plan::new("B".into(), "plan B".into(), 1);
        let mut c = Plan::new("C".into(), "plan C".into(), 1);

        b.depends_on = vec![a.id];
        c.depends_on = vec![b.id];

        let all = vec![a.clone(), b.clone(), c.clone()];

        // Set A to depend on C — creates A -> C -> B -> A cycle.
        let mut a_mut = a;
        let result = a_mut.set_depends_on(vec![c.id], &all);
        assert!(
            result.is_err(),
            "A -> C -> B -> A should be detected as a cycle"
        );
    }

    #[test]
    fn no_cycle_with_shared_dependency() {
        let a = Plan::new("A".into(), "plan A".into(), 1);
        let mut b = Plan::new("B".into(), "plan B".into(), 1);
        let mut c = Plan::new("C".into(), "plan C".into(), 1);

        // B and C both depend on A — diamond but no cycle.
        b.depends_on = vec![a.id];
        c.depends_on = vec![a.id];

        let all = vec![a, b, c.clone()];
        let result = detect_cycles(&all, &c);
        assert!(result.is_ok(), "diamond dependency should not be a cycle");
    }

    #[test]
    fn self_dependency_detected() {
        let a = Plan::new("A".into(), "plan A".into(), 1);
        let all = vec![a.clone()];

        let mut a_mut = a;
        let result = a_mut.set_depends_on(vec![a_mut.id], &all);
        assert!(
            result.is_err(),
            "self-dependency should be detected as a cycle"
        );
    }

    // ── Alternative constructors ──────────────────────────────────────────────

    #[test]
    fn from_research_creates_plan_per_complete_finding() {
        use crate::research::{ResearchDomain, ResearchFinding, ResearchOutput};

        let research = ResearchOutput {
            findings: vec![
                ResearchFinding {
                    domain: ResearchDomain::Stack,
                    content: "Use Rust with Tokio".into(),
                    status: FindingStatus::Complete,
                },
                ResearchFinding {
                    domain: ResearchDomain::Features,
                    content: "Need auth and billing".into(),
                    status: FindingStatus::Partial,
                },
                ResearchFinding {
                    domain: ResearchDomain::Architecture,
                    content: "Microservices".into(),
                    status: FindingStatus::Failed,
                },
                ResearchFinding {
                    domain: ResearchDomain::Pitfalls,
                    content: "Deadlocks possible".into(),
                    status: FindingStatus::TimedOut,
                },
            ],
            markdown: String::new(),
        };

        let plans = Plan::from_research(&research);
        assert_eq!(
            plans.len(),
            2,
            "only Complete and Partial findings become plans"
        );

        assert_eq!(plans[0].title, "Stack");
        assert_eq!(plans[0].description, "Use Rust with Tokio");
        assert_eq!(plans[0].wave, 0);
        assert_eq!(plans[0].state, PlanState::Pending);

        assert_eq!(plans[1].title, "Features");
        assert_eq!(plans[1].description, "Need auth and billing");
        assert_eq!(plans[1].wave, 0);
        assert_eq!(plans[1].state, PlanState::Pending);
    }

    #[test]
    fn from_research_empty_when_all_findings_fail() {
        use crate::research::{ResearchDomain, ResearchFinding, ResearchOutput};

        let research = ResearchOutput {
            findings: vec![ResearchFinding {
                domain: ResearchDomain::Stack,
                content: "error".into(),
                status: FindingStatus::Failed,
            }],
            markdown: String::new(),
        };

        let plans = Plan::from_research(&research);
        assert!(plans.is_empty());
    }

    #[test]
    fn from_template_copies_title_description_and_increments_wave() {
        let completed = Plan::new("Original".into(), "Original desc".into(), 2);
        let next = Plan::from_template(&completed, 3);

        assert_eq!(next.title, "Original");
        assert_eq!(next.description, "Original desc");
        assert_eq!(next.wave, 3);
        assert_eq!(next.state, PlanState::Pending);
        assert_eq!(next.iterations, 0);
        assert!(next.depends_on.is_empty());
        assert!(next.blockers.is_empty());
        assert!(next.achievements.is_empty());
        assert_eq!(next.max_iterations, completed.max_iterations);
        assert_ne!(next.id, completed.id, "template plan must get a new ID");
    }

    #[test]
    fn from_template_preserves_max_iterations() {
        let mut completed = Plan::new("Task".into(), "Desc".into(), 1);
        completed.max_iterations = 42;
        completed.iterations = 5;
        completed.achievements = vec!["done".into()];
        completed.state = PlanState::Complete;

        let next = Plan::from_template(&completed, 2);
        assert_eq!(next.max_iterations, 42);
        assert_eq!(next.iterations, 0);
        assert!(next.achievements.is_empty());
        assert_eq!(next.state, PlanState::Pending);
    }
}
