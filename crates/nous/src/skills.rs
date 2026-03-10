//! Skill loading for bootstrap context assembly.
//!
//! Queries mneme for skills relevant to the current task and returns them
//! as `BootstrapSection` values ready for injection into the bootstrap assembler.
//!
//! Skills are injected at `SectionPriority::Flexible`, so they are truncated
//! before workspace identity files under budget pressure.

// ── Always-available pure utilities ─────────────────────────────────────────

/// Maximum characters of task context to send as the BM25 query.
///
/// Longer queries dilute BM25 scores; keep the signal tight.
#[cfg(any(feature = "knowledge-store", test))]
const MAX_CONTEXT_CHARS: usize = 200;

/// Extracts a concise task description from the latest user message.
///
/// The result is used as the BM25 query for skill search, so brevity
/// is preferred. Trims whitespace and truncates at a word boundary.
#[cfg(any(feature = "knowledge-store", test))]
pub(crate) fn extract_task_context(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.len() <= MAX_CONTEXT_CHARS {
        return trimmed.to_owned();
    }

    // Truncate to MAX_CONTEXT_CHARS at a valid char boundary
    let mut end = MAX_CONTEXT_CHARS;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }

    // Prefer breaking at a word boundary
    let word_end = trimmed[..end].rfind(' ').unwrap_or(end);
    trimmed[..word_end].trim_end().to_owned()
}

/// Format a [`SkillContent`] as a compact markdown section for the system prompt.
#[cfg(any(feature = "knowledge-store", test))]
pub(crate) fn format_skill_as_markdown(skill: &aletheia_mneme::skill::SkillContent) -> String {
    use std::fmt::Write as _;

    let mut md = format!("**{}**\n\n{}", skill.name, skill.description);

    if !skill.steps.is_empty() {
        md.push_str("\n\n**Steps:**\n");
        for (i, step) in skill.steps.iter().enumerate() {
            // writeln! on String never returns Err
            let _ = writeln!(md, "{}. {}", i + 1, step);
        }
    }

    if !skill.tools_used.is_empty() {
        let _ = write!(md, "\n**Tools:** {}", skill.tools_used.join(", "));
    }

    if !skill.domain_tags.is_empty() {
        let _ = write!(md, "\n**Tags:** {}", skill.domain_tags.join(", "));
    }

    md
}

// ── knowledge-store-only items ───────────────────────────────────────────────

#[cfg(feature = "knowledge-store")]
use std::sync::Arc;

#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge::Fact;
#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;

#[cfg(feature = "knowledge-store")]
use tracing::{Instrument, warn};

#[cfg(feature = "knowledge-store")]
use crate::bootstrap::{BootstrapSection, SectionPriority};
#[cfg(feature = "knowledge-store")]
use crate::budget::{CharEstimator, TokenEstimator as _};

/// Default number of skills to inject per session.
#[cfg(feature = "knowledge-store")]
pub(crate) const DEFAULT_MAX_SKILLS: usize = 5;

/// Resolves relevant skills from mneme and converts them to bootstrap sections.
///
/// Skill loading is additive and gracefully degrades: if the knowledge store
/// is unavailable or no skills match, the system prompt is assembled without
/// skill sections, preserving existing behaviour in all degraded cases.
#[cfg(feature = "knowledge-store")]
pub(crate) struct SkillLoader {
    knowledge_store: Arc<KnowledgeStore>,
}

#[cfg(feature = "knowledge-store")]
impl SkillLoader {
    /// Create a new loader backed by the given knowledge store.
    pub(crate) fn new(knowledge_store: Arc<KnowledgeStore>) -> Self {
        Self { knowledge_store }
    }

