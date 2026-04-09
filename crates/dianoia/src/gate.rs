//! Phase boundary gates: conditions that must be met before transitioning between phases.
//!
//! Gates are metadata/flag-based — they do not run processes. External systems (CI, reviewers,
//! orchestrators) set flags on [`PhaseGate`]; `evaluate_gate` reads them and returns a verdict.

use serde::{Deserialize, Serialize};

use crate::state::ProjectState;

/// A condition that must be satisfied before leaving a phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GateCondition {
    /// All tests in the CI suite are currently passing.
    TestsPassing,
    /// No lint errors or warnings are reported.
    LintClean,
    /// Relevant documentation has been updated.
    DocsUpdated,
    /// At least one reviewer has approved the work.
    ReviewApproved,
    /// A named custom condition (description is the key checked against [`PhaseGate::satisfied`]).
    Custom(String),
}

impl GateCondition {
    /// Stable key used to look up this condition in [`PhaseGate::satisfied`].
    fn key(&self) -> &str {
        match self {
            Self::TestsPassing => "tests_passing",
            Self::LintClean => "lint_clean",
            Self::DocsUpdated => "docs_updated",
            Self::ReviewApproved => "review_approved",
            Self::Custom(k) => k.as_str(),
        }
    }
}

/// The outcome of evaluating a [`PhaseGate`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GateResult {
    /// All conditions are satisfied; the transition may proceed.
    Pass,
    /// One or more conditions are not satisfied.
    Fail(Vec<String>),
}

impl GateResult {
    /// Returns `true` if the gate passed.
    #[must_use]
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// A set of conditions guarding a phase transition.
///
/// External systems write to [`satisfied`][Self::satisfied] using the condition keys returned
/// by [`GateCondition::key`]. Call [`evaluate_gate`] to check the gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "PhaseGateRaw")]
pub struct PhaseGate {
    /// The project state this gate guards (the state being *left*).
    pub from: ProjectState,
    /// Conditions that must all be satisfied before advancing.
    pub conditions: Vec<GateCondition>,
    /// Condition keys that have been marked satisfied by external systems.
    pub satisfied: Vec<String>,
}

/// Raw deserialization type for [`PhaseGate`].
#[derive(Debug, Clone, Deserialize)]
struct PhaseGateRaw {
    from: ProjectState,
    conditions: Vec<GateCondition>,
    satisfied: Vec<String>,
}

impl TryFrom<PhaseGateRaw> for PhaseGate {
    type Error = std::convert::Infallible;

    fn try_from(raw: PhaseGateRaw) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            from: raw.from,
            conditions: raw.conditions,
            satisfied: raw.satisfied,
        })
    }
}

impl PhaseGate {
    /// Create a gate for the given `from` state with the specified conditions.
    #[must_use]
    pub fn new(from: ProjectState, conditions: Vec<GateCondition>) -> Self {
        Self {
            from,
            conditions,
            satisfied: Vec::new(),
        }
    }

    /// Mark a condition as satisfied by its key string.
    pub fn mark_satisfied(&mut self, key: impl Into<String>) {
        let key = key.into();
        if !self.satisfied.contains(&key) {
            self.satisfied.push(key);
        }
    }
}

/// Evaluate all conditions in a [`PhaseGate`].
///
/// Returns [`GateResult::Pass`] if every condition is satisfied, or
/// [`GateResult::Fail`] with the keys of the failing conditions.
#[must_use]
pub fn evaluate_gate(gate: &PhaseGate) -> GateResult {
    let failing: Vec<String> = gate
        .conditions
        .iter()
        .filter(|c| !gate.satisfied.contains(&c.key().to_owned()))
        .map(|c| c.key().to_owned())
        .collect();

    if failing.is_empty() {
        GateResult::Pass
    } else {
        GateResult::Fail(failing)
    }
}

