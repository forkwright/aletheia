//! LLM-as-judge scorer for benchmark answer correctness.
//!
//! Implements a binary correct/incorrect judgment via an external evaluator
//! LLM. This is the standard evaluation protocol for LongMemEval
//! comparability (paper uses GPT-4 as judge).
//!
//! The judge sends a structured prompt to an OpenAI-compatible chat
//! completions endpoint and parses the response for a verdict.

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error::{self, Result};

/// Result of an LLM-as-judge evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeScore {
    /// Whether the judge deemed the answer correct.
    pub correct: bool,
    /// Short reasoning provided by the judge.
    pub reasoning: String,
}

/// Configuration for the LLM judge endpoint.
#[derive(Debug, Clone)]
pub struct LlmJudgeConfig {
    /// OpenAI-compatible chat completions URL.
    pub endpoint: String,
    /// Model identifier (e.g. "gpt-4o", "claude-3-5-sonnet").
    pub model: String,
    /// Optional API key.
    pub api_key: Option<String>,
    /// Maximum tokens for the judgment response.
    pub max_tokens: u32,
    /// Temperature (default 0.0 for deterministic judging).
    pub temperature: f32,
}

/// Default LLM-judge model. Overridable via [`LlmJudgeConfig::model`].
pub const DEFAULT_JUDGE_MODEL: &str = "gpt-4o";

/// Default LLM-judge endpoint.
pub(crate) const DEFAULT_JUDGE_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";

impl Default for LlmJudgeConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_JUDGE_ENDPOINT.to_owned(),
            model: DEFAULT_JUDGE_MODEL.to_owned(),
            api_key: None,
            max_tokens: 256,
            temperature: 0.0,
        }
    }
}

/// LLM-as-judge evaluator.
pub(crate) struct LlmJudge {
    client: reqwest::Client,
    config: LlmJudgeConfig,
}

impl LlmJudge {
    /// Create a new judge with the given configuration.
    #[must_use]
    pub(crate) fn new(config: LlmJudgeConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    /// Ask the evaluator LLM whether `actual` correctly answers `question`
    /// given the ground-truth `expected`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the response is malformed,
    /// or the judge returns an unexpected format.
    pub async fn judge(&self, question: &str, actual: &str, expected: &str) -> Result<JudgeScore> {
        let prompt = build_judge_prompt(question, actual, expected);
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                { "role": "system", "content": JUDGE_SYSTEM_PROMPT },
                { "role": "user", "content": prompt }
            ],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
        });

        let mut req = self
            .client
            .post(&self.config.endpoint)
            .header("content-type", "application/json")
            .json(&body);
        if let Some(ref key) = self.config.api_key {
            req = req.header("authorization", format!("Bearer {key}"));
        }

        let resp = req.send().await.context(error::HttpSnafu)?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body_text = resp.text().await.unwrap_or_default();
            return error::UnexpectedStatusSnafu {
                endpoint: self.config.endpoint.clone(),
                status,
                body: body_text,
            }
            .fail();
        }

        let json: serde_json::Value = resp.json().await.context(error::HttpSnafu)?;
        parse_judge_response(&json)
    }
}

const JUDGE_SYSTEM_PROMPT: &str = r#"You are an expert evaluator for question-answering benchmarks.
Your task is to judge whether a given answer correctly responds to the question, using the provided ground-truth answer as reference.

Respond with a single JSON object containing exactly two keys:
- "correct": boolean — true if the answer is correct, false otherwise
- "reasoning": string — a one-sentence explanation of your judgment

Be strict: the answer must contain the core fact from the ground truth. Minor paraphrasing or additional context is acceptable, but missing the key fact is not."#;

fn build_judge_prompt(question: &str, actual: &str, expected: &str) -> String {
    format!(
        "Question: {question}\nGround-truth answer: {expected}\nActual answer: {actual}\n\nJudge whether the actual answer is correct. Output JSON only."
    )
}

fn parse_judge_response(json: &serde_json::Value) -> Result<JudgeScore> {
    let content = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| {
            error::SseParseSnafu {
                message: "missing content in judge response".to_owned(),
            }
            .build()
        })?;

    // The content may be wrapped in markdown code fences; strip them.
    let cleaned = content
        .trim()
        .strip_prefix("```json")
        .or_else(|| content.trim().strip_prefix("```"))
        .map_or(content.trim(), |s| {
            s.strip_suffix("```").unwrap_or(s).trim()
        });

    let parsed: JudgeResponse = serde_json::from_str(cleaned).map_err(|e| {
        error::SseParseSnafu {
            message: format!("failed to parse judge JSON: {e}"),
        }
        .build()
    })?;

    Ok(JudgeScore {
        correct: parsed.correct,
        reasoning: parsed.reasoning,
    })
}

#[derive(Debug, Deserialize)]
struct JudgeResponse {
    correct: bool,
    reasoning: String,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_contains_all_parts() {
        let prompt = build_judge_prompt("What color?", "blue", "blue");
        assert!(prompt.contains("What color?"));
        assert!(prompt.contains("blue"));
        assert!(prompt.contains("Judge whether"));
    }

    #[test]
    fn parse_valid_judge_response() {
        let json = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": "{\"correct\":true,\"reasoning\":\"The answer matches the ground truth.\"}"
                    }
                }
            ]
        });
        let score = parse_judge_response(&json).expect("should parse");
        assert!(score.correct);
        assert_eq!(score.reasoning, "The answer matches the ground truth.");
    }

    #[test]
    fn parse_judge_response_with_markdown_fences() {
        let json = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": "```json\n{\"correct\":false,\"reasoning\":\"Missing key fact.\"}\n```"
                    }
                }
            ]
        });
        let score = parse_judge_response(&json).expect("should parse");
        assert!(!score.correct);
        assert_eq!(score.reasoning, "Missing key fact.");
    }

    #[test]
    fn parse_judge_response_missing_choices_fails() {
        let json = serde_json::json!({"id": "123"});
        assert!(parse_judge_response(&json).is_err());
    }

    #[test]
    fn parse_judge_response_malformed_json_fails() {
        let json = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": "not json"
                    }
                }
            ]
        });
        assert!(parse_judge_response(&json).is_err());
    }

    #[test]
    fn llm_judge_config_defaults() {
        let config = LlmJudgeConfig::default();
        assert!(config.temperature.abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, 256);
        assert_eq!(config.model, "gpt-4o");
    }
}
