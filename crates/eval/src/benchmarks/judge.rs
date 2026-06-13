//! LLM-as-judge scorer for benchmark answer correctness.
//!
//! Implements a binary correct/incorrect judgment via an external evaluator
//! LLM. This is the standard evaluation protocol for LongMemEval
//! comparability (paper uses GPT-4 as judge).
//!
//! The judge sends a structured prompt to an OpenAI-compatible chat
//! completions endpoint and records provenance for every attempted judgment.

use std::time::Duration;

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
    /// Judge execution status.
    #[serde(default)]
    pub status: JudgeStatus,
    /// Error detail when the judge did not produce a parsed score.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error_message: Option<String>,
    /// Provider and parsing provenance for the judge call.
    pub provenance: JudgeProvenance,
}

impl JudgeScore {
    fn scored(correct: bool, reasoning: String, provenance: JudgeProvenance) -> Self {
        Self {
            correct,
            reasoning,
            status: JudgeStatus::Scored,
            error_message: None,
            provenance,
        }
    }

    fn error(message: String, provenance: JudgeProvenance) -> Self {
        Self {
            correct: false,
            reasoning: message.clone(),
            status: JudgeStatus::Error,
            error_message: Some(message),
            provenance,
        }
    }

    pub(crate) fn configuration_error(
        config: &LlmJudgeConfig,
        question: &str,
        actual: &str,
        expected: &[String],
        message: String,
    ) -> Self {
        let prompt = build_judge_prompt(question, actual, expected);
        Self::error(
            message,
            JudgeProvenance {
                endpoint: config.endpoint.clone(),
                model: config.model.clone(),
                prompt_sha256: crate::provenance::sha256_hex_str(&prompt),
                raw_response_sha256: None,
                raw_response_body_ref: None,
                request_id: None,
                usage: None,
                provider_status: None,
                parse_status: JudgeParseStatus::TransportError,
            },
        )
    }
}

/// Judge execution status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum JudgeStatus {
    /// The judge returned a parsed judgment.
    #[default]
    Scored,
    /// The judge attempt failed or returned unparseable/refusal data.
    Error,
}

impl JudgeStatus {
    /// Whether this status represents a parsed judge score.
    #[must_use]
    pub fn is_scored(self) -> bool {
        matches!(self, Self::Scored)
    }
}

/// Parse/provider status for a judge attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum JudgeParseStatus {
    /// Provider response parsed into a judge verdict.
    Parsed,
    /// The provider returned a non-success HTTP status.
    HttpError,
    /// The HTTP request or body read failed before parsing.
    TransportError,
    /// The request exceeded the configured judge timeout.
    Timeout,
    /// The provider response JSON lacked message content.
    MissingContent,
    /// The provider or judge JSON was malformed.
    MalformedJson,
    /// The provider returned a refusal instead of judgment content.
    Refusal,
}

/// Token usage reported by the judge provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeUsage {
    /// Prompt/input tokens.
    pub prompt_tokens: u64,
    /// Completion/output tokens.
    pub completion_tokens: u64,
    /// Total tokens.
    pub total_tokens: u64,
}

/// Provenance for an LLM judge call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeProvenance {
    /// OpenAI-compatible endpoint.
    pub endpoint: String,
    /// Judge model.
    pub model: String,
    /// SHA-256 hash of the full user prompt sent to the judge.
    pub prompt_sha256: String,
    /// SHA-256 hash of the raw provider response body.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_response_sha256: Option<String>,
    /// Body reference for external storage; currently the response hash URN.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_response_body_ref: Option<String>,
    /// Provider request ID from headers or response JSON.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub request_id: Option<String>,
    /// Provider token usage, when reported.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<JudgeUsage>,
    /// HTTP status returned by the provider.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_status: Option<u16>,
    /// Parse/provider outcome.
    pub parse_status: JudgeParseStatus,
}

impl JudgeProvenance {
    fn with_body_ref(mut self, body_ref: Option<String>) -> Self {
        self.raw_response_body_ref = body_ref;
        self
    }
}

/// Configuration for the LLM judge endpoint.
#[derive(Clone)]
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
    /// Explicit HTTP timeout for one judge request.
    pub timeout: Duration,
}

impl core::fmt::Debug for LlmJudgeConfig {
    // WHY: hand-rolled Debug redacts api_key so structured logs / panics never leak the bearer token.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LlmJudgeConfig")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("timeout", &self.timeout)
            .finish()
    }
}

