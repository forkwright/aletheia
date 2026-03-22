//! Cognitive evaluation framework for measuring agent behavioral quality.

pub(crate) mod adversarial;
pub(crate) mod recall;
pub(crate) mod self_assessment;
pub(crate) mod sycophancy;

use crate::scenario::Scenario;

/// Return all cognitive evaluation scenarios.
pub(crate) fn cognitive_scenarios() -> Vec<Box<dyn Scenario>> {
    let mut scenarios: Vec<Box<dyn Scenario>> = Vec::new();
    scenarios.extend(recall::scenarios());
    scenarios.extend(sycophancy::scenarios());
    scenarios.extend(adversarial::scenarios());
    scenarios.extend(self_assessment::scenarios());
    scenarios
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cognitive_scenarios_nonempty() {
        let scenarios = cognitive_scenarios();
        assert!(
            !scenarios.is_empty(),
            "cognitive scenario registry should not be empty"
        );
    }

    #[test]
    fn cognitive_scenarios_have_unique_ids() {
        let scenarios = cognitive_scenarios();
        let mut ids: Vec<&str> = scenarios.iter().map(|s| s.meta().id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            total,
            "duplicate cognitive scenario IDs detected"
        );
    }

    #[test]
    fn all_cognitive_scenarios_in_cognitive_category() {
        let scenarios = cognitive_scenarios();
        for s in &scenarios {
            assert_eq!(
                s.meta().category,
                "cognitive",
                "scenario {} should be in 'cognitive' category",
                s.meta().id
            );
        }
    }
}
