//! Cross-chunk contradiction detection via LLM comparison.

use std::fmt::Write;

use snafu::ResultExt;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{CompletionRequest, Content, ContentBlock, Message, Role};

use crate::error::LlmCallSnafu;

const CONTRADICTION_SYSTEM_PROMPT: &str = "\
You are reviewing a numbered list of facts extracted from a conversation.\n\
\n\
Your task: identify pairs of facts that directly contradict each other.\n\
\n\
A contradiction means one fact directly negates or conflicts with another \
not merely a nuance or update, but an outright logical conflict.\n\
\n\
Examples of contradictions:\n\
- \"User prefers coffee\" vs \"User dislikes all hot beverages\"\n\
- \"Meeting scheduled for Monday\" vs \"Meeting was cancelled\"\n\
- \"Server runs on port 8080\" vs \"Server runs on port 3000\"\n\
\n\
Examples that are NOT contradictions:\n\
- Two facts about different topics\n\
- A general statement and a specific case\n\
- Two facts that can both be true simultaneously\n\
\n\
Return ONLY valid JSON with this structure:\n\
{\"contradictions\": [{\"chunk_a\": 1, \"chunk_b\": 3, \"description\": \"fact 1 says X but fact 3 says Y\"}]}\n\
\n\
If no contradictions found: {\"contradictions\": []}\n\
\n\
Do not include explanations, markdown, or text outside the JSON.";

/// A detected contradiction between two chunks.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Contradiction {
    /// 0-based index of the first conflicting chunk.
    pub chunk_a: usize,
    /// 0-based index of the second conflicting chunk.
    pub chunk_b: usize,
    /// Human-readable description of the contradiction.
    pub description: String,
}

/// How a contradiction should be resolved.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum ResolutionStrategy {
    /// Prefer the more recent fact (higher index).
    PreferNewer,
    /// Requires explicit user review.
    NeedsUserReview,
}

/// Log of contradictions detected during a distillation pass.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContradictionLog {
    /// Detected contradictions.
    pub contradictions: Vec<Contradiction>,
    /// When detection was performed (ISO 8601).
    pub timestamp: String,
    /// Suggested resolution strategy.
    pub resolution_strategy: ResolutionStrategy,
}

impl ContradictionLog {
    /// An empty log with no contradictions.
    #[must_use]
    pub(crate) fn empty() -> Self {
        Self {
            contradictions: vec![],
            timestamp: String::new(),
            resolution_strategy: ResolutionStrategy::PreferNewer,
        }
    }

    /// Whether any contradictions were detected.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.contradictions.is_empty()
    }
}

/// Detect contradictions across text chunks using an LLM.
///
/// Sends all chunks in a single LLM call as a numbered list and parses the
/// response for identified contradictions. Chunks with fewer than 2 entries
/// skip the LLM call entirely.
///
/// # Errors
///
/// Returns `LlmCall` if the provider request fails.
pub(crate) async fn detect_contradictions(
    chunks: &[String],
    provider: &dyn LlmProvider,
    model: &str,
) -> crate::error::Result<ContradictionLog> {
    if chunks.len() < 2 {
        return Ok(ContradictionLog::empty());
    }

    let mut numbered = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let _ = writeln!(numbered, "{}. {chunk}", i + 1);
    }

    let request = CompletionRequest {
        model: model.to_owned(),
        system: Some(CONTRADICTION_SYSTEM_PROMPT.to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text(format!("Facts to review:\n{numbered}")),
        }],
        max_tokens: 1024,
        temperature: Some(0.0),
        ..Default::default()
    };

    let response = provider.complete(&request).await.context(LlmCallSnafu)?;

    let text = extract_response_text(&response.content);
    let contradictions = parse_contradictions(&text);
    let timestamp = jiff::Timestamp::now().to_string();

    let resolution_strategy = if contradictions.is_empty() {
        ResolutionStrategy::PreferNewer
    } else {
        ResolutionStrategy::NeedsUserReview
    };

    Ok(ContradictionLog {
        contradictions,
        timestamp,
        resolution_strategy,
    })
}

/// Extract plain text from response content blocks.
fn extract_response_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Find the outermost JSON object in text that may contain markdown fences.
fn extract_json_substring(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    // WHY: '{' and '}' are single-byte ASCII, so find() returns valid UTF-8 boundaries.
    text.get(start..=end)
}

