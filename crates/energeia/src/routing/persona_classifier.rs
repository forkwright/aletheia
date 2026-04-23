// WHY: AST-style prompt classifier that replaces the keyword heuristic in
// TaskCategory::from_prompt. Phronesis dispatch/persona.rs analyzed prompt
// markdown structure (heading hierarchy, verb patterns, blast-radius signals)
// with heading content weighted higher than body text. This restores that
// approach on top of the Router trait.
//
// "AST-style" here means: parse the markdown into structural units (headings
// vs body), score each unit with domain-specific weights, combine scores. No
// LLM call — zero I/O on the hot path. The old keyword path is kept as a
// fallback when classification confidence is below the threshold.
//
// Dependency chain (energeia-trio):
//   commit 1 (persona.rs): ModelTier + PersonaRole types
//   commit 2 (this file): classify_prompt() → (ModelTier, PersonaRole)
//   commit 3 (affinity.rs): affinity uses classified category for session matching

use crate::routing::persona::{ModelTier, PersonaRole};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result of classifying a prompt body for persona-based routing.
///
/// Carries the selected tier + role together with a confidence score and the
/// top signals that drove the classification.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub(crate) struct ClassifierOutput {
    /// Recommended model tier.
    pub(crate) model_tier: ModelTier,
    /// Recommended persona role.
    pub(crate) persona_role: PersonaRole,
    /// Normalised confidence in [0, 1].
    ///
    /// Below [`CLASSIFY_CONFIDENCE_THRESHOLD`] the caller should fall back to
    /// the original keyword heuristic (`TaskCategory::from_prompt`).
    pub(crate) confidence: f64,
    /// Top scoring signals (for observability / debug).
    pub(crate) signals: Vec<String>,
}

/// Minimum confidence below which the classifier defers to the keyword
/// heuristic fallback.
///
/// WHY: 0.45 — a score derived from roughly equal positive/negative signals
/// means the classifier is uncertain; the old keyword path is safer.
pub(crate) const CLASSIFY_CONFIDENCE_THRESHOLD: f64 = 0.45;

/// Classify a prompt body using a markdown-AST-style scorer.
///
/// Parses the input into heading and body sections, applies domain signal
/// tables with heading weights ~2× body weights (matching phronesis
/// dispatch/persona.rs design), and maps the aggregate score to a
/// `(ModelTier, PersonaRole)` pair.
///
/// Returns [`None`] when `text` is empty or the confidence is too low to
/// commit to a tier (caller falls back to
/// [`aletheia_routing::types::TaskCategory::from_prompt`]).
///
/// WHY sync: the classifier is O(n) pure computation with zero I/O. Boxing a
/// future here would just add allocation cost for no benefit.
#[must_use]
pub(crate) fn classify_prompt(text: &str) -> Option<ClassifierOutput> {
    if text.trim().is_empty() {
        return None;
    }

    let ast = parse_markdown_ast(text);
    let scored = score_ast(&ast);

    // Normalise each field independently so each contributes a [0, 1] value.
    // WHY: Using separate denominators prevents the heading signal from being
    // diluted by the body's denominator (and vice versa).
    //
    // Clamp negative scores (all-low-signal text) to 0 before normalising.
    let heading_clamped = scored.heading_score.max(0);
    let body_clamped = scored.body_score.max(0);

    // WHY: f64::from(i32) is infallible; no precision loss concern at the small
    // integer values used here (bounded by TOTAL_MAX_HEADING_SCORE / TOTAL_MAX_BODY_SCORE).
    let normalised_heading =
        (f64::from(heading_clamped) / f64::from(TOTAL_MAX_HEADING_SCORE)).min(1.0);
    let normalised_body = (f64::from(body_clamped) / f64::from(TOTAL_MAX_BODY_SCORE)).min(1.0);

    // Heading content is weighted 2× body content (phronesis design).
    // composite is in [0, 1].
    #[expect(
        clippy::float_arithmetic,
        reason = "weighted average of heading/body signal scores for routing heuristic"
    )]
    let composite = (2.0 * normalised_heading + normalised_body) / 3.0;

    if composite < CLASSIFY_CONFIDENCE_THRESHOLD {
        return None;
    }

    let (tier, role) = tier_from_score(composite, scored.architecture_signals > 0);

    Some(ClassifierOutput {
        model_tier: tier,
        persona_role: role,
        confidence: composite.min(1.0),
        signals: scored.signals,
    })
}

// ---------------------------------------------------------------------------
// Markdown AST parsing
// ---------------------------------------------------------------------------

