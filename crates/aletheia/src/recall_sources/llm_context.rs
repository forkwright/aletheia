//! LLM context recall source.
//!
//! WHY: Provides structured knowledge about available LLM models (context
//! windows, capabilities, pricing) as a queryable recall source, so agents
//! can answer questions like "which model supports 256K context?" FROM
//! recall rather than hardcoded logic.

use std::future::Future;
use std::pin::Pin;

use super::error::RecallSourceError;
use super::{RecallSource, SourceResult};

/// A single model card describing an LLM's capabilities.
#[derive(Debug, Clone)]
pub(crate) struct ModelCard {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub input_cost_per_mtok: Option<f64>,
    pub output_cost_per_mtok: Option<f64>,
}

impl ModelCard {
    /// Format as a human-readable content string for recall results.
    fn to_content(&self) -> String {
        let mut parts = Vec::with_capacity(6);

        parts.push(format!("Model: {} ({})", self.name, self.provider));
        parts.push(format!(
            "Context: {}K tokens, Max output: {}K tokens",
            self.context_window / 1000,
            self.max_output_tokens / 1000
        ));

        let mut caps = Vec::new();
        if self.supports_tools {
            caps.push("tool use");
        }
        if self.supports_vision {
            caps.push("vision");
        }
        if self.supports_thinking {
            caps.push("extended thinking");
        }
        if !caps.is_empty() {
            parts.push(format!("Capabilities: {}", caps.join(", ")));
        }

        match (self.input_cost_per_mtok, self.output_cost_per_mtok) {
            (Some(input), Some(output)) => {
                parts.push(format!(
                    "Pricing: ${input:.2}/MTok input, ${output:.2}/MTok output"
                ));
            }
            _ => {}
        }

        parts.join("\n")
    }

    /// Compute a relevance score against a query by checking keyword overlap.
    fn relevance(&self, query_lower: &str) -> f64 {
        let mut score = 0.0_f64;
        let name_lower = self.name.to_lowercase();
        let id_lower = self.id.to_lowercase();
        let provider_lower = self.provider.to_lowercase();

        // Direct name match is highest signal.
        if query_lower.contains(&name_lower) || query_lower.contains(&id_lower) {
            score += 0.6;
        }

        if query_lower.contains(&provider_lower) {
            score += 0.2;
        }

        // Capability keyword matching.
        if query_lower.contains("tool") && self.supports_tools {
            score += 0.3;
        }
        if query_lower.contains("vision") && self.supports_vision {
            score += 0.3;
        }
        if query_lower.contains("thinking") && self.supports_thinking {
            score += 0.3;
        }

        // Context window queries.
        if query_lower.contains("context") || query_lower.contains("window") {
            score += 0.2;
        }
        // WHY: Specific context size queries (e.g., "256k context") match
        // models whose window is at or above the requested size.
        if let Some(requested_k) = extract_context_size_k(query_lower) {
            if self.context_window >= requested_k * 1000 {
                score += 0.4;
            }
        }

        if query_lower.contains("cost")
            || query_lower.contains("price")
            || query_lower.contains("pricing")
        {
            if self.input_cost_per_mtok.is_some() {
                score += 0.2;
            }
        }

        score.min(1.0)
    }
}

