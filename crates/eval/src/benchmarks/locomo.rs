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

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use serde::Deserialize;

use super::validation::{
    BenchmarkValidationOptions, BenchmarkValidationReport, clean_refs, deserialize_string_list,
};
use super::{BenchmarkQuestion, MemoryBenchmark};

const DATASET_NAME: &str = "LoCoMo";
const VALID_CATEGORIES: &[&str] = &[
    "single_hop",
    "multi_hop",
    "temporal",
    "open_domain",
    "adversarial",
];

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
    #[serde(default, deserialize_with = "deserialize_string_list")]
    evidence: Vec<String>,
}

impl LocomoDataset {
    /// Load a `LoCoMo` dataset from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub async fn from_path(path: impl AsRef<Path> + Send) -> io::Result<Self> {
        let path_ref = path.as_ref();
        let options = BenchmarkValidationOptions::strict_for_path(path_ref.display().to_string());
        let (dataset, _) = Self::from_path_with_options(path_ref, options).await?;
        Ok(dataset)
    }

    /// Load and validate a `LoCoMo` dataset from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, parsed, or validated.
    pub async fn from_path_with_options(
        path: impl AsRef<Path> + Send,
        mut options: BenchmarkValidationOptions,
    ) -> io::Result<(Self, BenchmarkValidationReport)> {
        let path_ref = path.as_ref();
        if options.dataset_path.is_none() {
            options.dataset_path = Some(path_ref.display().to_string());
        }
        let bytes = tokio::fs::read(path_ref).await?;
        Self::from_bytes_with_options(&bytes, &options)
    }

    /// Parse a `LoCoMo` dataset from a JSON byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is not in the expected `LoCoMo` format.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let options = BenchmarkValidationOptions::strict();
        let (dataset, _) = Self::from_bytes_with_options(bytes, &options)?;
        Ok(dataset)
    }

    /// Parse and validate a `LoCoMo` dataset from a JSON byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid or validation fails.
    pub fn from_bytes_with_options(
        bytes: &[u8],
        options: &BenchmarkValidationOptions,
    ) -> io::Result<(Self, BenchmarkValidationReport)> {
        let conversations: Vec<LocomoConversation> = serde_json::from_slice(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let report = validate_conversations(&conversations, options).into_result()?;
        Ok((Self { conversations }, report))
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
                let mut expected_answers = Vec::new();
                if !qa.answer.trim().is_empty() {
                    expected_answers.push(qa.answer.clone());
                }
                expected_answers.extend(
                    qa.alternatives
                        .iter()
                        .filter(|answer| !answer.trim().is_empty())
                        .cloned(),
                );

                BenchmarkQuestion {
                    id: format!("{}:qa_{i}", conv.sample_id),
                    sessions: sessions.clone(),
                    question: qa.question.clone(),
                    expected_answers,
                    expected_evidence_refs: clean_refs(&qa.evidence),
                    category: qa.category.clone(),
                }
            })
        }))
    }

    fn len(&self) -> usize {
        self.question_count()
    }
}

fn validate_conversations(
    conversations: &[LocomoConversation],
    options: &BenchmarkValidationOptions,
) -> BenchmarkValidationReport {
    let mut report = BenchmarkValidationReport::new(DATASET_NAME, options);
    if conversations.is_empty()
        || conversations
            .iter()
            .all(|conversation| conversation.qa.is_empty())
    {
        report.error(
            None,
            None,
            "dataset",
            "dataset must contain at least one question",
        );
        return report;
    }

    let mut seen_question_ids = BTreeSet::new();
    for conversation in conversations {
        let record_id = optional_string(&conversation.sample_id);
        validate_conversation_shape(&mut report, conversation, options, record_id.as_deref());
        for (question_index, qa) in conversation.qa.iter().enumerate() {
            let question_id = record_id
                .as_ref()
                .map(|id| format!("{id}:qa_{question_index}"));
            validate_duplicate_question_id(
                &mut report,
                &mut seen_question_ids,
                question_id.clone(),
            );
            validate_qa_shape(&mut report, qa, options, record_id.clone(), question_id);
        }
    }
    report
}

