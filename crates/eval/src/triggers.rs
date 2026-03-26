//! Configurable evaluation trigger scheduling.

use serde::{Deserialize, Serialize};

/// Schedule frequency for evaluation triggers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TriggerSchedule {
    /// Run on every deployment.
    OnDeploy,
    /// Run daily.
    Daily,
    /// Run weekly.
    Weekly,
    /// Custom cron expression.
    Cron(String),
}

/// Configuration for a single evaluation trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalTrigger {
    /// Scenario ID pattern to match (substring filter).
    pub scenario_pattern: String,
    /// When to run this evaluation.
    pub schedule: TriggerSchedule,
    /// Whether this trigger is active.
    pub enabled: bool,
}

/// Top-level trigger configuration for scheduling evaluations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// List of evaluation triggers.
    pub triggers: Vec<EvalTrigger>,
}

impl TriggerConfig {
    /// Default trigger configuration with recommended schedules.
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            triggers: vec![
                EvalTrigger {
                    scenario_pattern: "recall".to_owned(),
                    schedule: TriggerSchedule::Daily,
                    enabled: true,
                },
                EvalTrigger {
                    scenario_pattern: "sycophancy".to_owned(),
                    schedule: TriggerSchedule::Weekly,
                    enabled: true,
                },
                EvalTrigger {
                    scenario_pattern: "adversarial".to_owned(),
                    schedule: TriggerSchedule::Weekly,
                    enabled: true,
                },
                EvalTrigger {
                    scenario_pattern: "self-assessment".to_owned(),
                    schedule: TriggerSchedule::Weekly,
                    enabled: true,
                },
                EvalTrigger {
                    scenario_pattern: "health".to_owned(),
                    schedule: TriggerSchedule::OnDeploy,
                    enabled: true,
                },
            ],
        }
    }

    /// Return triggers that match a given scenario ID.
    #[must_use]
    pub(crate) fn matching_triggers(&self, scenario_id: &str) -> Vec<&EvalTrigger> {
        self.triggers
            .iter()
            .filter(|t| t.enabled && scenario_id.contains(&t.scenario_pattern))
            .collect()
    }

    /// Return all enabled trigger patterns.
    #[must_use]
    pub(crate) fn enabled_patterns(&self) -> Vec<&str> {
        self.triggers
            .iter()
            .filter(|t| t.enabled)
            .map(|t| t.scenario_pattern.as_str())
            .collect()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_all_categories() {
        let config = TriggerConfig::default_config();
        assert!(
            !config.triggers.is_empty(),
            "default config should have triggers"
        );

        let patterns: Vec<&str> = config
            .triggers
            .iter()
            .map(|t| t.scenario_pattern.as_str())
            .collect();
        assert!(
            patterns.contains(&"recall"),
            "should include recall trigger"
        );
        assert!(
            patterns.contains(&"sycophancy"),
            "should include sycophancy trigger"
        );
        assert!(
            patterns.contains(&"adversarial"),
            "should include adversarial trigger"
        );
        assert!(
            patterns.contains(&"self-assessment"),
            "should include self-assessment trigger"
        );
        assert!(
            patterns.contains(&"health"),
            "should include health trigger"
        );
    }

    #[test]
    fn default_config_all_enabled() {
        let config = TriggerConfig::default_config();
        for trigger in &config.triggers {
            assert!(
                trigger.enabled,
                "default trigger {} should be enabled",
                trigger.scenario_pattern
            );
        }
    }

    #[test]
    fn matching_triggers_filters_by_pattern() {
        let config = TriggerConfig::default_config();
        let matches = config.matching_triggers("recall-at-k-benchmark");
        assert_eq!(
            matches.len(),
            1,
            "recall scenario should match recall trigger"
        );
        assert_eq!(
            matches[0].scenario_pattern, "recall",
            "matched trigger should be recall"
        );
    }

    #[test]
    fn matching_triggers_returns_empty_for_no_match() {
        let config = TriggerConfig::default_config();
        let matches = config.matching_triggers("xyzzy-nonexistent");
        assert!(
            matches.is_empty(),
            "nonexistent scenario should match no triggers"
        );
    }

    #[test]
    fn matching_triggers_excludes_disabled() {
        let config = TriggerConfig {
            triggers: vec![EvalTrigger {
                scenario_pattern: "health".to_owned(),
                schedule: TriggerSchedule::Daily,
                enabled: false,
            }],
        };
        let matches = config.matching_triggers("health-ok");
        assert!(matches.is_empty(), "disabled trigger should not match");
    }

    #[test]
    fn enabled_patterns_filters_disabled() {
        let config = TriggerConfig {
            triggers: vec![
                EvalTrigger {
                    scenario_pattern: "a".to_owned(),
                    schedule: TriggerSchedule::Daily,
                    enabled: true,
                },
                EvalTrigger {
                    scenario_pattern: "b".to_owned(),
                    schedule: TriggerSchedule::Weekly,
                    enabled: false,
                },
            ],
        };
        let patterns = config.enabled_patterns();
        assert_eq!(patterns, vec!["a"], "should only include enabled patterns");
    }

    #[test]
    fn trigger_schedule_serialization_roundtrip() {
        let schedules = vec![
            TriggerSchedule::OnDeploy,
            TriggerSchedule::Daily,
            TriggerSchedule::Weekly,
            TriggerSchedule::Cron("0 0 * * *".to_owned()),
        ];
        for schedule in &schedules {
            let json = serde_json::to_string(schedule).expect("should serialize");
            let back: TriggerSchedule = serde_json::from_str(&json).expect("should deserialize");
            assert_eq!(&back, schedule, "roundtrip should preserve schedule");
        }
    }

    #[test]
    fn trigger_config_serialization_roundtrip() {
        let config = TriggerConfig::default_config();
        let json = serde_json::to_string_pretty(&config).expect("should serialize");
        let back: TriggerConfig = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(
            config.triggers.len(),
            back.triggers.len(),
            "roundtrip should preserve trigger count"
        );
    }

    #[test]
    fn empty_config_matches_nothing() {
        let config = TriggerConfig::default();
        assert!(config.triggers.is_empty(), "default config should be empty");
        let matches = config.matching_triggers("anything");
        assert!(matches.is_empty(), "empty config matches nothing");
    }
}