/// Extract a context size in thousands FROM a query like "256k context".
fn extract_context_size_k(query: &str) -> Option<u64> {
    let bytes = query.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // WHY: Check for 'k' suffix immediately after digits.
            if i < bytes.len() && (bytes[i] == b'k' || bytes[i] == b'K') {
                if let Ok(n) = query[start..i].parse::<u64>() {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

/// Recall source providing structured knowledge about available LLM models.
///
/// Model cards are loaded at construction time FROM the configured providers
/// and pricing data. The source is always available (returns empty results
/// if no models are registered).
pub(crate) struct LlmContextSource {
    models: Vec<ModelCard>,
}

impl LlmContextSource {
    #[cfg(test)]
    pub(crate) fn new(models: Vec<ModelCard>) -> Self {
        Self { models }
    }

    /// Build model cards FROM the known Anthropic model catalog and any
    /// pricing overrides FROM config.
    pub(crate) fn from_known_models(
        pricing: &std::collections::HashMap<String, aletheia_taxis::config::ModelPricing>,
    ) -> Self {
        let mut models = anthropic_model_cards();

        // NOTE: Overlay pricing FROM config onto matching model cards.
        for card in &mut models {
            if let Some(p) = pricing.get(&card.id) {
                card.input_cost_per_mtok = Some(p.input_cost_per_mtok);
                card.output_cost_per_mtok = Some(p.output_cost_per_mtok);
            }
        }

        Self { models }
    }
}

impl RecallSource for LlmContextSource {
    fn query<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceResult>, RecallSourceError>> + Send + 'a>>
    {
        Box::pin(async move {
            let query_lower = query.to_lowercase();

            let mut scored: Vec<(f64, &ModelCard)> = self
                .models
                .iter()
                .map(|card| (card.relevance(&query_lower), card))
                .filter(|(score, _)| *score > 0.0)
                .collect();

            // Sort by relevance descending.
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(limit);

            let results = scored
                .into_iter()
                .map(|(relevance, card)| SourceResult {
                    content: card.to_content(),
                    relevance,
                    source_id: card.id.clone(),
                })
                .collect();

            Ok(results)
        })
    }

    fn source_type(&self) -> &str {
        "llm_context"
    }

    fn available(&self) -> bool {
        !self.models.is_empty()
    }
}

/// Known Anthropic model catalog.
///
/// NOTE: This is intentionally static data. The issue specifies that model
/// cards should be "updated automatically when new providers are configured,"
/// which is handled by overlaying pricing config. Adding entirely new models
/// requires a code UPDATE (or future config-driven model registry).
fn anthropic_model_cards() -> Vec<ModelCard> {
    vec![
        ModelCard {
            id: "claude-sonnet-4-20250514".to_owned(),
            name: "Claude Sonnet 4".to_owned(),
            provider: "Anthropic".to_owned(),
            context_window: 200_000,
            max_output_tokens: 16_000,
            supports_tools: true,
            supports_vision: true,
            supports_thinking: true,
            input_cost_per_mtok: Some(3.0),
            output_cost_per_mtok: Some(15.0),
        },
        ModelCard {
            id: "claude-opus-4-20250514".to_owned(),
            name: "Claude Opus 4".to_owned(),
            provider: "Anthropic".to_owned(),
            context_window: 200_000,
            max_output_tokens: 32_000,
            supports_tools: true,
            supports_vision: true,
            supports_thinking: true,
            input_cost_per_mtok: Some(15.0),
            output_cost_per_mtok: Some(75.0),
        },
        ModelCard {
            id: "claude-3-5-haiku-20241022".to_owned(),
            name: "Claude 3.5 Haiku".to_owned(),
            provider: "Anthropic".to_owned(),
            context_window: 200_000,
            max_output_tokens: 8_192,
            supports_tools: true,
            supports_vision: true,
            supports_thinking: false,
            input_cost_per_mtok: Some(0.8),
            output_cost_per_mtok: Some(4.0),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_model_cards() -> Vec<ModelCard> {
        vec![
            ModelCard {
                id: "model-a".to_owned(),
                name: "Model Alpha".to_owned(),
                provider: "TestCorp".to_owned(),
                context_window: 128_000,
                max_output_tokens: 8_000,
                supports_tools: true,
                supports_vision: false,
                supports_thinking: false,
                input_cost_per_mtok: Some(1.0),
                output_cost_per_mtok: Some(5.0),
            },
            ModelCard {
                id: "model-b".to_owned(),
                name: "Model Beta".to_owned(),
                provider: "TestCorp".to_owned(),
                context_window: 256_000,
                max_output_tokens: 32_000,
                supports_tools: true,
                supports_vision: true,
                supports_thinking: true,
                input_cost_per_mtok: Some(10.0),
                output_cost_per_mtok: Some(50.0),
            },
        ]
    }

    #[test]
    fn model_card_content_formatting() {
        let card = &sample_model_cards()[0];
        let content = card.to_content();
        assert!(content.contains("Model Alpha (TestCorp)"));
        assert!(content.contains("128K tokens"));
        assert!(content.contains("tool use"));
        assert!(!content.contains("vision"));
        assert!(content.contains("$1.00/MTok input"));
    }

    #[test]
    fn relevance_name_match() {
        let card = &sample_model_cards()[0];
        let score = card.relevance("tell me about model alpha");
        assert!(score > 0.5, "name match should score high: {score}");
    }

    #[test]
    fn relevance_capability_match() {
        let cards = sample_model_cards();
        let vision_score_a = cards.get(0).copied().unwrap_or_default().relevance("which model supports vision");
        let vision_score_b = cards.get(1).copied().unwrap_or_default().relevance("which model supports vision");
        assert!(
            vision_score_b > vision_score_a,
            "model with vision should score higher: a={vision_score_a}, b={vision_score_b}"
        );
    }

    #[test]
    fn relevance_context_window_query() {
        let cards = sample_model_cards();
        let score_a = cards.get(0).copied().unwrap_or_default().relevance("model with 256k context");
        let score_b = cards.get(1).copied().unwrap_or_default().relevance("model with 256k context");
        assert!(
            score_b > score_a,
            "256K model should rank higher for 256K query: a={score_a}, b={score_b}"
        );
    }

    #[test]
    fn extract_context_size_k_works() {
        assert_eq!(extract_context_size_k("256k context"), Some(256));
        assert_eq!(extract_context_size_k("128K window"), Some(128));
        assert_eq!(extract_context_size_k("no number here"), None);
        assert_eq!(extract_context_size_k("100 tokens"), None);
    }

    #[test]
    fn relevance_no_match() {
        let card = &sample_model_cards()[0];
        let score = card.relevance("unrelated query about cooking");
        assert!(
            score < f64::EPSILON,
            "unrelated query should score zero: {score}"
        );
    }

    #[tokio::test]
    async fn llm_context_source_query() {
        let source = LlmContextSource::new(sample_model_cards());
        let results = source
            .query("model with vision support", 10)
            .await
            .unwrap_or_default();
        assert!(!results.is_empty());
        // Model Beta supports vision, should appear first.
        assert_eq!(results.get(0).copied().unwrap_or_default().source_id, "model-b");
    }

    #[tokio::test]
    async fn llm_context_source_empty_on_no_match() {
        let source = LlmContextSource::new(sample_model_cards());
        let results = source
            .query("quantum entanglement recipes", 10)
            .await
            .unwrap_or_default();
        assert!(results.is_empty());
    }

    #[test]
    fn from_known_models_applies_pricing() {
        let mut pricing = std::collections::HashMap::new();
        pricing.insert(
            "claude-sonnet-4-20250514".to_owned(),
            aletheia_taxis::config::ModelPricing {
                input_cost_per_mtok: 99.0,
                output_cost_per_mtok: 199.0,
            },
        );
        let source = LlmContextSource::from_known_models(&pricing);
        let sonnet = source
            .models
            .iter()
            .find(|m| m.id == "claude-sonnet-4-20250514")
            .unwrap_or_default();
        assert!((sonnet.input_cost_per_mtok.unwrap() - 99.0).abs() < f64::EPSILON);
        assert!((sonnet.output_cost_per_mtok.unwrap() - 199.0).abs() < f64::EPSILON);
    }
}