fn validate_conversation_shape(
    report: &mut BenchmarkValidationReport,
    conversation: &LocomoConversation,
    options: &BenchmarkValidationOptions,
    record_id: Option<&str>,
) {
    if conversation.sample_id.trim().is_empty() {
        report.issue(
            options,
            None,
            None,
            "sample_id",
            "conversation sample_id must not be empty",
        );
    }
    if conversation.qa.is_empty() {
        report.issue(
            options,
            record_id.map(ToOwned::to_owned),
            None,
            "qa",
            "conversation must contain at least one QA pair",
        );
    }
    validate_sessions(report, conversation, options, record_id);
}

fn validate_duplicate_question_id(
    report: &mut BenchmarkValidationReport,
    seen_question_ids: &mut BTreeSet<String>,
    question_id: Option<String>,
) {
    let Some(id) = question_id else {
        return;
    };
    if !seen_question_ids.insert(id.clone()) {
        report.error(
            Some(id.clone()),
            Some(id),
            "question_id",
            "duplicate question id",
        );
    }
}

fn validate_qa_shape(
    report: &mut BenchmarkValidationReport,
    qa: &LocomoQa,
    options: &BenchmarkValidationOptions,
    record_id: Option<String>,
    question_id: Option<String>,
) {
    if qa.question.trim().is_empty() {
        report.issue(
            options,
            record_id.clone(),
            question_id.clone(),
            "question",
            "question must not be empty",
        );
    }
    let has_expected_answer = !qa.answer.trim().is_empty()
        || qa
            .alternatives
            .iter()
            .any(|answer| !answer.trim().is_empty());
    if !has_expected_answer {
        report.issue(
            options,
            record_id.clone(),
            question_id.clone(),
            "answer",
            "expected answers must not be empty",
        );
    }
    if qa.category.trim().is_empty() || !VALID_CATEGORIES.contains(&qa.category.as_str()) {
        report.issue(
            options,
            record_id.clone(),
            question_id.clone(),
            "category",
            format!("category must be one of {}", VALID_CATEGORIES.join(", ")),
        );
    }
    if options.require_retrieval_evidence && clean_refs(&qa.evidence).is_empty() {
        report.issue(
            options,
            record_id,
            question_id,
            "evidence",
            "retrieval metrics require expected evidence or fact references",
        );
    }
}

fn validate_sessions(
    report: &mut BenchmarkValidationReport,
    conversation: &LocomoConversation,
    options: &BenchmarkValidationOptions,
    record_id: Option<&str>,
) {
    if conversation.conversation.is_empty() {
        report.issue(
            options,
            record_id.map(ToOwned::to_owned),
            None,
            "conversation",
            "at least one non-empty session is required",
        );
        return;
    }
    for (session_id, turns) in &conversation.conversation {
        if session_id.trim().is_empty() {
            report.issue(
                options,
                record_id.map(ToOwned::to_owned),
                None,
                "conversation.session_id",
                "session id must not be empty",
            );
        }
        if turns.is_empty() {
            report.issue(
                options,
                record_id.map(ToOwned::to_owned),
                None,
                format!("conversation.{session_id}"),
                "session must not be empty",
            );
        }
        for (turn_index, turn) in turns.iter().enumerate() {
            if turn.speaker.trim().is_empty() {
                report.issue(
                    options,
                    record_id.map(ToOwned::to_owned),
                    None,
                    format!("conversation.{session_id}[{turn_index}].speaker"),
                    "turn speaker must not be empty",
                );
            }
            if turn.text.trim().is_empty() {
                report.issue(
                    options,
                    record_id.map(ToOwned::to_owned),
                    None,
                    format!("conversation.{session_id}[{turn_index}].text"),
                    "turn text must not be empty",
                );
            }
        }
    }
}