/// Build the default gate for a given project state.
///
/// Returns `None` for states that have no gate (no advancement restriction).
#[must_use]
pub fn default_gate(from: &ProjectState) -> Option<PhaseGate> {
    match from {
        ProjectState::Planning => Some(PhaseGate::new(
            ProjectState::Planning,
            vec![GateCondition::Custom(
                "acceptance_criteria_defined".to_owned(),
            )],
        )),
        ProjectState::Executing => Some(PhaseGate::new(
            ProjectState::Executing,
            vec![
                GateCondition::Custom("code_committed".to_owned()),
                GateCondition::TestsPassing,
            ],
        )),
        ProjectState::Verifying => Some(PhaseGate::new(
            ProjectState::Verifying,
            vec![GateCondition::ReviewApproved],
        )),
        ProjectState::Created
        | ProjectState::Questioning
        | ProjectState::Researching
        | ProjectState::Scoping
        | ProjectState::Discussing
        | ProjectState::Complete
        | ProjectState::Abandoned
        | ProjectState::Paused { .. } => None,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    // ── GateCondition ──────────────────────────────────────────────────────────

    #[test]
    fn gate_condition_keys_are_stable() {
        assert_eq!(GateCondition::TestsPassing.key(), "tests_passing");
        assert_eq!(GateCondition::LintClean.key(), "lint_clean");
        assert_eq!(GateCondition::DocsUpdated.key(), "docs_updated");
        assert_eq!(GateCondition::ReviewApproved.key(), "review_approved");
        assert_eq!(GateCondition::Custom("foo".into()).key(), "foo");
    }

    // ── evaluate_gate ──────────────────────────────────────────────────────────

    #[test]
    fn empty_conditions_always_passes() {
        let gate = PhaseGate::new(ProjectState::Planning, vec![]);
        assert_eq!(evaluate_gate(&gate), GateResult::Pass);
    }

    #[test]
    fn all_conditions_satisfied_passes() {
        let mut gate = PhaseGate::new(
            ProjectState::Executing,
            vec![GateCondition::TestsPassing, GateCondition::LintClean],
        );
        gate.mark_satisfied("tests_passing");
        gate.mark_satisfied("lint_clean");
        assert_eq!(evaluate_gate(&gate), GateResult::Pass);
    }

    #[test]
    fn unsatisfied_condition_fails() {
        let gate = PhaseGate::new(
            ProjectState::Executing,
            vec![GateCondition::TestsPassing, GateCondition::LintClean],
        );
        let result = evaluate_gate(&gate);
        assert_eq!(
            result,
            GateResult::Fail(vec![
                "tests_passing".to_owned(),
                "lint_clean".to_owned()
            ])
        );
    }

    #[test]
    fn partially_satisfied_reports_missing() {
        let mut gate = PhaseGate::new(
            ProjectState::Executing,
            vec![GateCondition::TestsPassing, GateCondition::LintClean],
        );
        gate.mark_satisfied("tests_passing");
        let result = evaluate_gate(&gate);
        assert_eq!(
            result,
            GateResult::Fail(vec!["lint_clean".to_owned()])
        );
    }

    #[test]
    fn custom_condition_satisfied_by_key() {
        let mut gate = PhaseGate::new(
            ProjectState::Planning,
            vec![GateCondition::Custom("acceptance_criteria_defined".into())],
        );
        gate.mark_satisfied("acceptance_criteria_defined");
        assert_eq!(evaluate_gate(&gate), GateResult::Pass);
    }

    #[test]
    fn custom_condition_fails_when_not_satisfied() {
        let gate = PhaseGate::new(
            ProjectState::Planning,
            vec![GateCondition::Custom("acceptance_criteria_defined".into())],
        );
        assert_eq!(
            evaluate_gate(&gate),
            GateResult::Fail(vec!["acceptance_criteria_defined".to_owned()])
        );
    }

    #[test]
    fn mark_satisfied_is_idempotent() {
        let mut gate = PhaseGate::new(
            ProjectState::Verifying,
            vec![GateCondition::ReviewApproved],
        );
        gate.mark_satisfied("review_approved");
        gate.mark_satisfied("review_approved");
        assert_eq!(gate.satisfied.len(), 1, "duplicate mark_satisfied should not add duplicates");
        assert_eq!(evaluate_gate(&gate), GateResult::Pass);
    }

    // ── default_gate ───────────────────────────────────────────────────────────

    #[test]
    fn default_gate_planning_requires_acceptance_criteria() {
        let gate = default_gate(&ProjectState::Planning).unwrap();
        assert_eq!(gate.from, ProjectState::Planning);
        assert_eq!(
            gate.conditions,
            vec![GateCondition::Custom("acceptance_criteria_defined".into())]
        );
        assert!(gate.satisfied.is_empty());
    }

    #[test]
    fn default_gate_executing_requires_code_committed_and_tests() {
        let gate = default_gate(&ProjectState::Executing).unwrap();
        assert_eq!(gate.from, ProjectState::Executing);
        assert!(gate.conditions.contains(&GateCondition::Custom("code_committed".into())));
        assert!(gate.conditions.contains(&GateCondition::TestsPassing));
    }

    #[test]
    fn default_gate_verifying_requires_review() {
        let gate = default_gate(&ProjectState::Verifying).unwrap();
        assert_eq!(gate.from, ProjectState::Verifying);
        assert_eq!(gate.conditions, vec![GateCondition::ReviewApproved]);
    }

    #[test]
    fn default_gate_none_for_non_gated_states() {
        for state in [
            ProjectState::Created,
            ProjectState::Questioning,
            ProjectState::Researching,
            ProjectState::Scoping,
            ProjectState::Discussing,
            ProjectState::Complete,
            ProjectState::Abandoned,
        ] {
            assert!(
                default_gate(&state).is_none(),
                "expected no default gate for {state:?}"
            );
        }
    }

    // ── GateResult helpers ─────────────────────────────────────────────────────

    #[test]
    fn gate_result_is_pass() {
        assert!(GateResult::Pass.is_pass());
        assert!(!GateResult::Fail(vec!["foo".into()]).is_pass());
    }

    // ── serde roundtrip ────────────────────────────────────────────────────────

    #[test]
    fn phase_gate_serde_roundtrip() {
        let mut gate = PhaseGate::new(
            ProjectState::Executing,
            vec![GateCondition::TestsPassing, GateCondition::Custom("ci_green".into())],
        );
        gate.mark_satisfied("tests_passing");

        let json = serde_json::to_string(&gate).unwrap();
        let back: PhaseGate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.conditions, gate.conditions);
        assert_eq!(back.satisfied, gate.satisfied);
    }

    #[test]
    fn gate_condition_serde_roundtrip() {
        let conditions = vec![
            GateCondition::TestsPassing,
            GateCondition::LintClean,
            GateCondition::DocsUpdated,
            GateCondition::ReviewApproved,
            GateCondition::Custom("my_check".into()),
        ];
        for c in &conditions {
            let json = serde_json::to_string(c).unwrap();
            let back: GateCondition = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, c, "roundtrip failed for {c:?}");
        }
    }
}
