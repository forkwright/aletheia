//! `LongMemEval` dataset format parser.
//!
//! `LongMemEval` (arxiv 2410.10813) provides 500 human-generated questions
//! spanning five memory abilities across ~115k-token conversation histories.
//!
//! # Expected JSON format
//!
//! Each item in the top-level array has the shape:
//!
//! ```json
//! {
//!   "question_id": "abc-123",
//!   "question_type": "single-session-user",
//!   "question": "What did I say my favorite color was?",
//!   "answer": "blue",
//!   "haystack_sessions": [
//!     [
//!       {"role": "user", "content": "My favorite color is blue"},
//!       {"role": "assistant", "content": "Good to know!"}
//!     ],
//!     [ ... next session ... ]
//!   ]
//! }
//! ```
//!
//! The `question_type` maps to one of the five memory abilities:
//! `single-session-user`, `single-session-assistant`, `multi-session`,
//! `temporal-reasoning`, `knowledge-update`.

use std::io;
use std::path::Path;

use serde::Deserialize;

use super::{BenchmarkQuestion, MemoryBenchmark};

/// Parsed `LongMemEval` dataset.
#[derive(Debug, Clone)]
pub struct LongMemEvalDataset {
    items: Vec<LongMemEvalItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct LongMemEvalItem {
    question_id: String,
    #[serde(default)]
    question_type: String,
    question: String,
    answer: String,
    /// Optional alternate answers (some questions accept multiple valid forms).
    #[serde(default)]
    answer_alternatives: Vec<String>,
    #[serde(default)]
    haystack_sessions: Vec<Vec<LongMemEvalTurn>>,
}

#[derive(Debug, Clone, Deserialize)]
struct LongMemEvalTurn {
    role: String,
    content: String,
}

impl LongMemEvalDataset {
    /// Load a `LongMemEval` dataset from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub async fn from_path(path: impl AsRef<Path> + Send) -> io::Result<Self> {
        let bytes = tokio::fs::read(path).await?;
        Self::from_bytes(&bytes)
    }

    /// Parse a `LongMemEval` dataset from a JSON byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is not in the expected `LongMemEval` format.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let items: Vec<LongMemEvalItem> = serde_json::from_slice(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(Self { items })
    }
}

impl MemoryBenchmark for LongMemEvalDataset {
    fn name(&self) -> &'static str {
        "LongMemEval"
    }

    fn questions(&self) -> Box<dyn Iterator<Item = BenchmarkQuestion> + '_> {
        Box::new(self.items.iter().map(|item| {
            let sessions = item
                .haystack_sessions
                .iter()
                .map(|session| {
                    session
                        .iter()
                        .map(|turn| (turn.role.clone(), turn.content.clone()))
                        .collect()
                })
                .collect();

            let mut expected_answers = vec![item.answer.clone()];
            expected_answers.extend(item.answer_alternatives.iter().cloned());

            BenchmarkQuestion {
                id: item.question_id.clone(),
                sessions,
                question: item.question.clone(),
                expected_answers,
                category: if item.question_type.is_empty() {
                    "unknown".to_owned()
                } else {
                    item.question_type.clone()
                },
            }
        }))
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "tests assert against known-length parsed datasets"
)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"[
        {
            "question_id": "q1",
            "question_type": "single-session-user",
            "question": "What is my favorite color?",
            "answer": "blue",
            "haystack_sessions": [
                [
                    {"role": "user", "content": "My favorite color is blue"},
                    {"role": "assistant", "content": "Got it!"}
                ]
            ]
        },
        {
            "question_id": "q2",
            "question_type": "temporal-reasoning",
            "question": "When did I move to Berlin?",
            "answer": "2024",
            "answer_alternatives": ["year 2024", "2024-01"],
            "haystack_sessions": [
                [
                    {"role": "user", "content": "I moved to Berlin in 2024"}
                ],
                [
                    {"role": "user", "content": "Still enjoying Berlin"}
                ]
            ]
        }
    ]"#;

    #[test]
    fn parses_sample_dataset() {
        let dataset = LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        assert_eq!(dataset.len(), 2);
        assert!(!dataset.is_empty());
        assert_eq!(dataset.name(), "LongMemEval");
    }

    #[test]
    fn questions_preserve_metadata() {
        let dataset = LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions.len(), 2);

        let q1 = &questions[0];
        assert_eq!(q1.id, "q1");
        assert_eq!(q1.category, "single-session-user");
        assert_eq!(q1.expected_answers, vec!["blue"]);
        assert_eq!(q1.sessions.len(), 1);
        assert_eq!(q1.sessions[0].len(), 2);
        assert_eq!(q1.sessions[0][0].0, "user");
        assert_eq!(q1.sessions[0][0].1, "My favorite color is blue");
    }

    #[test]
    fn answer_alternatives_included() {
        let dataset = LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        let q2 = &questions[1];
        assert_eq!(
            q2.expected_answers,
            vec!["2024".to_owned(), "year 2024".to_owned(), "2024-01".to_owned()]
        );
    }

    #[test]
    fn multi_session_preserved() {
        let dataset = LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        let q2 = &questions[1];
        assert_eq!(q2.sessions.len(), 2, "should have 2 sessions");
    }

    #[test]
    fn empty_dataset_parses() {
        let dataset = LongMemEvalDataset::from_bytes(b"[]").expect("valid JSON");
        assert_eq!(dataset.len(), 0);
        assert!(dataset.is_empty());
        assert_eq!(dataset.questions().count(), 0);
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = LongMemEvalDataset::from_bytes(b"not json");
        assert!(result.is_err());
    }

    #[test]
    fn missing_question_type_defaults_to_unknown() {
        let json = r#"[{
            "question_id": "q1",
            "question": "test",
            "answer": "yes",
            "haystack_sessions": []
        }]"#;
        let dataset = LongMemEvalDataset::from_bytes(json.as_bytes())
            .expect("valid JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].category, "unknown");
    }
}