    /// Resolve skills relevant to `task_context` and return bootstrap sections.
    ///
    /// Returns at most `max_skills` sections ordered by relevance. Returns an
    /// empty vec on any error so skill loading never breaks the pipeline.
    ///
    /// # Latency
    ///
    /// Instrumented with a `skill_loader.resolve_skills` tracing span. Target
    /// is < 100 ms warm (typical: 12–57 ms per planning estimates).
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. The inner `spawn_blocking` task runs to completion even if
    /// this future is cancelled; only the result is lost.
    pub(crate) async fn resolve_skills(
        &self,
        nous_id: &str,
        task_context: &str,
        max_skills: usize,
    ) -> Vec<BootstrapSection> {
        let span = tracing::info_span!(
            "skill_loader.resolve_skills",
            nous_id = %nous_id,
            max_skills = max_skills,
            skills_found = tracing::field::Empty,
            elapsed_ms = tracing::field::Empty,
        );
        let start = std::time::Instant::now();

        let sections = self
            .do_resolve(nous_id, task_context, max_skills)
            .instrument(span.clone())
            .await;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "elapsed_ms fits in u64 for any realistic latency"
        )]
        span.record("elapsed_ms", start.elapsed().as_millis() as u64);
        span.record("skills_found", sections.len() as u64);

        sections
    }

    async fn do_resolve(
        &self,
        nous_id: &str,
        task_context: &str,
        max_skills: usize,
    ) -> Vec<BootstrapSection> {
        if task_context.is_empty() || max_skills == 0 {
            return vec![];
        }

        // Fetch 2× candidates to have headroom for ranking
        let fetch_limit = max_skills.saturating_mul(2).max(4);
        let store = Arc::clone(&self.knowledge_store);
        let nous_id_owned = nous_id.to_owned();
        let query = task_context.to_owned();

        let candidates = match tokio::task::spawn_blocking(move || {
            store.search_skills(&nous_id_owned, &query, fetch_limit)
        })
        .await
        {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                warn!(error = %e, "skill search failed, continuing without skills");
                return vec![];
            }
            Err(e) => {
                warn!(error = %e, "skill search task panicked");
                return vec![];
            }
        };

        if candidates.is_empty() {
            tracing::debug!("no skills matched task context");
            return vec![];
        }

        let ranked = rank_skills(candidates);
        let selected: Vec<Fact> = ranked.into_iter().take(max_skills).collect();

        let sections: Vec<BootstrapSection> = selected.iter().map(fact_to_section).collect();

        // Increment access counts in the background — do not block the pipeline
        if !selected.is_empty() {
            let ids: Vec<_> = selected.iter().map(|f| f.id.clone()).collect();
            let store = Arc::clone(&self.knowledge_store);
            let increment_span =
                tracing::info_span!("skill_loader.increment_access", count = ids.len());
            tokio::spawn(
                async move {
                    if let Err(e) = store.increment_access_async(ids).await {
                        warn!(error = %e, "failed to increment skill access counts");
                    }
                }
                .instrument(increment_span),
            );
        }

        sections
    }
}

/// Convert a skill [`Fact`] to a [`BootstrapSection`].
///
/// Tries to parse `content` as JSON [`SkillContent`] and format it as markdown.
/// Falls back to the raw content string if parsing fails (e.g. plain-text skills).
#[cfg(feature = "knowledge-store")]
pub(crate) fn fact_to_section(fact: &Fact) -> BootstrapSection {
    let content = if let Ok(skill) =
        serde_json::from_str::<aletheia_mneme::skill::SkillContent>(&fact.content)
    {
        format_skill_as_markdown(&skill)
    } else {
        fact.content.clone()
    };

    let tokens = CharEstimator.estimate(&content);

    BootstrapSection {
        name: format!("[skill] {}", fact.id),
        priority: SectionPriority::Flexible,
        content,
        tokens,
        truncatable: true,
    }
}

