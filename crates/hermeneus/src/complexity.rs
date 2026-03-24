//! Complexity-based model routing for adaptive inference.
//!
//! Scores query complexity on multiple dimensions (length, tool requirements,
//! domain signals, conversation depth, explicit markers) and routes to an
//! appropriate model tier (Haiku / Sonnet / Opus).

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::models::names;

/// Default threshold below which queries route to the fast tier.
const DEFAULT_LOW_THRESHOLD: u32 = 30;

/// Default threshold above which queries route to the high-capability tier.
const DEFAULT_HIGH_THRESHOLD: u32 = 70;

// --- Regex patterns (compiled once) ---

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static FORCE_COMPLEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(think hard|deep think|opus|be thorough|take your time)\b")
        .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static FORCE_ROUTINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(quick question|just (?:tell me|answer)|short answer|quick)\b")
        .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static SIMPLE_RESPONSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(yes|no|ok|thanks|sure|got it|hi|hello|hey|yep|nope|k|lgtm|ship it|do it|go|go ahead)\b")
        .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static COMPLEX_INTENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(analyze|plan|design|implement|debug|review|compare|explain why|architecture|strategy|refactor|investigate|evaluate|diagnose|decide|tradeoff|synthesize|audit|spec|migrate)\b")
        .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static TOOL_KEYWORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(search|find|edit|run|execute|create|delete|deploy|build|test|install|configure|check|read|write|commit|push|merge|pr)\b")
        .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static MULTI_STEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(then|after that|next|also|and then|step \d|first.*then|finally|for each|all of)\b",
    )
    .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static CODE_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```.*```").expect("constant regex"));

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static QUESTION_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(what|how|why|where|when|who|which|can you|could you|would you)")
        .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static DOMAIN_JUDGMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(should I|what do you think|your opinion|recommend|advice|best approach|pros and cons|worth it)\b")
        .expect("constant regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static PHILOSOPHICAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(meaning|philosophy|ethics|moral|epistem\w*|ontolog\w*|metaphys\w*|existential|consciousness)\b")
        .expect("constant regex")
});

/// Model capability tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ModelTier {
    /// Fast, cheap, sufficient for simple queries.
    Haiku,
    /// Balanced capability and cost.
    Sonnet,
    /// Maximum capability for hard problems.
    Opus,
}

impl fmt::Display for ModelTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Haiku => f.write_str("haiku"),
            Self::Sonnet => f.write_str("sonnet"),
            Self::Opus => f.write_str("opus"),
        }
    }
}

/// Input signals for complexity scoring.
#[derive(Debug, Clone)]
pub struct ComplexityInput<'a> {
    /// The user's message text.
    pub message_text: &'a str,
    /// Number of tools available in the current context.
    pub tool_count: usize,
    /// Number of messages already in the conversation.
    pub message_count: usize,
    /// Nesting depth for cross-agent calls (0 = top-level).
    pub depth: u32,
    /// Agent-level override from configuration (bypasses scoring).
    pub tier_override: Option<ModelTier>,
    /// Explicit model override from the user (bypasses routing entirely).
    pub model_override: Option<&'a str>,
}

/// Configuration for complexity-based routing thresholds and model mappings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ComplexityConfig {
    /// Whether complexity-based routing is enabled.
    pub enabled: bool,
    /// Score at or below which queries route to `haiku_model`.
    pub low_threshold: u32,
    /// Score at or above which queries route to `opus_model`.
    pub high_threshold: u32,
    /// Model identifier for the fast/cheap tier.
    pub haiku_model: String,
    /// Model identifier for the balanced tier.
    pub sonnet_model: String,
    /// Model identifier for the high-capability tier.
    pub opus_model: String,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            low_threshold: DEFAULT_LOW_THRESHOLD,
            high_threshold: DEFAULT_HIGH_THRESHOLD,
            haiku_model: names::HAIKU.to_owned(),
            sonnet_model: names::SONNET.to_owned(),
            opus_model: names::OPUS.to_owned(),
        }
    }
}

