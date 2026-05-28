// kanon:ignore RUST/file-too-long — cohesive budget prediction module; 808 lines is only marginally over limit and splitting would fragment tightly coupled allocation logic
// WHY: Predictive budget allocation from prompt characteristics. Analyzes
// prompt complexity, blast radius, domain, and historical data to estimate
// turn budgets before dispatch. Prevents both over-allocation (wasted budget)
// and under-allocation (premature termination).

use crate::prompt::PromptSpec;

/// Complexity classification of a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Complexity {
    /// Simple, mechanical changes (lint fixes, formatting, typos).
    Low,
    /// Moderate changes requiring some design (feature additions, tests).
    Medium,
    /// Architectural or large-scale redesign work.
    High,
}

/// Detailed result of complexity classification.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ClassificationDetail {
    /// Assigned complexity tier.
    pub complexity: Complexity,
    /// Normalized score (0–10+) used for fine-grained adjustment.
    pub score: u32,
    /// Keywords or signals that influenced the classification.
    pub signals: Vec<String>,
}

/// Predicted budget allocation for a prompt.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PredictedBudget {
    /// Recommended turn budget for initial session run.
    initial_turns: u32,
    /// Recommended turn budget for each resume attempt.
    resume_turns: u32,
    /// Confidence level in the prediction (0.0–1.0).
    confidence: f64,
    /// Factors that contributed to the prediction.
    factors: PredictionFactors,
    /// Human-readable explanation of the prediction.
    explanation: String,
}

impl PredictedBudget {
    /// Recommended turn budget for initial session run.
    #[must_use]
    pub fn initial_turns(&self) -> u32 {
        self.initial_turns
    }

    /// Recommended turn budget for each resume attempt.
    #[must_use]
    pub fn resume_turns(&self) -> u32 {
        self.resume_turns
    }

    /// Confidence level in the prediction (0.0–1.0).
    #[must_use]
    pub fn confidence(&self) -> f64 {
        self.confidence
    }

    /// Factors that contributed to the prediction.
    #[must_use]
    pub fn factors(&self) -> &PredictionFactors {
        &self.factors
    }

    /// Human-readable explanation of the prediction.
    #[must_use]
    pub fn explanation(&self) -> &str {
        &self.explanation
    }
}

/// Factors contributing to a budget prediction.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PredictionFactors {
    /// Complexity classification of the prompt.
    pub complexity: Complexity,
    /// Number of files in the blast radius.
    pub blast_radius_count: usize,
    /// Domain/type of the prompt (feat, fix, refactor, etc.).
    pub domain: String,
    /// Complexity score from classification.
    pub complexity_score: u32,
    /// Estimated based on historical data.
    pub historical_based: bool,
}

/// Base turn allocations by complexity tier.
//
// WHY: Derived from analysis of historical dispatch data. Low-complexity
// mechanical work averages 30–50 turns. Medium complexity averages 80–120.
// High complexity (architecture, redesign) often requires 150+ turns.
const BASE_TURNS_LOW: u32 = 50;
const BASE_TURNS_MEDIUM: u32 = 120;
const BASE_TURNS_HIGH: u32 = 200;

/// Resume turn allocations by complexity tier.
//
// WHY: Resume attempts should be shorter since they're finishing work,
// not starting fresh. Ratio of 0.5 for low, 0.6 for medium, 0.75 for high
// reflects that complex work needs more runway on resume.
const RESUME_RATIO_LOW: f64 = 0.5;
const RESUME_RATIO_MEDIUM: f64 = 0.6;
const RESUME_RATIO_HIGH: f64 = 0.75;

/// Blast radius thresholds for budget adjustment.
const BLAST_SMALL_THRESHOLD: usize = 3;
const BLAST_LARGE_THRESHOLD: usize = 10;

/// Absolute minimum turns to prevent immediate termination.
const ABSOLUTE_MIN_TURNS: u32 = 20;

/// Default maximum turns when no caller-supplied ceiling is given.
const DEFAULT_MAX_TURNS: u32 = 500;