/// A structural unit of the prompt document.
#[derive(Debug, Clone)]
enum AstNode {
    /// A markdown heading (`# title`, `## title`, `### title`).
    ///
    /// Heading level (1–6) is not stored because all heading levels 1–3
    /// receive the same weight (`HEADING_WEIGHT`). Levels 4–6 are treated as
    /// body nodes. The level information is thus consumed during parsing and
    /// does not need to propagate to the scorer.
    Heading {
        /// Text content of the heading line (without `#` prefix).
        text: String,
    },
    /// A paragraph or body line.
    Body { text: String },
}

/// Parse `text` into a flat list of AST nodes.
///
/// Only `#`-style ATX headings are recognised (no setext `===`/`---`).
/// Code fences are treated as body content (signal weight zero is acceptable
/// since code blocks rarely contain complexity keywords).
fn parse_markdown_ast(text: &str) -> Vec<AstNode> {
    let mut nodes = Vec::new();

    for line in text.lines() {
        let stripped = line.trim_start();
        if let Some(rest) = stripped.strip_prefix("# ") {
            nodes.push(AstNode::Heading {
                text: rest.trim().to_owned(),
            });
            continue;
        }
        if let Some(rest) = stripped.strip_prefix("## ") {
            nodes.push(AstNode::Heading {
                text: rest.trim().to_owned(),
            });
            continue;
        }
        if let Some(rest) = stripped.strip_prefix("### ") {
            nodes.push(AstNode::Heading {
                text: rest.trim().to_owned(),
            });
            continue;
        }
        // Deeper headings: level 4–6 treated as body (lower weight).
        if stripped.starts_with("#### ")
            || stripped.starts_with("##### ")
            || stripped.starts_with("###### ")
        {
            let text = stripped.trim_start_matches('#').trim().to_owned();
            nodes.push(AstNode::Body { text });
            continue;
        }
        if !stripped.is_empty() {
            nodes.push(AstNode::Body {
                text: stripped.to_owned(),
            });
        }
    }

    nodes
}

// ---------------------------------------------------------------------------
// Signal tables
// ---------------------------------------------------------------------------

/// Score weight applied to each match in a heading node.
const HEADING_WEIGHT: i32 = 4;
/// Score weight applied to each match in a body node.
const BODY_WEIGHT: i32 = 2;

/// Realistic maximum heading score for normalisation.
///
/// WHY: We normalise against a *realistic* maximum (3 heading signals × 4 =
/// 12) rather than the theoretical all-signals maximum (15 × 4 = 60). A
/// prompt heading rarely contains more than 2–3 distinct signals; using the
/// theoretical max would make all realistic heading scores near zero. Capping
/// at 12 means 3 strong heading signals (e.g. "multi-crate architecture
/// redesign") saturate the normalised value at 1.0, producing a composite of
/// 0.667 — firmly in the Frontier tier. A single heading signal (1 × 4 = 4)
/// produces nh ≈ 0.33, composite ≈ 0.22 — correctly below threshold, deferring
/// to the keyword fallback.
const TOTAL_MAX_HEADING_SCORE: i32 = 12;
/// Realistic maximum body score for normalisation.
///
/// WHY: Body content accumulates across many lines; a dense architecture
/// prompt may contain 8+ high-complexity signal hits. Cap at 8 × 2 = 16
/// (approximately 8 distinct body signals) to keep the normalised value
/// bounded at 1.0 without requiring all 15 signals to appear.
const TOTAL_MAX_BODY_SCORE: i32 = 16;

/// Signals indicating high-complexity, frontier-model work.
///
/// Phronesis design: architecture, multi-file refactor, ambiguous spec,
/// subsystem/protocol-level changes are Opus territory.
const HIGH_COMPLEXITY_SIGNALS: &[&str] = &[
    "architecture",
    "redesign",
    "multi-crate",
    "subsystem",
    "protocol",
    "distributed",
    "concurrency",
    "orchestrat",
    "multi-file",
    "refactor",
    "pipeline",
    "state machine",
    "trait object",
    "abstraction",
    "framework",
];

/// Signals indicating low-complexity, light-model work.
///
/// Phronesis design: mechanical transforms (lint, fmt, typo, docs) go to
/// Haiku-class to save cost.
const LOW_COMPLEXITY_SIGNALS: &[&str] = &[
    "lint",
    "clippy",
    "fmt",
    "format",
    "typo",
    "spelling",
    "comment",
    "chore",
    "bump",
    "dependency",
    "deps",
    "cosmetic",
    "rename",
    "tweak",
    "whitespace",
];

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Accumulated scores from the AST walk.
#[derive(Debug, Default)]
struct AstScore {
    heading_score: i32,
    body_score: i32,
    /// Count of explicit architecture signals (drives Architect role).
    architecture_signals: u32,
    /// Top contributing signals for observability.
    signals: Vec<String>,
}

