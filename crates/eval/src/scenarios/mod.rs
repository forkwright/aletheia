//! Scenario registry: all built-in behavioral scenarios.

mod auth;
mod conversation;
mod health;
mod nous;
mod session;

use crate::scenario::Scenario;

/// Return all built-in scenarios in execution order.
#[tracing::instrument(skip_all)]
pub(crate) fn all_scenarios() -> Vec<Box<dyn Scenario>> {
    let mut scenarios: Vec<Box<dyn Scenario>> = Vec::new();
    scenarios.extend(health::scenarios());
    scenarios.extend(auth::scenarios());
    scenarios.extend(nous::scenarios());
    scenarios.extend(session::scenarios());
    scenarios.extend(conversation::scenarios());
    scenarios
}

/// Generate a timestamped unique session key for eval scenarios.
///
/// Keys are prefixed with `eval-` followed by the given suffix and a
/// millisecond-resolution UNIX timestamp to avoid collisions between runs.
pub(super) fn unique_key(prefix: &str, suffix: &str) -> String {
    format!(
        "eval-{prefix}-{suffix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_key_contains_prefix_and_suffix() {
        let key = unique_key("session", "create");
        assert!(key.starts_with("eval-session-create-"), "key: {key}");
    }

    #[test]
    fn unique_key_is_nonempty() {
        let key = unique_key("foo", "bar");
        assert!(!key.is_empty());
    }

    #[test]
    fn all_scenarios_returns_nonempty_list() {
        let scenarios = all_scenarios();
        assert!(
            !scenarios.is_empty(),
            "scenario registry should not be empty"
        );
    }

    #[test]
    fn all_scenarios_have_unique_ids() {
        let scenarios = all_scenarios();
        let mut ids: Vec<&str> = scenarios.iter().map(|s| s.meta().id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate scenario IDs detected");
    }

    #[test]
    fn all_scenarios_have_nonempty_descriptions() {
        let scenarios = all_scenarios();
        for s in &scenarios {
            let meta = s.meta();
            assert!(
                !meta.description.is_empty(),
                "scenario {} has empty description",
                meta.id
            );
            assert!(
                !meta.category.is_empty(),
                "scenario {} has empty category",
                meta.id
            );
        }
    }

    #[test]
    fn scenario_filter_by_id_substring() {
        let all = all_scenarios();
        let filter = "health";
        let filtered: Vec<_> = all
            .into_iter()
            .filter(|s| s.meta().id.contains(filter))
            .collect();
        assert!(
            !filtered.is_empty(),
            "filter 'health' should match at least one scenario"
        );
        for s in &filtered {
            assert!(
                s.meta().id.contains(filter),
                "scenario {} should contain 'health'",
                s.meta().id
            );
        }
    }

    #[test]
    fn scenario_filter_nonexistent_returns_empty() {
        let all = all_scenarios();
        let filtered: Vec<_> = all
            .into_iter()
            .filter(|s| s.meta().id.contains("xyzzy-nonexistent"))
            .collect();
        assert!(filtered.is_empty());
    }
}