/// Classify prompt complexity from body and description text.
//
// WHY: Keyword-based heuristic replacing phronesis AST analysis. Scans
// for high-complexity signals (architecture, redesign, multi-crate) and
// low-complexity signals (lint, fmt, typo, docs) to assign a tier.
#[must_use]
pub fn classify_with_detail(text: &str) -> ClassificationDetail {
    let lowered = text.to_lowercase();
    let mut signals = Vec::new();
    let mut score: u32 = 3; // start neutral

    // High-complexity keywords.
    let high_signals = [
        "architecture",
        "redesign",
        "refactor",
        "multi-crate",
        "subsystem",
        "orchestrat",
        "pipeline",
        "protocol",
        "distributed",
        "concurrency",
        "async",
        "actor",
        "state machine",
    ];
    for sig in &high_signals {
        if lowered.contains(sig) {
            signals.push(sig.to_string());
            score += 2;
        }
    }

    // Medium-complexity keywords.
    let medium_signals = [
        "feature",
        "implement",
        "endpoint",
        "handler",
        "middleware",
        "database",
        "migration",
        "api",
        "service",
        "component",
    ];
    for sig in &medium_signals {
        if lowered.contains(sig) {
            signals.push(sig.to_string());
            score += 1;
        }
    }

    // Low-complexity keywords (reduce score).
    let low_signals = [
        "lint",
        "clippy",
        "fmt",
        "format",
        "typo",
        "spelling",
        "docstring",
        "comment",
        "style",
        "chore",
        "tweak",
        "cosmetic",
    ];
    for sig in &low_signals {
        if lowered.contains(sig) {
            signals.push(sig.to_string());
            score = score.saturating_sub(2);
        }
    }

    // Penalise very short prompts (less context = less complexity).
    if text.len() < 100 {
        score = score.saturating_sub(1);
    }

    let complexity = if score >= 6 {
        Complexity::High
    } else if score >= 3 {
        Complexity::Medium
    } else {
        Complexity::Low
    };

    ClassificationDetail {
        complexity,
        score,
        signals,
    }
}

/// Predict the budget allocation for a prompt based on its characteristics.
//
// Analyses complexity classification, blast radius size, and domain/type
// to estimate turn budgets before dispatch.
//
// `domain` is optional; when `None` the domain is inferred from the prompt
// description and body.
//
// `max_turns` is an optional caller-supplied ceiling. When `None` a default
// absolute ceiling of 500 is used.
#[must_use]
pub fn predict_budget(
    prompt: &PromptSpec,
    domain: Option<&str>,
    max_turns: Option<u32>,
) -> PredictedBudget {
    let text = format!("{} {}", prompt.description, prompt.body);
    let classification = classify_with_detail(&text);
    let blast_count = prompt.blast_radius.len();

    let inferred_domain = infer_domain(domain, &text);

    // WHY: Base allocation determined by complexity tier.
    let (base_initial, base_resume) = base_allocation_for_complexity(classification.complexity);

    // WHY: Adjust based on blast radius size. More files = more work.
    let blast_adjustment = blast_radius_adjustment(blast_count);

    // WHY: Domain-specific adjustments. Research needs more turns for
    // synthesis; fixes with narrow scope need fewer.
    let domain_adjustment = domain_adjustment(&inferred_domain, blast_count);

    // WHY: Complexity score from classification provides fine-grained tuning.
    let score_adjustment = complexity_score_adjustment(classification.score);

    // Calculate final allocations.
    let ceiling = max_turns.unwrap_or(DEFAULT_MAX_TURNS);
    let initial_turns = apply_adjustments(
        base_initial,
        &[blast_adjustment, domain_adjustment, score_adjustment],
        ceiling,
    );
    let resume_turns = apply_adjustments(
        base_resume,
        &[blast_adjustment, domain_adjustment, score_adjustment],
        ceiling,
    );

    // WHY: Confidence reflects how many signals we have. More signals
    // (complexity, blast radius, domain patterns) = higher confidence.
    let confidence = calculate_confidence(&classification, blast_count, &inferred_domain);

    let factors = PredictionFactors {
        complexity: classification.complexity,
        blast_radius_count: blast_count,
        domain: inferred_domain.clone(),
        complexity_score: classification.score,
        historical_based: false,
    };

    let explanation = format_explanation(
        classification.complexity,
        blast_count,
        &inferred_domain,
        initial_turns,
        resume_turns,
        confidence,
    );

    PredictedBudget {
        initial_turns,
        resume_turns,
        confidence,
        factors,
        explanation,
    }
}

