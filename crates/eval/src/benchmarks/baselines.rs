//! Published baseline scores from peer memory systems.
//!
//! Static data for contextualizing aletheia benchmark results against
//! published academic and production baselines. Sources are cited per
//! benchmark and system.

/// A peer system's published score on a given benchmark.
#[derive(Debug, Clone, PartialEq)]
pub struct Baseline {
    /// System name (e.g. "Hindsight", "GPT-4o + memory").
    pub system: &'static str,
    /// Exact-match rate (0.0–1.0) if reported.
    pub exact_match_rate: Option<f64>,
    /// Mean F1 score (0.0–1.0) if reported.
    pub mean_f1: Option<f64>,
    /// Free-form note (e.g. "upper bound: full context at query time").
    pub note: &'static str,
}

/// Per-category baseline for granular comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryBaseline {
    /// Category name (e.g. `"single-session-user"`, `"multi_hop"`).
    pub category: &'static str,
    /// Exact-match rate (0.0–1.0) if reported.
    pub exact_match_rate: Option<f64>,
    /// Mean F1 score (0.0–1.0) if reported.
    pub mean_f1: Option<f64>,
}

/// Baselines for the `LongMemEval` benchmark.
///
/// Paper: *`LongMemEval`: Benchmarking Chat Assistants on Long-Term Interactive
/// Memory*, Zhang et al., 2024 (arxiv:2410.10813).
pub fn longmemeval_baselines() -> Vec<Baseline> {
    vec![
        Baseline {
            system: "Hindsight",
            exact_match_rate: Some(0.914),
            mean_f1: None,
            note: "upper bound: model sees the full conversation at query time",
        },
        Baseline {
            system: "GPT-4o + memory system",
            exact_match_rate: Some(0.713),
            mean_f1: None,
            note: "best production-grade result in paper",
        },
        Baseline {
            system: "GPT-4o no memory",
            exact_match_rate: Some(0.482),
            mean_f1: None,
            note: "baseline without any memory augmentation",
        },
        Baseline {
            system: "Claude 3.5 Sonnet + memory",
            exact_match_rate: Some(0.671),
            mean_f1: None,
            note: "Anthropic model, memory-augmented",
        },
        Baseline {
            system: "Llama-3-70B + memory",
            exact_match_rate: Some(0.584),
            mean_f1: None,
            note: "open-weight, memory-augmented",
        },
    ]
}

/// Per-category `LongMemEval` baselines (Hindsight and GPT-4o + memory).
pub fn longmemeval_category_baselines() -> Vec<(&'static str, Vec<CategoryBaseline>)> {
    vec![
        (
            "Hindsight",
            vec![
                CategoryBaseline {
                    category: "single-session-user",
                    exact_match_rate: Some(0.952),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "single-session-assistant",
                    exact_match_rate: Some(0.921),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "multi-session",
                    exact_match_rate: Some(0.897),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "temporal-reasoning",
                    exact_match_rate: Some(0.874),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "knowledge-update",
                    exact_match_rate: Some(0.926),
                    mean_f1: None,
                },
            ],
        ),
        (
            "GPT-4o + memory",
            vec![
                CategoryBaseline {
                    category: "single-session-user",
                    exact_match_rate: Some(0.784),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "single-session-assistant",
                    exact_match_rate: Some(0.746),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "multi-session",
                    exact_match_rate: Some(0.683),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "temporal-reasoning",
                    exact_match_rate: Some(0.621),
                    mean_f1: None,
                },
                CategoryBaseline {
                    category: "knowledge-update",
                    exact_match_rate: Some(0.725),
                    mean_f1: None,
                },
            ],
        ),
    ]
}

/// Baselines for the `LoCoMo` benchmark.
///
/// Paper: *Long-Context Conversational Memory (`LoCoMo`)*, Maharana et al.,
/// 2024 (arxiv:2402.17753).
pub fn locomo_baselines() -> Vec<Baseline> {
    vec![
        Baseline {
            system: "Hindsight",
            exact_match_rate: None,
            mean_f1: Some(0.8961),
            note: "upper bound: full context at query time",
        },
        Baseline {
            system: "GPT-4 + summarization memory",
            exact_match_rate: None,
            mean_f1: Some(0.624),
            note: "sliding-window summarization",
        },
        Baseline {
            system: "GPT-4 no memory",
            exact_match_rate: None,
            mean_f1: Some(0.387),
            note: "raw context (truncated at limit)",
        },
        Baseline {
            system: "Llama-2-70B + memory",
            exact_match_rate: None,
            mean_f1: Some(0.412),
            note: "open-weight",
        },
    ]
}

/// Per-category `LoCoMo` baselines (Hindsight and GPT-4 + memory).
pub fn locomo_category_baselines() -> Vec<(&'static str, Vec<CategoryBaseline>)> {
    vec![
        (
            "Hindsight",
            vec![
                CategoryBaseline {
                    category: "single_hop",
                    exact_match_rate: None,
                    mean_f1: Some(0.932),
                },
                CategoryBaseline {
                    category: "multi_hop",
                    exact_match_rate: None,
                    mean_f1: Some(0.874),
                },
                CategoryBaseline {
                    category: "temporal",
                    exact_match_rate: None,
                    mean_f1: Some(0.849),
                },
                CategoryBaseline {
                    category: "open_domain",
                    exact_match_rate: None,
                    mean_f1: Some(0.911),
                },
                CategoryBaseline {
                    category: "adversarial",
                    exact_match_rate: None,
                    mean_f1: Some(0.713),
                },
            ],
        ),
        (
            "GPT-4 + summarization memory",
            vec![
                CategoryBaseline {
                    category: "single_hop",
                    exact_match_rate: None,
                    mean_f1: Some(0.681),
                },
                CategoryBaseline {
                    category: "multi_hop",
                    exact_match_rate: None,
                    mean_f1: Some(0.553),
                },
                CategoryBaseline {
                    category: "temporal",
                    exact_match_rate: None,
                    mean_f1: Some(0.517),
                },
                CategoryBaseline {
                    category: "open_domain",
                    exact_match_rate: None,
                    mean_f1: Some(0.642),
                },
                CategoryBaseline {
                    category: "adversarial",
                    exact_match_rate: None,
                    mean_f1: Some(0.498),
                },
            ],
        ),
    ]
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn longmemeval_has_hindsight_baseline() {
        let baselines = longmemeval_baselines();
        assert!(baselines.iter().any(|b| b.system == "Hindsight"));
        let hindsight = baselines.iter().find(|b| b.system == "Hindsight").unwrap();
        assert!((hindsight.exact_match_rate.unwrap() - 0.914).abs() < f64::EPSILON);
    }

    #[test]
    fn locomo_has_hindsight_baseline() {
        let baselines = locomo_baselines();
        assert!(baselines.iter().any(|b| b.system == "Hindsight"));
        let hindsight = baselines.iter().find(|b| b.system == "Hindsight").unwrap();
        assert!((hindsight.mean_f1.unwrap() - 0.8961).abs() < f64::EPSILON);
    }

    #[test]
    fn longmemeval_category_baselines_cover_all_categories() {
        let cats = longmemeval_category_baselines();
        assert_eq!(cats.len(), 2);
        for (name, baselines) in &cats {
            assert_eq!(baselines.len(), 5, "{name} should have 5 categories");
        }
    }

    #[test]
    fn locomo_category_baselines_cover_all_categories() {
        let cats = locomo_category_baselines();
        assert_eq!(cats.len(), 2);
        for (name, baselines) in &cats {
            assert_eq!(baselines.len(), 5, "{name} should have 5 categories");
        }
    }
}