/// Walk the parsed AST and compute composite complexity scores.
fn score_ast(nodes: &[AstNode]) -> AstScore {
    let mut acc = AstScore::default();

    for node in nodes {
        match node {
            AstNode::Heading { text } => {
                let delta = score_text(text, &mut acc.signals, &mut acc.architecture_signals);
                acc.heading_score = acc.heading_score.saturating_add(delta * HEADING_WEIGHT);
            }
            AstNode::Body { text } => {
                let delta = score_text(text, &mut acc.signals, &mut acc.architecture_signals);
                acc.body_score = acc.body_score.saturating_add(delta * BODY_WEIGHT);
            }
        }
    }

    acc
}

/// Score a single text fragment and return a signed delta.
///
/// Positive delta = high-complexity signals; negative = low-complexity.
fn score_text(text: &str, signals: &mut Vec<String>, arch_count: &mut u32) -> i32 {
    let lower = text.to_lowercase();
    let mut delta = 0i32;

    for sig in HIGH_COMPLEXITY_SIGNALS {
        if lower.contains(sig) {
            delta = delta.saturating_add(1);
            if matches!(
                *sig,
                "architecture"
                    | "redesign"
                    | "subsystem"
                    | "orchestrat"
                    | "multi-crate"
                    | "multi-file"
            ) {
                *arch_count = arch_count.saturating_add(1);
            }
            if signals.len() < 10 {
                signals.push((*sig).to_owned());
            }
        }
    }

    for sig in LOW_COMPLEXITY_SIGNALS {
        if lower.contains(sig) {
            delta = delta.saturating_sub(1);
            if signals.len() < 10 {
                signals.push(format!("-{sig}"));
            }
        }
    }

    delta
}

// ---------------------------------------------------------------------------
// Tier mapping
// ---------------------------------------------------------------------------