/// Predict budget for a prompt using historical data when available.
//
// Blends historical average with characteristic-based prediction.
#[must_use]
#[expect(clippy::float_arithmetic, reason = "weighted blending of predictions")]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "historical avg is non-negative and fits in u32 after clamping; base*ratio is bounded by DEFAULT_MAX_TURNS"
)]
pub fn predict_budget_with_historical(
    prompt: &PromptSpec,
    domain: Option<&str>,
    max_turns: Option<u32>,
    historical_avg: Option<f64>,
) -> PredictedBudget {
    let prediction = predict_budget(prompt, domain, max_turns);

    if let Some(avg_turns) = historical_avg {
        // WHY: Blend historical average with characteristic-based prediction.
        // Historical data gets 30% weight, characteristics get 70%.
        // kanon:ignore RUST/as-cast — historical_avg is a positive f64 bounded by realistic turn counts; truncation is safe and intended
        let historical_initial = avg_turns as u32;
        // kanon:ignore RUST/as-cast — blended values are bounded by clamp below and fit safely in u32
        let blended_initial = (f64::from(prediction.initial_turns) * 0.7
            + f64::from(historical_initial) * 0.3) as u32;
        // kanon:ignore RUST/as-cast — resume ratio produces positive u32 bounded by BASE_TURNS_HIGH*RESUME_RATIO_HIGH < 200
        let blended_resume = (f64::from(blended_initial) * 0.6) as u32;
        let ceiling = max_turns.unwrap_or(DEFAULT_MAX_TURNS);
        let clamped_initial = blended_initial.clamp(ABSOLUTE_MIN_TURNS, ceiling);
        let clamped_resume = blended_resume.clamp(ABSOLUTE_MIN_TURNS, ceiling);

        return PredictedBudget {
            initial_turns: clamped_initial,
            resume_turns: clamped_resume,
            confidence: (prediction.confidence + 0.1).min(0.95),
            factors: PredictionFactors {
                historical_based: true,
                ..prediction.factors
            },
            explanation: format!(
                "{} (blended with historical avg of {:.0} turns)",
                prediction.explanation, avg_turns
            ),
        };
    }

    prediction
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Infer domain from explicit value or prompt text.
fn infer_domain(explicit: Option<&str>, text: &str) -> String {
    if let Some(d) = explicit {
        return d.to_owned();
    }

    let lowered = text.to_lowercase();
    let keywords: [(&str, &str); 8] = [
        ("research", "research"),
        ("refactor", "refactor"),
        ("fix", "fix"),
        ("feat", "feat"),
        ("feature", "feat"),
        ("style", "style"),
        ("docs", "docs"),
        ("test", "test"),
    ];

    for (kw, domain) in &keywords {
        if lowered.contains(kw) {
            return (*domain).to_owned();
        }
    }

    "general".to_owned()
}

/// Get base allocation for a complexity tier.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "resume ratio produces positive u32 bounded by BASE_TURNS_HIGH*RESUME_RATIO_HIGH < 200"
)]
#[expect(clippy::float_arithmetic, reason = "multiplication with f64 ratio")]
fn base_allocation_for_complexity(complexity: Complexity) -> (u32, u32) {
    match complexity {
        Complexity::Low => (
            BASE_TURNS_LOW,
            // kanon:ignore RUST/as-cast — resume ratio produces positive u32 bounded by BASE_TURNS_LOW*RESUME_RATIO_LOW < 200
            (f64::from(BASE_TURNS_LOW) * RESUME_RATIO_LOW) as u32,
        ),
        Complexity::Medium => (
            BASE_TURNS_MEDIUM,
            // kanon:ignore RUST/as-cast — resume ratio produces positive u32 bounded by BASE_TURNS_MEDIUM*RESUME_RATIO_MEDIUM < 200
            (f64::from(BASE_TURNS_MEDIUM) * RESUME_RATIO_MEDIUM) as u32,
        ),
        Complexity::High => (
            BASE_TURNS_HIGH,
            // kanon:ignore RUST/as-cast — resume ratio produces positive u32 bounded by BASE_TURNS_HIGH*RESUME_RATIO_HIGH < 200
            (f64::from(BASE_TURNS_HIGH) * RESUME_RATIO_HIGH) as u32,
        ),
    }
}