// kanon:ignore RUST/hardcoded-model — default judge model constant for LLM-as-judge evaluator
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
            timeout: Duration::from_secs(30),
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
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub(crate) fn new(config: LlmJudgeConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .context(error::HttpSnafu)?;
        Ok(Self { client, config })
    }

    /// Ask the evaluator LLM whether `actual` correctly answers `question`
    /// given all acceptable ground-truth answers.
    pub async fn judge(&self, question: &str, actual: &str, expected: &[String]) -> JudgeScore {
        let prompt = build_judge_prompt(question, actual, expected);
        let prompt_sha256 = crate::provenance::sha256_hex_str(&prompt);
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

        match req.send().await {
            Ok(resp) => self.score_response(prompt_sha256, resp).await,
            Err(e) => {
                let parse_status = if e.is_timeout() {
                    JudgeParseStatus::Timeout
                } else {
                    JudgeParseStatus::TransportError
                };
                JudgeScore::error(
                    format!("judge request failed: {e}"),
                    self.provenance(prompt_sha256, parse_status, None, None, None, None),
                )
            }
        }
    }

    async fn score_response(&self, prompt_sha256: String, resp: reqwest::Response) -> JudgeScore {
        let provider_status = Some(resp.status().as_u16());
        let header_request_id = response_request_id(resp.headers());
        let body_text = match resp.text().await {
            Ok(body) => body,
            Err(e) => {
                let parse_status = if e.is_timeout() {
                    JudgeParseStatus::Timeout
                } else {
                    JudgeParseStatus::TransportError
                };
                return JudgeScore::error(
                    format!("failed to read judge response body: {e}"),
                    self.provenance(
                        prompt_sha256,
                        parse_status,
                        provider_status,
                        None,
                        header_request_id,
                        None,
                    ),
                );
            }
        };
        let body_hash = crate::provenance::sha256_hex_str(&body_text);
        let body_ref = Some(format!("sha256:{body_hash}"));
        let raw_response_sha256 = Some(body_hash);

        if !provider_status.is_some_and(http_status_success) {
            return JudgeScore::error(
                "judge provider returned non-success status".to_owned(),
                self.provenance(
                    prompt_sha256,
                    JudgeParseStatus::HttpError,
                    provider_status,
                    raw_response_sha256,
                    header_request_id,
                    None,
                )
                .with_body_ref(body_ref),
            );
        }

        let json: serde_json::Value = match serde_json::from_str(&body_text) {
            Ok(json) => json,
            Err(e) => {
                return JudgeScore::error(
                    format!("failed to parse judge provider JSON: {e}"),
                    self.provenance(
                        prompt_sha256,
                        JudgeParseStatus::MalformedJson,
                        provider_status,
                        raw_response_sha256,
                        header_request_id,
                        None,
                    )
                    .with_body_ref(body_ref),
                );
            }
        };
        let usage = parse_usage(&json);
        let request_id = header_request_id.or_else(|| json_id(&json));

        match parse_judge_response(&json) {
            Ok(parsed) => JudgeScore::scored(
                parsed.correct,
                parsed.reasoning,
                self.provenance(
                    prompt_sha256,
                    JudgeParseStatus::Parsed,
                    provider_status,
                    raw_response_sha256,
                    request_id,
                    usage,
                )
                .with_body_ref(body_ref),
            ),
            Err(e) => JudgeScore::error(
                e.message,
                self.provenance(
                    prompt_sha256,
                    e.status,
                    provider_status,
                    raw_response_sha256,
                    request_id,
                    usage,
                )
                .with_body_ref(body_ref),
            ),
        }
    }

    fn provenance(
        &self,
        prompt_sha256: String,
        parse_status: JudgeParseStatus,
        provider_status: Option<u16>,
        raw_response_sha256: Option<String>,
        request_id: Option<String>,
        usage: Option<JudgeUsage>,
    ) -> JudgeProvenance {
        JudgeProvenance {
            endpoint: self.config.endpoint.clone(),
            model: self.config.model.clone(),
            prompt_sha256,
            raw_response_sha256,
            raw_response_body_ref: None,
            request_id,
            usage,
            provider_status,
            parse_status,
        }
    }
}

const JUDGE_SYSTEM_PROMPT: &str = r#"You are an expert evaluator for question-answering benchmarks.
Your task is to judge whether a given answer correctly responds to the question, using all provided ground-truth answers as acceptable references.

Respond with a single JSON object containing exactly two keys:
- "correct": boolean — true if the answer is correct, false otherwise
- "reasoning": string — a one-sentence explanation of your judgment

Be strict: the answer must contain the core fact from at least one acceptable ground truth. Minor paraphrasing or additional context is acceptable, but missing the key fact is not."#;