/// Rank skill candidates by a combined score.
///
/// `score = 0.40 × position + 0.35 × confidence + 0.15 × access + 0.10 × recency`
///
/// - **position**: BM25/search rank (first result = 1.0, last = 0.0).
/// - **confidence**: fact confidence from mneme (0.0–1.0).
/// - **access**: normalised `access_count`, capped at 20 accesses.
/// - **recency**: exponential decay with 30-day half-life since last access (or `valid_from`).
#[cfg(feature = "knowledge-store")]
pub(crate) fn rank_skills(candidates: Vec<Fact>) -> Vec<Fact> {
    let total = candidates.len();
    if total <= 1 {
        return candidates;
    }

    let now_secs = jiff::Timestamp::now().as_second();

    let mut scored: Vec<(f64, Fact)> = candidates
        .into_iter()
        .enumerate()
        .map(|(i, fact)| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "array index and length for ranking; sub-LSB precision loss is acceptable"
            )]
            let position_score = 1.0 - (i as f64 / total as f64);
            let confidence = fact.confidence.clamp(0.0, 1.0);

            let access_score = f64::from(fact.access_count.min(20)) / 20.0;

            let reference_secs = fact.last_accessed_at.unwrap_or(fact.valid_from).as_second();
            #[expect(
                clippy::cast_precision_loss,
                reason = "age in seconds converted to days; sub-second precision is not needed"
            )]
            let age_days = ((now_secs - reference_secs).max(0) as f64) / 86_400.0;
            // Half-life of 30 days: recency = 2^(-age/30)
            let recency_score = 2_f64.powf(-age_days / 30.0);

            let score = 0.40 * position_score
                + 0.35 * confidence
                + 0.15 * access_score
                + 0.10 * recency_score;

            (score, fact)
        })
        .collect();

    // Sort descending by score
    scored.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, fact)| fact).collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_task_context ─────────────────────────────────────────────────

    #[test]
    fn extract_task_context_returns_content() {
        let ctx = extract_task_context("Implement a retry loop in Rust");
        assert_eq!(ctx, "Implement a retry loop in Rust");
    }

    #[test]
    fn extract_task_context_trims_whitespace() {
        let ctx = extract_task_context("  debug the config parser   ");
        assert_eq!(ctx, "debug the config parser");
    }

    #[test]
    fn extract_task_context_empty_returns_empty() {
        assert_eq!(extract_task_context(""), "");
        assert_eq!(extract_task_context("   "), "");
    }

    #[test]
    fn extract_task_context_truncates_long_input() {
        let long = "a".repeat(250);
        let ctx = extract_task_context(&long);
        assert!(
            ctx.len() <= MAX_CONTEXT_CHARS,
            "should be truncated to ≤200 chars"
        );
    }

    #[test]
    fn extract_task_context_truncates_at_word_boundary() {
        // 195 chars of "word " + a long word that crosses the boundary
        let prefix = "word ".repeat(39); // 195 chars
        let suffix = "toolongword that keeps going";
        let input = format!("{prefix}{suffix}");
        assert!(input.len() > MAX_CONTEXT_CHARS);

        let ctx = extract_task_context(&input);
        assert!(ctx.len() <= MAX_CONTEXT_CHARS);
        // Should not cut mid-"toolongword"
        assert!(!ctx.ends_with("toolong"));
    }

    #[test]
    fn extract_task_context_exact_boundary_not_truncated() {
        let exact = "x".repeat(MAX_CONTEXT_CHARS);
        let ctx = extract_task_context(&exact);
        assert_eq!(ctx.len(), MAX_CONTEXT_CHARS);
    }

    // ── format_skill_as_markdown ─────────────────────────────────────────────

    fn sample_skill() -> aletheia_mneme::skill::SkillContent {
        aletheia_mneme::skill::SkillContent {
            name: "rust-error-handling".to_owned(),
            description: "How to handle errors in Rust using snafu.".to_owned(),
            steps: vec![
                "Define error enum with snafu".to_owned(),
                "Add context selectors".to_owned(),
                "Use .context() propagation".to_owned(),
            ],
            tools_used: vec!["cargo".to_owned(), "rustc".to_owned()],
            domain_tags: vec!["rust".to_owned(), "errors".to_owned()],
            origin: "manual".to_owned(),
        }
    }

    #[test]
    fn format_skill_includes_name_and_description() {
        let md = format_skill_as_markdown(&sample_skill());
        assert!(md.contains("rust-error-handling"));
        assert!(md.contains("How to handle errors in Rust using snafu."));
    }

    #[test]
    fn format_skill_includes_numbered_steps() {
        let md = format_skill_as_markdown(&sample_skill());
        assert!(md.contains("1. Define error enum with snafu"));
        assert!(md.contains("2. Add context selectors"));
        assert!(md.contains("3. Use .context() propagation"));
    }

    #[test]
    fn format_skill_includes_tools() {
        let md = format_skill_as_markdown(&sample_skill());
        assert!(md.contains("cargo") && md.contains("rustc"));
    }

    #[test]
    fn format_skill_includes_domain_tags() {
        let md = format_skill_as_markdown(&sample_skill());
        assert!(md.contains("rust") && md.contains("errors"));
    }

    #[test]
    fn format_skill_empty_steps_omits_steps_section() {
        let mut skill = sample_skill();
        skill.steps.clear();
        let md = format_skill_as_markdown(&skill);
        assert!(!md.contains("**Steps:**"));
    }

    #[test]
    fn format_skill_empty_tools_omits_tools_section() {
        let mut skill = sample_skill();
        skill.tools_used.clear();
        let md = format_skill_as_markdown(&skill);
        assert!(!md.contains("**Tools:**"));
    }

    // ── fact_to_section and rank_skills (require knowledge-store feature) ────

    #[cfg(feature = "knowledge-store")]
    fn make_fact(id: &str, content: &str, confidence: f64, access_count: u32) -> Fact {
        Fact {
            id: aletheia_mneme::id::FactId::from(id),
            nous_id: "test-agent".to_owned(),
            content: content.to_owned(),
            confidence,
            tier: aletheia_mneme::knowledge::EpistemicTier::Verified,
            valid_from: jiff::Timestamp::now(),
            valid_to: jiff::Timestamp::from_second(i64::MAX / 2).unwrap_or(jiff::Timestamp::now()),
            superseded_by: None,
            source_session_id: None,
            recorded_at: jiff::Timestamp::now(),
            access_count,
            last_accessed_at: None,
            stability_hours: 2190.0,
            fact_type: "skill".to_owned(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn fact_to_section_uses_flexible_priority() {
        let skill_json = serde_json::to_string(&sample_skill()).unwrap();
        let fact = make_fact("fact-1", &skill_json, 0.9, 3);
        let section = fact_to_section(&fact);
        assert_eq!(section.priority, SectionPriority::Flexible);
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn fact_to_section_is_truncatable() {
        let skill_json = serde_json::to_string(&sample_skill()).unwrap();
        let fact = make_fact("fact-1", &skill_json, 0.9, 0);
        let section = fact_to_section(&fact);
        assert!(section.truncatable);
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn fact_to_section_parses_json_skill_content() {
        let skill_json = serde_json::to_string(&sample_skill()).unwrap();
        let fact = make_fact("fact-1", &skill_json, 0.9, 0);
        let section = fact_to_section(&fact);
        assert!(section.content.contains("rust-error-handling"));
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn fact_to_section_falls_back_to_plain_text() {
        let fact = make_fact("fact-2", "plain text skill description", 0.8, 0);
        let section = fact_to_section(&fact);
        assert_eq!(section.content, "plain text skill description");
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn fact_to_section_name_includes_fact_id() {
        let fact = make_fact("my-skill-id", "content", 0.7, 0);
        let section = fact_to_section(&fact);
        assert!(
            section.name.contains("my-skill-id"),
            "section name: {}",
            section.name
        );
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn fact_to_section_has_nonzero_token_estimate() {
        let skill_json = serde_json::to_string(&sample_skill()).unwrap();
        let fact = make_fact("fact-1", &skill_json, 0.9, 0);
        let section = fact_to_section(&fact);
        assert!(section.tokens > 0);
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn rank_skills_empty_returns_empty() {
        let ranked = rank_skills(vec![]);
        assert!(ranked.is_empty());
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn rank_skills_single_passes_through() {
        let fact = make_fact("f1", "content", 0.9, 0);
        let ranked = rank_skills(vec![fact]);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].id.as_str(), "f1");
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn rank_skills_preserves_all_facts() {
        let facts: Vec<Fact> = (0..10)
            .map(|i| make_fact(&format!("f{i}"), "content", 0.5, 0))
            .collect();
        let ranked = rank_skills(facts);
        assert_eq!(ranked.len(), 10);
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn rank_skills_high_confidence_can_overcome_lower_position() {
        // First item: high position score but zero confidence
        // Second item: lower position score but high confidence
        let low_conf = make_fact("low", "content", 0.0, 0);
        let high_conf = make_fact("high", "content", 1.0, 20); // also high access
        // Feed low_conf first (better BM25 position) so ranking must consider confidence
        let ranked = rank_skills(vec![low_conf, high_conf]);
        // High confidence + high access_count should win despite lower position
        assert_eq!(ranked[0].id.as_str(), "high");
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn rank_skills_returns_sorted_order() {
        // Sanity check: ranking returns the right number and doesn't panic
        let facts: Vec<Fact> = vec![
            make_fact("a", "content", 0.9, 5),
            make_fact("b", "content", 0.1, 1),
            make_fact("c", "content", 0.5, 10),
        ];
        let ranked = rank_skills(facts);
        assert_eq!(ranked.len(), 3, "all facts preserved");
        // Each fact id must appear exactly once
        let ids: Vec<&str> = ranked.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
        assert!(ids.contains(&"c"));
    }
}