/// Calculate adjustment factor based on blast radius size.
//
// WHY: Small blast radius (<3 files) reduces budget by 10%.
// Large blast radius (>10 files) increases budget by 20%.
fn blast_radius_adjustment(blast_count: usize) -> f64 {
    if blast_count <= BLAST_SMALL_THRESHOLD {
        0.9
    } else if blast_count >= BLAST_LARGE_THRESHOLD {
        1.2
    } else {
        1.0
    }
}

/// Calculate adjustment factor based on domain/type.
//
// WHY: Research prompts need more turns for synthesis.
// Style/docs fixes need fewer. Refactors need more for careful changes.
fn domain_adjustment(domain: &str, blast_count: usize) -> f64 {
    match domain {
        "research" => 1.3,
        "refactor" => 1.2,
        "feat" if blast_count > 5 => 1.15,
        "fix" if blast_count <= 2 => 0.85,
        "style" | "docs" | "chore" => 0.8,
        "test" => 0.9,
        _ => 1.0,
    }
}

/// Calculate adjustment based on complexity classification score.
//
// WHY: Fine-tune within complexity tiers. Score of 6 is the high threshold.
fn complexity_score_adjustment(score: u32) -> f64 {
    if score >= 10 {
        1.25
    } else if score >= 6 {
        1.1
    } else if score <= 1 {
        0.9
    } else {
        1.0
    }
}

/// Apply multiple adjustment factors to a base value.
//
// WHY: Clamp to reasonable bounds. Minimum 20 turns prevents immediate
// termination; maximum respects caller config (default 500).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamp produces non-negative u32 bounded by max_turns which fits in u32"
)]
#[expect(
    clippy::float_arithmetic,
    reason = "multiplication for budget adjustment"
)]
fn apply_adjustments(base: u32, adjustments: &[f64], max_turns: u32) -> u32 {
    let factor: f64 = adjustments.iter().product();
    let adjusted = f64::from(base) * factor;
    // kanon:ignore RUST/as-cast — clamp produces non-negative f64 bounded by max_turns which fits in u32
    adjusted.clamp(f64::from(ABSOLUTE_MIN_TURNS), f64::from(max_turns)) as u32
}

/// Calculate confidence in the prediction.
//
// WHY: More signals = higher confidence. Complexity classification provides
// the base; blast radius and domain add confidence if present.
#[expect(clippy::float_arithmetic, reason = "confidence calculation with f64")]
fn calculate_confidence(
    classification: &ClassificationDetail,
    blast_count: usize,
    domain: &str,
) -> f64 {
    let mut confidence: f64 = 0.6;

    if !classification.signals.is_empty() {
        confidence += 0.15;
    }

    if blast_count > 0 {
        confidence += 0.1;
    }

    if matches!(
        domain,
        "feat" | "fix" | "refactor" | "research" | "style" | "docs" | "test" | "chore"
    ) {
        confidence += 0.1;
    }

    if classification.score >= 6 && !classification.signals.is_empty() {
        confidence += 0.05;
    }

    confidence.min(0.95)
}