fn build_judge_prompt(question: &str, actual: &str, expected: &[String]) -> String {
    let expected_block = if expected.is_empty() {
        "No ground-truth answers supplied.".to_owned()
    } else {
        expected
            .iter()
            .enumerate()
            .map(|(index, answer)| format!("{}. {answer}", index + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Question: {question}\nAcceptable ground-truth answers:\n{expected_block}\nActual answer: {actual}\n\nJudge whether the actual answer is correct. Output JSON only."
    )
}

fn parse_judge_response(
    json: &serde_json::Value,
) -> std::result::Result<JudgeResponse, ParseFailure> {
    let message = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| ParseFailure {
            status: JudgeParseStatus::MissingContent,
            message: "missing content in judge response".to_owned(),
        })?;

    if let Some(refusal) = message.get("refusal").and_then(|value| value.as_str())
        && !refusal.trim().is_empty()
    {
        return Err(ParseFailure {
            status: JudgeParseStatus::Refusal,
            message: format!("judge refused: {refusal}"),
        });
    }

    let content = message
        .get("content")
        .and_then(|c| c.as_str())
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| ParseFailure {
            status: JudgeParseStatus::MissingContent,
            message: "missing content in judge response".to_owned(),
        })?;

    serde_json::from_str(strip_code_fences(content)).map_err(|e| ParseFailure {
        status: JudgeParseStatus::MalformedJson,
        message: format!("failed to parse judge JSON: {e}"),
    })
}

fn strip_code_fences(content: &str) -> &str {
    content
        .trim()
        .strip_prefix("```json")
        .or_else(|| content.trim().strip_prefix("```"))
        .map_or(content.trim(), |s| {
            s.strip_suffix("```").unwrap_or(s).trim()
        })
}

fn response_request_id(headers: &reqwest::header::HeaderMap) -> Option<String> {
    ["x-request-id", "request-id", "openai-request-id"]
        .iter()
        .find_map(|name| {
            headers
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned)
        })
}

fn json_id(json: &serde_json::Value) -> Option<String> {
    json.get("id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn parse_usage(json: &serde_json::Value) -> Option<JudgeUsage> {
    let usage = json.get("usage")?;
    Some(JudgeUsage {
        prompt_tokens: usage.get("prompt_tokens")?.as_u64()?,
        completion_tokens: usage.get("completion_tokens")?.as_u64()?,
        total_tokens: usage.get("total_tokens")?.as_u64()?,
    })
}

fn http_status_success(status: u16) -> bool {
    (200..300).contains(&status)
}

#[derive(Debug, Deserialize)]
struct JudgeResponse {
    correct: bool,
    reasoning: String,
}

#[derive(Debug)]
struct ParseFailure {
    status: JudgeParseStatus,
    message: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn init_crypto() {
        // install_default() is idempotent: subsequent calls return Err and are ignored.
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn build_prompt_contains_all_expected_answers() {
        let prompt = build_judge_prompt(
            "What color?",
            "blue",
            &["blue".to_owned(), "azure".to_owned()],
        );
        assert!(prompt.contains("What color?"));
        assert!(prompt.contains("1. blue"));
        assert!(prompt.contains("2. azure"));
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
        let err = parse_judge_response(&json).unwrap_err();
        assert_eq!(err.status, JudgeParseStatus::MissingContent);
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
        let err = parse_judge_response(&json).unwrap_err();
        assert_eq!(err.status, JudgeParseStatus::MalformedJson);
    }

    #[test]
    fn parse_judge_response_refusal_fails() {
        let json = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "refusal": "I cannot judge this."
                    }
                }
            ]
        });
        let err = parse_judge_response(&json).unwrap_err();
        assert_eq!(err.status, JudgeParseStatus::Refusal);
    }

    #[tokio::test]
    async fn judge_records_malformed_json_as_error_score() {
        init_crypto();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/judge"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let judge = LlmJudge::new(LlmJudgeConfig {
            endpoint: format!("{}/judge", server.uri()),
            model: "judge-model".to_owned(),
            timeout: Duration::from_secs(1),
            ..LlmJudgeConfig::default()
        })
        .expect("judge client");
        let score = judge
            .judge("question", "answer", &["expected".to_owned()])
            .await;

        assert_eq!(score.status, JudgeStatus::Error);
        assert_eq!(
            score.provenance.parse_status,
            JudgeParseStatus::MalformedJson
        );
        assert_eq!(score.provenance.provider_status, Some(200));
        assert!(score.provenance.raw_response_sha256.is_some());
    }

    #[tokio::test]
    async fn judge_records_timeout_as_error_score() {
        init_crypto();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/judge"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(100))
                    .set_body_json(serde_json::json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "{\"correct\":true,\"reasoning\":\"ok\"}"
                                }
                            }
                        ]
                    })),
            )
            .mount(&server)
            .await;

        let judge = LlmJudge::new(LlmJudgeConfig {
            endpoint: format!("{}/judge", server.uri()),
            model: "judge-model".to_owned(),
            timeout: Duration::from_millis(10),
            ..LlmJudgeConfig::default()
        })
        .expect("judge client");
        let score = judge
            .judge("question", "answer", &["expected".to_owned()])
            .await;

        assert_eq!(score.status, JudgeStatus::Error);
        assert_eq!(score.provenance.parse_status, JudgeParseStatus::Timeout);
    }

    #[test]
    fn llm_judge_config_defaults() {
        let config = LlmJudgeConfig::default();
        assert!(config.temperature.abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, 256);
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.model, "gpt-4o");
    }
}