fn optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
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
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        assert_eq!(dataset.conversations.len(), 1);
        assert_eq!(dataset.question_count(), 2);
        assert_eq!(dataset.len(), 2);
        assert!(!dataset.is_empty());
        assert_eq!(dataset.name(), "LoCoMo");
    }

    #[test]
    fn questions_include_all_sessions() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions.len(), 2);

        // Both questions share the same conversation, so both should have
        // 2 sessions.
        assert_eq!(questions[0].sessions.len(), 2);
        assert_eq!(questions[1].sessions.len(), 2);
    }

    #[test]
    fn question_ids_include_sample_and_index() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].id, "conv_1:qa_0");
        assert_eq!(questions[1].id, "conv_1:qa_1");
    }

    #[test]
    fn answer_alternatives_preserved() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(
            questions[1].expected_answers,
            vec![
                "visited the museums".to_owned(),
                "went to museums".to_owned()
            ]
        );
    }

    #[test]
    fn category_preserved() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].category, "single_hop");
        assert_eq!(questions[1].category, "multi_hop");
    }

    #[test]
    fn speaker_preserved_as_role() {
        let dataset = LocomoDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        let session_0 = &questions[0].sessions[0];
        assert!(
            session_0
                .iter()
                .any(|(role, _)| role == "Alice" || role == "Bob"),
            "speakers should be preserved as roles"
        );
    }

    #[test]
    fn empty_dataset_is_rejected() {
        let err = LocomoDataset::from_bytes(b"[]").unwrap_err();
        assert!(err.to_string().contains("at least one question"));
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = LocomoDataset::from_bytes(b"not json");
        assert!(result.is_err());
    }

    #[test]
    fn missing_category_is_rejected() {
        let json = r#"[
            {
                "sample_id": "conv_1",
                "conversation": {
                    "session_1": [
                        {"speaker": "Alice", "text": "Hi Bob"}
                    ]
                },
                "qa": [
                    {
                        "question": "Who did Alice greet?",
                        "answer": "Bob"
                    }
                ]
            }
        ]"#;
        let err = LocomoDataset::from_bytes(json.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("category"));
    }

    #[test]
    fn best_effort_allows_missing_category_with_warning() {
        let json = r#"[
            {
                "sample_id": "conv_1",
                "conversation": {
                    "session_1": [
                        {"speaker": "Alice", "text": "Hi Bob"}
                    ]
                },
                "qa": [
                    {
                        "question": "Who did Alice greet?",
                        "answer": "Bob"
                    }
                ]
            }
        ]"#;
        let (dataset, report) = LocomoDataset::from_bytes_with_options(
            json.as_bytes(),
            &BenchmarkValidationOptions {
                allow_best_effort: true,
                ..BenchmarkValidationOptions::strict()
            },
        )
        .expect("best-effort dataset");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].category, "");
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn evidence_refs_are_preserved() {
        let json = r#"[
            {
                "sample_id": "conv_1",
                "conversation": {
                    "session_1": [
                        {"speaker": "Alice", "text": "Hi Bob"}
                    ]
                },
                "qa": [
                    {
                        "question": "Who did Alice greet?",
                        "answer": "Bob",
                        "category": "single_hop",
                        "evidence": [" session_1:0 "]
                    }
                ]
            }
        ]"#;
        let dataset = LocomoDataset::from_bytes(json.as_bytes()).expect("valid JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].expected_evidence_refs, vec!["session_1:0"]);
    }

    #[test]
    fn retrieval_evidence_required_when_requested() {
        let err = LocomoDataset::from_bytes_with_options(
            SAMPLE_JSON.as_bytes(),
            &BenchmarkValidationOptions {
                require_retrieval_evidence: true,
                ..BenchmarkValidationOptions::strict()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("evidence"));
    }
}