/// Format a human-readable explanation of the prediction.
//
// Example style: "Single-file mechanical change in known domain → 30 turns
// initial, 15 resume (high confidence)"
fn format_explanation(
    complexity: Complexity,
    blast_count: usize,
    domain: &str,
    initial_turns: u32,
    resume_turns: u32,
    confidence: f64,
) -> String {
    let complexity_str = match complexity {
        Complexity::Low => "mechanical change",
        Complexity::Medium => "moderate complexity change",
        Complexity::High => "complex architectural change",
    };

    let blast_str = if blast_count == 0 {
        "unspecified blast radius".to_owned()
    } else if blast_count == 1 {
        "single-file".to_owned()
    } else {
        format!("{blast_count}-file")
    };

    let confidence_str = if confidence >= 0.85 {
        "high confidence"
    } else if confidence >= 0.7 {
        "medium confidence"
    } else {
        "low confidence"
    };

    format!(
        "{blast_str} {complexity_str} in {domain} domain → {initial_turns} turns initial, {resume_turns} resume ({confidence_str})"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_prompt(
        number: u32,
        description: &str,
        body: &str,
        blast_radius: Vec<String>,
    ) -> PromptSpec {
        PromptSpec {
            number,
            description: description.to_owned(),
            depends_on: Vec::new(),
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: Vec::new(),
            blast_radius,
            body: body.to_owned(),
            prompt_components: None,
        }
    }

    #[test]
    fn low_complexity_mechanical_work() {
        let prompt = make_prompt(
            1,
            "Fix lint",
            "Fix clippy warning and run cargo fmt.",
            Vec::new(),
        );
        let budget = predict_budget(&prompt, None, None);

        assert_eq!(budget.factors().complexity, Complexity::Low);
        assert!(
            budget.initial_turns() <= 60,
            "low complexity should get <= 60 turns, got {}",
            budget.initial_turns()
        );
        assert!(budget.confidence() >= 0.6);
    }

    #[test]
    fn high_complexity_architecture() {
        let prompt = make_prompt(
            2,
            "Architecture Redesign",
            "Implement new subsystem with multi-crate changes.",
            Vec::new(),
        );
        let budget = predict_budget(&prompt, None, None);

        assert_eq!(budget.factors().complexity, Complexity::High);
        assert!(
            budget.initial_turns() >= 150,
            "high complexity should get >= 150 turns, got {}",
            budget.initial_turns()
        );
    }

    #[test]
    fn blast_radius_affects_budget() {
        let small = make_prompt(
            10,
            "Feature",
            "Implement new feature.",
            vec!["src/a.rs".to_owned()],
        );
        let large = make_prompt(
            11,
            "Feature",
            "Implement new feature.",
            vec![
                "src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs", "src/f.rs", "src/g.rs",
                "src/h.rs", "src/i.rs", "src/j.rs", "src/k.rs",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        let small_budget = predict_budget(&small, None, None);
        let large_budget = predict_budget(&large, None, None);

        assert!(
            large_budget.initial_turns() > small_budget.initial_turns(),
            "large blast radius should get higher budget"
        );
    }

    #[test]
    fn research_prompts_get_higher_budget() {
        let research = make_prompt(
            20,
            "Research API patterns",
            "Research and document API patterns.",
            Vec::new(),
        );
        let fix = make_prompt(
            21,
            "Fix bug",
            "Fix the crash in src/bug.rs.",
            vec!["src/bug.rs".to_owned()],
        );

        let research_budget = predict_budget(&research, Some("research"), None);
        let fix_budget = predict_budget(&fix, Some("fix"), None);

        assert!(
            research_budget.initial_turns() > fix_budget.initial_turns(),
            "research should get higher budget than simple fix"
        );
    }

    #[test]
    fn domain_adjustments_applied() {
        let style = make_prompt(30, "Style fix", "Apply style fixes.", Vec::new());
        let refactor = make_prompt(
            31,
            "Refactor",
            "Refactor lib.rs.",
            vec!["src/lib.rs".to_owned()],
        );

        let style_budget = predict_budget(&style, Some("style"), None);
        let refactor_budget = predict_budget(&refactor, Some("refactor"), None);

        assert!(
            style_budget.initial_turns() < refactor_budget.initial_turns(),
            "style should get lower budget than refactor"
        );
    }

    #[test]
    fn confidence_increases_with_signals() {
        let minimal = make_prompt(40, "Something", "Do something.", Vec::new());
        let detailed = make_prompt(
            41,
            "Architecture change",
            "Multi-crate refactoring with subsystem redesign.",
            vec![
                "src/a.rs".to_owned(),
                "src/b.rs".to_owned(),
                "src/c.rs".to_owned(),
            ],
        );

        let minimal_budget = predict_budget(&minimal, None, None);
        let detailed_budget = predict_budget(&detailed, None, None);

        assert!(
            detailed_budget.confidence() > minimal_budget.confidence(),
            "detailed prompt should have higher confidence"
        );
    }

    #[test]
    fn resume_turns_ratio() {
        let prompt = make_prompt(50, "Feature", "Implement new feature.", Vec::new());
        let budget = predict_budget(&prompt, None, None);

        let ratio = f64::from(budget.resume_turns()) / f64::from(budget.initial_turns());
        assert!(
            (0.5..=0.8).contains(&ratio),
            "resume turns should be 50-80% of initial, got {:.0}%",
            ratio * 100.0
        );
    }

    #[test]
    fn budget_bounds_enforced() {
        let tiny = make_prompt(60, "Style", "Style fix.", Vec::new());
        let huge = make_prompt(
            61,
            "Feature",
            "Massive feature.",
            vec![
                "src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs", "src/f.rs", "src/g.rs",
                "src/h.rs", "src/i.rs", "src/j.rs", "src/k.rs", "src/l.rs", "src/m.rs", "src/n.rs",
                "src/o.rs",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );

        let tiny_budget = predict_budget(&tiny, Some("style"), None);
        let huge_budget = predict_budget(&huge, Some("feat"), None);

        assert!(
            tiny_budget.initial_turns() >= 20,
            "budget should be at least 20, got {}",
            tiny_budget.initial_turns()
        );
        assert!(
            huge_budget.initial_turns() <= 500,
            "budget should be at most 500, got {}",
            huge_budget.initial_turns()
        );
    }

    #[test]
    fn custom_max_turns_ceiling() {
        let prompt = make_prompt(
            62,
            "Feature",
            "Massive feature.",
            vec!["src/a.rs".to_owned()],
        );
        let budget = predict_budget(&prompt, Some("feat"), Some(100));

        assert!(
            budget.initial_turns() <= 100,
            "budget should respect custom ceiling of 100, got {}",
            budget.initial_turns()
        );
    }

    #[test]
    fn explanation_is_informative() {
        let prompt = make_prompt(
            70,
            "Feature",
            "Implement feature.",
            vec!["src/main.rs".to_owned()],
        );
        let budget = predict_budget(&prompt, Some("feat"), None);

        assert!(budget.explanation().contains("feat"));
        assert!(budget.explanation().contains("turns"));
        assert!(budget.explanation().contains("confidence"));
    }

    #[test]
    fn historical_blending() {
        let prompt = make_prompt(
            80,
            "Feature",
            "Implement feature.",
            vec!["src/a.rs".to_owned()],
        );
        let without_historical = predict_budget(&prompt, Some("feat"), None);
        let with_historical =
            predict_budget_with_historical(&prompt, Some("feat"), None, Some(150.0));

        assert!(!without_historical.factors().historical_based);
        assert!(with_historical.factors().historical_based);
        assert!(with_historical.confidence() >= without_historical.confidence());
    }

    #[test]
    fn domain_inference_from_description() {
        let research = make_prompt(90, "Research API patterns", "Study APIs.", Vec::new());
        let budget = predict_budget(&research, None, None);
        assert_eq!(budget.factors().domain, "research");
    }

    #[test]
    fn domain_inference_defaults_to_general() {
        let general = make_prompt(91, "Something", "Do something.", Vec::new());
        let budget = predict_budget(&general, None, None);
        assert_eq!(budget.factors().domain, "general");
    }
}