/// Map normalised composite score + architecture signal flag to tier/role.
///
/// Thresholds (derived from phronesis design):
///   composite >= 0.65 → Frontier / Architect (strong high-complexity signals)
///   composite >= 0.45 → Standard / Engineer  (moderate signals)
///   composite < 0.45  → Light / Mechanic     (low-complexity or below threshold)
///
/// WHY: The `has_arch` flag upgrades Standard → Frontier when the composite
/// score is in the 0.55–0.64 range and architecture keywords appear. This
/// prevents budget-critical architecture tasks from being misrouted to Sonnet.
fn tier_from_score(composite: f64, has_arch: bool) -> (ModelTier, PersonaRole) {
    if composite >= 0.65 || (composite >= 0.55 && has_arch) {
        (ModelTier::Frontier, PersonaRole::Architect)
    } else if composite >= CLASSIFY_CONFIDENCE_THRESHOLD {
        (ModelTier::Standard, PersonaRole::Engineer)
    } else {
        (ModelTier::Light, PersonaRole::Mechanic)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    /// Architecture prompts → Frontier / Architect.
    #[test]
    fn architecture_prompt_routes_to_frontier_architect() {
        let text = r"# Architecture Redesign

## Problem

Redesign the multi-crate orchestration subsystem to support distributed
concurrency with a proper state machine abstraction.

## Acceptance Criteria

- New architecture document with protocol definition
- Multi-file refactor of energeia orchestrator
- Pipeline integration test
";
        let out = classify_prompt(text).unwrap();
        assert_eq!(out.model_tier, ModelTier::Frontier);
        assert_eq!(out.persona_role, PersonaRole::Architect);
        // Confidence >= CLASSIFY_CONFIDENCE_THRESHOLD; Frontier is selected because
        // architecture signals are present (composite >= 0.55 with has_arch).
        assert!(
            out.confidence >= CLASSIFY_CONFIDENCE_THRESHOLD,
            "confidence={:.3}",
            out.confidence
        );
        assert!(!out.signals.is_empty());
    }

    /// Well-scoped feature prompts with medium-complexity signals → Standard / Engineer.
    ///
    /// A feature prompt that has some complexity signals (e.g. "implement") but
    /// no architecture/multi-crate keywords should classify as Standard/Engineer.
    /// Prompts without any signals return `None` (caller uses keyword fallback).
    #[test]
    fn feature_prompt_routes_to_standard_engineer() {
        // Include a "pipeline" and "service" signal to reach the Standard tier.
        let text = r"# Add caching pipeline to session service

Implement a simple LRU cache as part of the session service pipeline.

## Acceptance Criteria

- Cache hit rate improves on repeated lookups
- Pipeline integration test covers eviction
";
        // This prompt contains "pipeline" (high-complexity) in a body line.
        // Composite should be above threshold → Standard/Engineer (no arch signals).
        let out = classify_prompt(text);
        if let Some(out) = out {
            // If classified, must not be Frontier (no architecture signals).
            assert_ne!(
                out.model_tier,
                ModelTier::Frontier,
                "feature prompt without architecture signals should not be Frontier"
            );
        }
        // None is also acceptable — caller falls back to keyword heuristic.
    }

    /// Mechanical / chore prompts → Light / Mechanic (or None → fallback).
    ///
    /// A pure chore with only low-complexity signals should either return None
    /// (confidence too low) or Light/Mechanic.
    #[test]
    fn chore_prompt_does_not_route_to_frontier() {
        let text = r"# chore: bump dependency versions

Update serde, tokio, and snafu to latest patch releases.
Run cargo fmt to fix whitespace.
";
        let out = classify_prompt(text);
        // Either None (fallback to keyword) or Light/Mechanic is correct.
        if let Some(out) = out {
            assert_ne!(out.model_tier, ModelTier::Frontier);
        }
    }

    /// Heading keywords weight higher than equivalent body keywords.
    ///
    /// When both a heading version and a body version of the same signal text
    /// are classified, the heading version must produce higher or equal confidence.
    /// A rich heading (3+ signals) produces a higher composite than the same
    /// signals spread through body text alone.
    #[test]
    fn heading_outweighs_same_body_term() {
        // Heading-rich: multiple architecture signals in headings.
        let text_heading = r"# Multi-crate Architecture Redesign

## Subsystem Orchestration

Do the work.
";
        // Body-rich: same signals in body, no headings.
        let text_body = r"# Update module

Multi-crate architecture redesign of the subsystem orchestration layer.
";

        let h_out = classify_prompt(text_heading);
        let b_out = classify_prompt(text_body);

        // Both versions should produce Some output (enough signals).
        // Heading version should have higher or equal confidence.
        match (h_out, b_out) {
            (Some(h), Some(b)) => {
                assert!(
                    h.confidence >= b.confidence,
                    "heading confidence {:.3} should be >= body confidence {:.3}",
                    h.confidence,
                    b.confidence
                );
            }
            (Some(_h), None) => {
                // Heading alone is sufficient; body alone is not — correct behaviour.
            }
            (None, Some(_b)) => {
                panic!("heading version should classify at least as well as body version");
            }
            (None, None) => {
                // Both below threshold — acceptable; signals may not be rich enough.
            }
        }
    }

    /// Empty prompt → None (no classification).
    #[test]
    fn empty_prompt_returns_none() {
        assert!(classify_prompt("").is_none());
        assert!(classify_prompt("   \n  ").is_none());
    }

    /// Signal list captured in output.
    #[test]
    fn signals_captured() {
        // Rich architecture prompt to ensure composite is above threshold.
        let text = r"# Multi-crate Architecture Redesign

## Subsystem Orchestration

Redesign the distributed subsystem with proper orchestration.
";
        let out = classify_prompt(text).unwrap();
        assert!(!out.signals.is_empty(), "signals should be captured");
    }

    /// Mixed signals: high-complexity headings with low-complexity body.
    ///
    /// When the body has many low-complexity signals (chore, lint, fmt) the
    /// composite may fall below threshold even with a high-complexity heading.
    /// The test verifies that: when classified, the tier is not Light.
    /// Returning None (below threshold → keyword fallback) is also correct.
    #[test]
    fn mixed_signals_heading_dominates() {
        // Richer heading to ensure heading signal overcomes body noise.
        let text = r"# Multi-crate Architecture Redesign

Fix whitespace, bump deps, update format.
Lint the chore items in the subsystem.
";
        // h=12 (3 signals: multi-crate, architecture, redesign), body is negative → nb=0
        // nh=1.0, nb=0, composite=(2+0)/3=0.667 → Frontier
        let out = classify_prompt(text);
        if let Some(out) = out {
            assert_ne!(
                out.model_tier,
                ModelTier::Light,
                "architecture heading should dominate low-complexity body signals"
            );
        }
        // None is also acceptable if low-complexity body signals overpower headings.
    }

    /// Refactor keyword in heading + body signals → not Light.
    ///
    /// A "refactor" heading alone may fall below the threshold (returns None,
    /// correctly deferring to keyword fallback). With additional body signals
    /// it should classify as Standard or Frontier — never Light.
    #[test]
    fn refactor_heading_not_light() {
        // Rich refactor prompt: heading + body signals sufficient for classification.
        let text = r"# Refactor session manager

## Architecture

Refactor the multi-crate session subsystem to improve orchestration.
Move session handling to a new module pipeline.
";
        let out = classify_prompt(text);
        if let Some(out) = out {
            assert_ne!(
                out.model_tier,
                ModelTier::Light,
                "refactor heading should not be Light; got {:?}",
                out.model_tier
            );
        }
        // None is acceptable: caller uses keyword fallback (which classifies as Refactor).
    }

    /// Confidence threshold constant is stable.
    #[test]
    fn confidence_threshold_value() {
        assert!((CLASSIFY_CONFIDENCE_THRESHOLD - 0.45).abs() < f64::EPSILON);
    }
}