/// Result of complexity scoring.
#[derive(Debug, Clone)]
pub struct ComplexityScore {
    /// Numeric score (0--100).
    pub score: u32,
    /// Determined model tier.
    pub tier: ModelTier,
    /// Human-readable factors that contributed to the score.
    pub reason: String,
}

/// Final routing decision with model selection.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    /// Selected model identifier.
    pub model: String,
    /// Complexity score that drove the decision.
    pub complexity: ComplexityScore,
    /// Whether the user explicitly overrode model selection.
    pub is_override: bool,
}

impl fmt::Display for RoutingDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_override {
            write!(f, "routed to {} (user override)", self.model)
        } else {
            write!(
                f,
                "routed to {} (complexity: {}, reason: {})",
                self.model, self.complexity.score, self.complexity.reason
            )
        }
    }
}

/// Outcome of a routed request, fed back to competence tracking.
#[derive(Debug, Clone)]
pub struct RoutingOutcome {
    /// The routing decision that was made.
    pub decision: RoutingDecision,
    /// Whether the response was successful.
    pub success: bool,
    /// Whether the model self-escalated (indicated it needed more capability).
    pub self_escalated: bool,
}

/// Score the complexity of a query across multiple dimensions.
///
/// Returns a [`ComplexityScore`] with a numeric score (0--100), the determined
/// tier, and a human-readable reason string.
#[must_use]
pub fn score_complexity(input: &ComplexityInput<'_>) -> ComplexityScore {
    // Agent-level tier override bypasses scoring
    if let Some(tier) = input.tier_override {
        let score = match tier {
            ModelTier::Opus => 100,
            ModelTier::Sonnet => 50,
            ModelTier::Haiku => 10,
        };
        return ComplexityScore {
            score,
            tier,
            reason: "agent override".to_owned(),
        };
    }

    // Cross-agent calls always get full power
    if input.depth > 0 {
        return ComplexityScore {
            score: 90,
            tier: ModelTier::Opus,
            reason: "cross-agent".to_owned(),
        };
    }

    let text = input.message_text;

    // User-explicit override markers
    if FORCE_COMPLEX.is_match(text) {
        return ComplexityScore {
            score: 95,
            tier: ModelTier::Opus,
            reason: "user override: think hard".to_owned(),
        };
    }
    if FORCE_ROUTINE.is_match(text) {
        return ComplexityScore {
            score: 5,
            tier: ModelTier::Haiku,
            reason: "user override: quick".to_owned(),
        };
    }

    let (score, factors) = score_dimensions(input);
    let score = clamp_score(score);
    let tier = tier_from_score(score, DEFAULT_LOW_THRESHOLD, DEFAULT_HIGH_THRESHOLD);

    let reason = if factors.is_empty() {
        "baseline".to_owned()
    } else {
        factors.join(", ")
    };

    ComplexityScore {
        score,
        tier,
        reason,
    }
}

