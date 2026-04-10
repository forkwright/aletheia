//! `LoCoMo` dataset format parser.
//!
//! `LoCoMo` (Long Conversational Memory, arxiv 2402.17753) provides 50 long
//! conversations (~27 sessions, ~588 turns each) with ~200 QA pairs per
//! conversation covering single-hop, multi-hop, temporal, open-domain, and
//! adversarial question categories.
//!
//! # Expected JSON format
//!
//! The top-level is an array of conversations:
//!
//! ```json
//! [
//!   {
//!     "sample_id": "conv_1",
//!     "conversation": {
//!       "session_1": [
//!         {"speaker": "Alice", "text": "Hi Bob"},
//!         {"speaker": "Bob", "text": "Hi Alice"}
//!       ],
//!       "session_2": [ ... ]
//!     },
//!     "qa": [
//!       {
//!         "question": "Who did Alice greet?",
//!         "answer": "Bob",
//!         "category": "single-hop",
//!         "evidence": ["session_1:0"]
//!       }
//!     ]
//!   }
//! ]
//! ```
//!
//! Category values in the real dataset include: `single_hop`, `multi_hop`,
//! `temporal`, `open_domain`, `adversarial`.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::Deserialize;

use super::{BenchmarkQuestion, MemoryBenchmark};

/// Parsed `LoCoMo` dataset.
#[derive(Debug, Clone)]
pub struct LocomoDataset {
    conversations: Vec<LocomoConversation>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocomoConversation {
    sample_id: String,
    #[serde(default)]
    conversation: BTreeMap<String, Vec<LocomoTurn>>,
    #[serde(default)]
    qa: Vec<LocomoQa>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocomoTurn {
    speaker: String,
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LocomoQa {
    question: String,
    answer: String,
    #[serde(default)]
    category: String,
    #[serde(default, rename = "answer_alternatives")]
    alternatives: Vec<String>,
}

impl LocomoDataset {
    /// Load a `LoCoMo` dataset from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub async fn from_path(path: impl AsRef<Path> + Send) -> io::Result<Self> {
        let bytes = tokio::fs::read(path).await?;
        Self::from_bytes(&bytes)
    }

    /// Parse a `LoCoMo` dataset from a JSON byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is not in the expected `LoCoMo` format.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let conversations: Vec<LocomoConversation> = serde_json::from_slice(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(Self { conversations })
    }

    /// Total number of QA pairs across all conversations.
    #[must_use]
    pub fn question_count(&self) -> usize {
        self.conversations.iter().map(|c| c.qa.len()).sum()
    }
}

impl MemoryBenchmark for LocomoDataset {
    fn name(&self) -> &'static str {
        "LoCoMo"
    }

    fn questions(&self) -> Box<dyn Iterator<Item = BenchmarkQuestion> + '_> {
        Box::new(self.conversations.iter().flat_map(|conv| {
            // Build sessions from the BTreeMap (sorted by session key for
            // deterministic ordering). Each session is a list of (speaker, text)
            // turns mapped into (role, content) the harness expects.
            let sessions: Vec<Vec<(String, String)>> = conv
                .conversation
                .values()
                .map(|turns| {
                    turns
                        .iter()
                        .map(|t| (t.speaker.clone(), t.text.clone()))
                        .collect()
                })
                .collect();

            conv.qa.iter().enumerate().map(move |(i, qa)| {
                let mut expected_answers = vec![qa.answer.clone()];
                expected_answers.extend(qa.alternatives.iter().cloned());

                BenchmarkQuestion {
                    id: format!("{}:qa_{i}", conv.sample_id),
                    sessions: sessions.clone(),
                    question: qa.question.clone(),
                    expected_answers,
                    category: if qa.category.is_empty() {
                        "unknown".to_owned()
                    } else {
                        qa.category.clone()
                    },
                }
            })
        }))
    }

    fn len(&self) -> usize {
        self.question_count()
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
            "sample_id": "conv_1",
            "conversation": {
                "session_1": [
                    {"speaker": "Alice", "text": "Hi Bob, how are you?"},
                    {"speaker": "Bob", "text": "Great, just got back from Berlin"}
                ],
                "session_2": [
                    {"speaker": "Alice", "text": "How was Berlin?"},
                    {"speaker": "Bob", "text": "Amazing, visited the museums"}
                ]
            },
            "qa": [
                {
                    "question": "Where did Bob travel to?",
                    "answer": "Berlin",
                    "category": "single_hop"
                },
                {
                    "question": "What did Bob do in Berlin?",
                    "answer": "visited the museums",
                    "category": "multi_hop",
                    "answer_alternatives": ["went to museums"]
                }
            ]
        }
    ]"#;

    #[test]
    fn parses_sample_dataset() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        assert_eq!(dataset.conversations.len(), 1);
        assert_eq!(dataset.question_count(), 2);
        assert_eq!(dataset.len(), 2);
        assert!(!dataset.is_empty());
        assert_eq!(dataset.name(), "LoCoMo");
    }

    #[test]
    fn questions_include_all_sessions() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions.len(), 2);

        // Both questions share the same conversation, so both should have
        // 2 sessions.
        assert_eq!(questions[0].sessions.len(), 2);
        assert_eq!(questions[1].sessions.len(), 2);
    }

    #[test]
    fn question_ids_include_sample_and_index() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].id, "conv_1:qa_0");
        assert_eq!(questions[1].id, "conv_1:qa_1");
    }

    #[test]
    fn answer_alternatives_preserved() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(
            questions[1].expected_answers,
            vec!["visited the museums".to_owned(), "went to museums".to_owned()]
        );
    }

    #[test]
    fn category_preserved() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].category, "single_hop");
        assert_eq!(questions[1].category, "multi_hop");
    }

    #[test]
    fn speaker_preserved_as_role() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes())
            .expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        let session_0 = &questions[0].sessions[0];
        assert!(
            session_0.iter().any(|(role, _)| role == "Alice" || role == "Bob"),
            "speakers should be preserved as roles"
        );
    }

    #[test]
    fn empty_dataset_parses() {
        let dataset = LocomoDataset::from_bytes(b"[]").expect("valid JSON");
        assert_eq!(dataset.len(), 0);
        assert!(dataset.is_empty());
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = LocomoDataset::from_bytes(b"not json");
        assert!(result.is_err());
    }
}