/// Parse LLM output into structured contradictions.
///
/// Handles both object-format `{"chunk_a": N, "chunk_b": M, "description": "..."}` and
/// plain string entries in the `contradictions` array.
fn parse_contradictions(text: &str) -> Vec<Contradiction> {
    let Some(json_text) = extract_json_substring(text) else {
        return vec![];
    };

    let parsed: serde_json::Value = match serde_json::from_str(json_text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let Some(arr) = parsed
        .get("contradictions")
        .and_then(serde_json::Value::as_array)
    else {
        return vec![];
    };

    arr.iter()
        .filter_map(|item| {
            if let Some(s) = item.as_str() {
                if s.is_empty() {
                    return None;
                }
                Some(Contradiction {
                    chunk_a: 0,
                    chunk_b: 0,
                    description: s.to_owned(),
                })
            } else if let Some(obj) = item.as_object() {
                let chunk_a = obj
                    .get("chunk_a")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(0);
                let chunk_b = obj
                    .get("chunk_b")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(0);
                let description = obj
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                if description.is_empty() {
                    return None;
                }
                // WHY: LLM returns 1-based indices, convert to 0-based.
                Some(Contradiction {
                    chunk_a: chunk_a.saturating_sub(1),
                    chunk_b: chunk_b.saturating_sub(1),
                    description,
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_contradictions_object_format() {
        let text =
            r#"{"contradictions": [{"chunk_a": 1, "chunk_b": 3, "description": "port conflict"}]}"#;
        let result = parse_contradictions(text);
        assert_eq!(result.len(), 1, "should parse one contradiction");
        let c = result.first().expect("already checked len");
        assert_eq!(c.chunk_a, 0, "1-based index 1 should become 0-based 0");
        assert_eq!(c.chunk_b, 2, "1-based index 3 should become 0-based 2");
        assert_eq!(c.description, "port conflict");
    }

    #[test]
    fn parse_contradictions_string_format() {
        let text = r#"{"contradictions": ["fact 1 conflicts with fact 2"]}"#;
        let result = parse_contradictions(text);
        assert_eq!(result.len(), 1, "should parse one string contradiction");
        assert_eq!(
            result.first().expect("already checked len").description,
            "fact 1 conflicts with fact 2"
        );
    }

    #[test]
    fn parse_contradictions_empty_array() {
        let text = r#"{"contradictions": []}"#;
        let result = parse_contradictions(text);
        assert!(result.is_empty(), "empty array should yield empty vec");
    }

    #[test]
    fn parse_contradictions_malformed_json() {
        let text = "this is not json at all";
        let result = parse_contradictions(text);
        assert!(result.is_empty(), "malformed input should yield empty vec");
    }

    #[test]
    fn parse_contradictions_missing_field() {
        let text = r#"{"other_field": "value"}"#;
        let result = parse_contradictions(text);
        assert!(
            result.is_empty(),
            "missing contradictions field should yield empty vec"
        );
    }

    #[test]
    fn parse_contradictions_skips_empty_descriptions() {
        let text = r#"{"contradictions": [{"chunk_a": 1, "chunk_b": 2, "description": ""}, ""]}"#;
        let result = parse_contradictions(text);
        assert!(
            result.is_empty(),
            "empty descriptions should be filtered out"
        );
    }

    #[test]
    fn extract_json_from_markdown_fenced_block() {
        let text = "Some preamble\n```json\n{\"contradictions\": []}\n```\nTrailing text";
        let result = extract_json_substring(text);
        assert!(result.is_some(), "should find JSON in fenced block");
        assert!(
            result
                .expect("already checked is_some")
                .contains("contradictions"),
            "extracted JSON should contain the contradictions key"
        );
    }

    #[test]
    fn extract_json_from_raw_text() {
        let text =
            r#"{"contradictions": [{"chunk_a": 1, "chunk_b": 2, "description": "conflict"}]}"#;
        let result = extract_json_substring(text);
        assert_eq!(result, Some(text), "raw JSON should be extracted as-is");
    }

    #[test]
    fn extract_json_no_braces() {
        let text = "no json here";
        assert!(
            extract_json_substring(text).is_none(),
            "text without braces should return None"
        );
    }

    #[test]
    fn contradiction_log_empty_is_empty() {
        assert!(
            ContradictionLog::empty().is_empty(),
            "empty log should report is_empty"
        );
    }

    #[test]
    fn contradiction_log_with_entries_not_empty() {
        let log = ContradictionLog {
            contradictions: vec![Contradiction {
                chunk_a: 0,
                chunk_b: 1,
                description: "test conflict".to_owned(),
            }],
            timestamp: "2026-03-22T00:00:00Z".to_owned(),
            resolution_strategy: ResolutionStrategy::NeedsUserReview,
        };
        assert!(!log.is_empty(), "log with entries should not be empty");
    }

    #[tokio::test]
    async fn detect_with_fewer_than_two_chunks_returns_empty() {
        let provider = aletheia_hermeneus::test_utils::MockProvider::new("unused");
        let result = detect_contradictions(&["single".to_owned()], &provider, "model")
            .await
            .expect("should succeed with single chunk");
        assert!(
            result.is_empty(),
            "fewer than 2 chunks should skip detection"
        );
    }

    #[tokio::test]
    async fn detect_parses_llm_response() {
        let llm_output = r#"{"contradictions": [{"chunk_a": 1, "chunk_b": 2, "description": "port conflict: 8080 vs 3000"}]}"#;
        let provider = aletheia_hermeneus::test_utils::MockProvider::new(llm_output)
            .models(&["test-model"])
            .named("contradiction-test");
        let chunks = vec![
            "Server runs on port 8080".to_owned(),
            "Server runs on port 3000".to_owned(),
        ];
        let result = detect_contradictions(&chunks, &provider, "test-model")
            .await
            .expect("should succeed with valid provider");
        assert_eq!(
            result.contradictions.len(),
            1,
            "should detect one contradiction"
        );
        assert!(
            !result.timestamp.is_empty(),
            "timestamp should be populated"
        );
        assert!(
            matches!(
                result.resolution_strategy,
                ResolutionStrategy::NeedsUserReview
            ),
            "contradictions present should suggest user review"
        );
    }

    #[tokio::test]
    async fn detect_no_contradictions_returns_prefer_newer() {
        let llm_output = r#"{"contradictions": []}"#;
        let provider = aletheia_hermeneus::test_utils::MockProvider::new(llm_output)
            .models(&["test-model"])
            .named("contradiction-test");
        let chunks = vec![
            "User likes coffee".to_owned(),
            "Server runs on port 8080".to_owned(),
        ];
        let result = detect_contradictions(&chunks, &provider, "test-model")
            .await
            .expect("should succeed");
        assert!(result.is_empty(), "no contradictions should be detected");
        assert!(
            matches!(result.resolution_strategy, ResolutionStrategy::PreferNewer),
            "empty result should default to PreferNewer"
        );
    }
}