/// Evaluate individual scoring dimensions and return raw score + factor list.
fn score_dimensions(input: &ComplexityInput<'_>) -> (i32, Vec<&'static str>) {
    let text = input.message_text;
    let mut score: i32 = 30;
    let mut factors: Vec<&str> = Vec::new();

    // Length signals
    if text.len() < 30 {
        score -= 20;
        factors.push("very short");
    } else if text.len() < 80 {
        score -= 10;
        factors.push("short");
    } else if text.len() > 1000 {
        score += 25;
        factors.push("very long");
    } else if text.len() > 500 {
        score += 15;
        factors.push("long");
    }

    // First message in session
    if input.message_count == 0 {
        score += 15;
        factors.push("first message");
    }

    // Simple response patterns
    if SIMPLE_RESPONSE.is_match(text) {
        score -= 30;
        factors.push("simple response");
    }

    // Complex intent keywords
    if COMPLEX_INTENT.is_match(text) {
        score += 25;
        factors.push("complex intent");
    }

    // Domain judgment (needs high-quality reasoning)
    if DOMAIN_JUDGMENT.is_match(text) {
        score += 20;
        factors.push("judgment");
    }

    // Philosophical / subtle
    if PHILOSOPHICAL.is_match(text) {
        score += 20;
        factors.push("philosophical");
    }

    // Tool keywords (floor at 35 when tools are mentioned)
    if TOOL_KEYWORDS.is_match(text) {
        score = score.max(35);
        factors.push("tool keywords");
    }

    // Multiple tools available increase complexity
    if input.tool_count > 5 {
        score += 10;
        factors.push("many tools");
    } else if input.tool_count > 2 {
        score += 5;
        factors.push("multiple tools");
    }

    // Multi-step patterns
    if MULTI_STEP.is_match(text) {
        score += 15;
        factors.push("multi-step");
    }

    // Code blocks
    if CODE_BLOCK.is_match(text) {
        score += 10;
        factors.push("code block");
    }

    // Question complexity
    if QUESTION_WORDS.is_match(text) && text.ends_with('?') {
        if text.len() < 60 {
            score -= 5;
            factors.push("simple question");
        } else {
            score += 5;
            factors.push("detailed question");
        }
    }

    // Sentence count signals multi-part reasoning
    let sentence_count = text
        .split(['.', '!', '?'])
        .filter(|s| s.trim().len() > 10)
        .count();
    if sentence_count >= 3 {
        score += 10;
        factors.push("multi-sentence");
    }

    // Conversation depth: deeper conversations tend toward complexity
    if input.message_count > 20 {
        score += 10;
        factors.push("deep conversation");
    } else if input.message_count > 10 {
        score += 5;
        factors.push("moderate conversation");
    }

    (score, factors)
}

/// Route a query to a model based on complexity scoring.
///
/// User model overrides always take precedence. When complexity routing is
/// disabled, returns the configured sonnet (primary) model.
#[must_use]
pub fn route_model(input: &ComplexityInput<'_>, config: &ComplexityConfig) -> RoutingDecision {
    // User override always wins
    if let Some(model) = input.model_override {
        let complexity = score_complexity(input);
        tracing::info!(
            model,
            complexity_score = complexity.score,
            complexity_tier = %complexity.tier,
            "model routing: user override"
        );
        return RoutingDecision {
            model: model.to_owned(),
            complexity,
            is_override: true,
        };
    }

    // Disabled routing: use the configured sonnet model (primary)
    if !config.enabled {
        let complexity = score_complexity(input);
        return RoutingDecision {
            model: config.sonnet_model.clone(),
            complexity,
            is_override: false,
        };
    }

    let complexity = score_complexity(input);
    let model = select_model_for_tier(complexity.tier, config);

    tracing::info!(
        model,
        complexity_score = complexity.score,
        complexity_tier = %complexity.tier,
        reason = %complexity.reason,
        "model routing decision"
    );

    RoutingDecision {
        model,
        complexity,
        is_override: false,
    }
}

/// Select the model identifier for a given tier from config.
#[must_use]
fn select_model_for_tier(tier: ModelTier, config: &ComplexityConfig) -> String {
    match tier {
        ModelTier::Haiku => config.haiku_model.clone(),
        ModelTier::Sonnet => config.sonnet_model.clone(),
        ModelTier::Opus => config.opus_model.clone(),
    }
}

/// Map a numeric score to a model tier using configurable thresholds.
#[must_use]
fn tier_from_score(score: u32, low_threshold: u32, high_threshold: u32) -> ModelTier {
    if score <= low_threshold {
        ModelTier::Haiku
    } else if score >= high_threshold {
        ModelTier::Opus
    } else {
        ModelTier::Sonnet
    }
}

