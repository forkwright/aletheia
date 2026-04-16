//! Self-tuning feedback loop configuration.

use serde::{Deserialize, Serialize};

/// Self-tuning feedback loop configuration.
///
/// Controls whether agents may propose parameter changes and the evidence
/// thresholds required before a proposal is accepted. The global `enabled`
/// flag is a kill switch; individual agents may opt out via
/// `AgentBehaviorDefaults::tuning_eligible`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TuningConfig {
    /// Global kill switch for self-tuning. Default: false.
    ///
    /// When false, no tuning proposals are generated or applied regardless
    /// of per-agent settings.
    pub enabled: bool,
    /// Maximum parameter changes applied per prosoche cycle. Default: 3.
    ///
    /// Limits the blast radius of a single tuning cycle. Additional proposals
    /// beyond this limit are deferred to the next cycle.
    pub max_changes_per_cycle: u32,
    /// Minimum metric observations required before a proposal is generated. Default: 20.
    ///
    /// Below this threshold, evidence is considered insufficient and the
    /// proposal is rejected.
    pub evidence_min_samples: u32,
    /// Significance threshold in standard deviations. Default: 1.5.
    ///
    /// The observed delta must exceed `significance_threshold * stddev` for
    /// the evidence to be considered statistically significant.
    pub significance_threshold: f64,
}

impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_changes_per_cycle: 3,
            evidence_min_samples: 20,
            significance_threshold: 1.5,
        }
    }
}