/// Clamp an i32 score into the [0, 100] range.
#[must_use]
fn clamp_score(raw: i32) -> u32 {
    #[expect(
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "value is guaranteed non-negative after clamping to 0; as-cast is safe for 0..=100"
    )]
    {
        raw.clamp(0, 100) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> ComplexityInput<'_> {
        ComplexityInput {
            message_text: text,
            tool_count: 0,
            message_count: 5,
            depth: 0,
            tier_override: None,
            model_override: None,
        }
    }

    // --- Scoring edge cases ---

    #[test]
    fn empty_message_scores_low() {
        let result = score_complexity(&input(""));
        assert!(
            result.score <= DEFAULT_LOW_THRESHOLD,
            "empty message should score low, got {}",
            result.score
        );
        assert_eq!(result.tier, ModelTier::Haiku);
    }

    #[test]
    fn simple_greeting_scores_low() {
        let result = score_complexity(&input("hi"));
        assert!(
            result.score <= DEFAULT_LOW_THRESHOLD,
            "greeting should score low, got {}",
            result.score
        );
        assert_eq!(result.tier, ModelTier::Haiku);
    }

    #[test]
    fn yes_no_scores_very_low() {
        for word in &["yes", "no", "ok", "thanks", "lgtm", "ship it"] {
            let result = score_complexity(&input(word));
            assert!(
                result.score <= 15,
                "'{word}' should score very low, got {}",
                result.score
            );
        }
    }

    #[test]
    fn complex_intent_boosts_score() {
        let result = score_complexity(&input(
            "Please analyze this code and investigate why the tests are failing in the CI pipeline",
        ));
        assert!(
            result.score > DEFAULT_LOW_THRESHOLD,
            "complex intent should boost score, got {}",
            result.score
        );
    }

    #[test]
    fn multi_step_boosts_score() {
        let result = score_complexity(&input(
            "First refactor the config module, then update the tests, and finally fix the CI",
        ));
        assert!(
            result.score >= 50,
            "multi-step request should score at least medium, got {}",
            result.score
        );
    }

    #[test]
    fn code_block_boosts_score() {
        let result = score_complexity(&input(
            "Review this code:\n```rust\nfn main() { println!(\"hello\"); }\n```\nIs it correct?",
        ));
        assert!(
            result.score > DEFAULT_LOW_THRESHOLD,
            "code block should boost score, got {}",
            result.score
        );
    }

    #[test]
    fn very_long_message_boosts_score() {
        let long_text = "a ".repeat(600);
        let result = score_complexity(&input(&long_text));
        assert!(
            result.score > DEFAULT_LOW_THRESHOLD,
            "long message should boost score, got {}",
            result.score
        );
    }

    #[test]
    fn first_message_gets_boost() {
        let mut inp = input("hello there, I need help with something");
        inp.message_count = 0;
        let first = score_complexity(&inp);

        inp.message_count = 10;
        let later = score_complexity(&inp);

        assert!(
            first.score > later.score,
            "first message should score higher: {} vs {}",
            first.score,
            later.score
        );
    }

    #[test]
    fn many_tools_boost_score() {
        let mut inp = input("Can you help me with this task?");
        inp.tool_count = 0;
        let no_tools = score_complexity(&inp);

        inp.tool_count = 8;
        let many_tools = score_complexity(&inp);

        assert!(
            many_tools.score > no_tools.score,
            "many tools should increase score: {} vs {}",
            many_tools.score,
            no_tools.score
        );
    }

    #[test]
    fn deep_conversation_boosts_score() {
        let mut inp = input("What about this approach?");
        inp.message_count = 5;
        let shallow = score_complexity(&inp);

        inp.message_count = 25;
        let deep = score_complexity(&inp);

        assert!(
            deep.score > shallow.score,
            "deep conversation should score higher: {} vs {}",
            deep.score,
            shallow.score
        );
    }

    #[test]
    fn judgment_request_boosts_score() {
        let result = score_complexity(&input(
            "What do you think about this architecture? Should I use microservices or a monolith?",
        ));
        assert!(
            result.score >= 50,
            "judgment request should score high, got {}",
            result.score
        );
    }

    // --- Explicit markers ---

    #[test]
    fn force_complex_marker_returns_opus() {
        let result = score_complexity(&input("Think hard about the best database schema"));
        assert_eq!(result.tier, ModelTier::Opus);
        assert_eq!(result.score, 95);
        assert!(
            result.reason.contains("user override"),
            "reason should mention user override"
        );
    }

    #[test]
    fn force_routine_marker_returns_haiku() {
        let result = score_complexity(&input("quick question: what port does the server use?"));
        assert_eq!(result.tier, ModelTier::Haiku);
        assert_eq!(result.score, 5);
    }

    // --- Agent and depth overrides ---

    #[test]
    fn agent_override_bypasses_scoring() {
        let mut inp = input("simple question");
        inp.tier_override = Some(ModelTier::Opus);
        let result = score_complexity(&inp);
        assert_eq!(result.tier, ModelTier::Opus);
        assert_eq!(result.score, 100);
        assert_eq!(result.reason, "agent override");
    }

    #[test]
    fn cross_agent_always_opus() {
        let mut inp = input("hello");
        inp.depth = 1;
        let result = score_complexity(&inp);
        assert_eq!(result.tier, ModelTier::Opus);
        assert_eq!(result.score, 90);
    }

    // --- Score clamping ---

    #[test]
    fn score_never_exceeds_100() {
        let result = score_complexity(&input(
            "Think hard about this complex philosophical analysis. Analyze the architecture, \
             design the migration strategy, and evaluate all the tradeoffs. What do you think is \
             the best approach? Consider the ethical implications and synthesize a plan.",
        ));
        assert!(
            result.score <= 100,
            "score should be clamped to 100, got {}",
            result.score
        );
    }

    #[test]
    fn score_clamps_low_inputs() {
        let result = score_complexity(&input("no"));
        assert_eq!(
            result.tier,
            ModelTier::Haiku,
            "very negative raw score should clamp to 0 and route to haiku"
        );
    }

    // --- Threshold routing ---

    #[test]
    fn tier_from_score_low() {
        assert_eq!(tier_from_score(0, 30, 70), ModelTier::Haiku);
        assert_eq!(tier_from_score(15, 30, 70), ModelTier::Haiku);
        assert_eq!(tier_from_score(30, 30, 70), ModelTier::Haiku);
    }

    #[test]
    fn tier_from_score_medium() {
        assert_eq!(tier_from_score(31, 30, 70), ModelTier::Sonnet);
        assert_eq!(tier_from_score(50, 30, 70), ModelTier::Sonnet);
        assert_eq!(tier_from_score(69, 30, 70), ModelTier::Sonnet);
    }

    #[test]
    fn tier_from_score_high() {
        assert_eq!(tier_from_score(70, 30, 70), ModelTier::Opus);
        assert_eq!(tier_from_score(85, 30, 70), ModelTier::Opus);
        assert_eq!(tier_from_score(100, 30, 70), ModelTier::Opus);
    }

    #[test]
    fn custom_thresholds_shift_tiers() {
        assert_eq!(tier_from_score(40, 50, 80), ModelTier::Haiku);
        assert_eq!(tier_from_score(60, 50, 80), ModelTier::Sonnet);
        assert_eq!(tier_from_score(85, 50, 80), ModelTier::Opus);
    }

    // --- Model routing ---

    #[test]
    fn route_model_user_override_wins() {
        let mut inp = input("analyze this complex problem");
        inp.model_override = Some("claude-opus-4-6");
        let config = ComplexityConfig {
            enabled: true,
            ..Default::default()
        };

        let decision = route_model(&inp, &config);
        assert!(decision.is_override, "should be flagged as override");
        assert_eq!(decision.model, "claude-opus-4-6");
    }

    #[test]
    fn route_model_disabled_returns_sonnet() {
        let config = ComplexityConfig {
            enabled: false,
            ..Default::default()
        };

        let decision = route_model(&input("analyze this"), &config);
        assert_eq!(decision.model, names::SONNET);
        assert!(!decision.is_override);
    }

    #[test]
    fn route_model_simple_query_routes_haiku() {
        let config = ComplexityConfig {
            enabled: true,
            ..Default::default()
        };

        let decision = route_model(&input("yes"), &config);
        assert_eq!(decision.model, names::HAIKU);
    }

    #[test]
    fn route_model_complex_query_routes_opus() {
        let config = ComplexityConfig {
            enabled: true,
            ..Default::default()
        };

        let decision = route_model(
            &input("Think hard about the best migration strategy"),
            &config,
        );
        assert_eq!(decision.model, names::OPUS);
    }

    #[test]
    fn route_model_custom_model_names() {
        let config = ComplexityConfig {
            enabled: true,
            haiku_model: "custom-fast".to_owned(),
            sonnet_model: "custom-balanced".to_owned(),
            opus_model: "custom-powerful".to_owned(),
            ..Default::default()
        };

        let decision = route_model(&input("ok"), &config);
        assert_eq!(decision.model, "custom-fast");
    }

    #[test]
    fn config_default_is_disabled() {
        let config = ComplexityConfig::default();
        assert!(!config.enabled, "complexity routing should be opt-in");
    }

    // --- Display ---

    #[test]
    fn routing_decision_display_override() {
        let decision = RoutingDecision {
            model: "claude-opus-4-6".to_owned(),
            complexity: ComplexityScore {
                score: 50,
                tier: ModelTier::Sonnet,
                reason: "baseline".to_owned(),
            },
            is_override: true,
        };
        let display = format!("{decision}");
        assert!(
            display.contains("user override"),
            "display should mention override: {display}"
        );
    }

    #[test]
    fn routing_decision_display_routed() {
        let decision = RoutingDecision {
            model: "claude-sonnet-4-6".to_owned(),
            complexity: ComplexityScore {
                score: 45,
                tier: ModelTier::Sonnet,
                reason: "single-tool code review".to_owned(),
            },
            is_override: false,
        };
        let display = format!("{decision}");
        assert!(
            display.contains("complexity: 45"),
            "display should include score: {display}"
        );
        assert!(
            display.contains("single-tool code review"),
            "display should include reason: {display}"
        );
    }

    #[test]
    fn model_tier_display() {
        assert_eq!(format!("{}", ModelTier::Haiku), "haiku");
        assert_eq!(format!("{}", ModelTier::Sonnet), "sonnet");
        assert_eq!(format!("{}", ModelTier::Opus), "opus");
    }

    // --- Routing outcome for competence feedback ---

    #[test]
    fn routing_outcome_captures_escalation() {
        let outcome = RoutingOutcome {
            decision: RoutingDecision {
                model: names::SONNET.to_owned(),
                complexity: ComplexityScore {
                    score: 45,
                    tier: ModelTier::Sonnet,
                    reason: "baseline".to_owned(),
                },
                is_override: false,
            },
            success: true,
            self_escalated: true,
        };
        assert!(
            outcome.self_escalated,
            "should record self-escalation for competence tracking"
        );
    }

    // --- Multi-sentence scoring ---

    #[test]
    fn multi_sentence_boosts_score() {
        let single = score_complexity(&input("Fix the bug"));
        let multi = score_complexity(&input(
            "The login page crashes on submit. The error log shows a null pointer. \
             Users are reporting data loss. Please investigate the root cause.",
        ));
        assert!(
            multi.score > single.score,
            "multi-sentence should score higher: {} vs {}",
            multi.score,
            single.score
        );
    }

    // --- Philosophical content ---

    #[test]
    fn philosophical_content_boosts_score() {
        let result = score_complexity(&input(
            "What are the epistemological implications of using LLMs for automated reasoning?",
        ));
        assert!(
            result.score >= 50,
            "philosophical content should score high, got {}",
            result.score
        );
    }
}
