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

use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use serde::Deserialize;

use super::validation::{
    BenchmarkValidationOptions, BenchmarkValidationReport, clean_refs, deserialize_string_list,
};
use super::{BenchmarkQuestion, BenchmarkTurn, MemoryBenchmark};

const DATASET_NAME: &str = "LongMemEval";
const VALID_CATEGORIES: &[&str] = &[
    "single-session-user",
    "single-session-assistant",
    "multi-session",
    "temporal-reasoning",
    "knowledge-update",
];

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
    /// Optional evidence/fact references supplied by derived benchmark files.
    #[serde(
        default,
        alias = "evidence",
        alias = "evidence_ids",
        alias = "relevant_ids",
        alias = "fact_ids",
        deserialize_with = "deserialize_string_list"
    )]
    evidence_refs: Vec<String>,
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
        let path_ref = path.as_ref();
        let options = BenchmarkValidationOptions::strict_for_path(path_ref.display().to_string());
        let (dataset, _) = Self::from_path_with_options(path_ref, options).await?;
        Ok(dataset)
    }

    /// Load and validate a `LongMemEval` dataset from a JSON file.
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

    /// Parse a `LongMemEval` dataset from a JSON byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is not in the expected `LongMemEval` format.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let options = BenchmarkValidationOptions::strict();
        let (dataset, _) = Self::from_bytes_with_options(bytes, &options)?;
        Ok(dataset)
    }

    /// Parse and validate a `LongMemEval` dataset from a JSON byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid or validation fails.
    pub fn from_bytes_with_options(
        bytes: &[u8],
        options: &BenchmarkValidationOptions,
    ) -> io::Result<(Self, BenchmarkValidationReport)> {
        let items: Vec<LongMemEvalItem> = serde_json::from_slice(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let report = validate_items(&items, options).into_result()?;
        Ok((Self { items }, report))
    }
}

impl MemoryBenchmark for LongMemEvalDataset {
    fn name(&self) -> &'static str {
        "LongMemEval"
    }

    fn questions(&self) -> Box<dyn Iterator<Item = BenchmarkQuestion> + '_> {
        Box::new(self.items.iter().map(|item| {
            let question_id = item.question_id.clone();
            let sessions = item
                .haystack_sessions
                .iter()
                .enumerate()
                .map(|(session_index, session)| {
                    session
                        .iter()
                        .enumerate()
                        .map(|(turn_index, turn)| {
                            let turn_id = format!("s{session_index}:t{turn_index}");
                            let provenance = format!(
                                "LongMemEval:{question_id}:session_{session_index}:turn_{turn_index}"
                            );
                            BenchmarkTurn {
                                role: turn.role.clone(),
                                content: turn.content.clone(),
                                speaker: None,
                                turn_id: Some(turn_id),
                                timestamp: None,
                                provenance: Some(provenance),
                            }
                        })
                        .collect()
                })
                .collect();

            let mut expected_answers = Vec::new();
            if !item.answer.trim().is_empty() {
                expected_answers.push(item.answer.clone());
            }
            expected_answers.extend(
                item.answer_alternatives
                    .iter()
                    .filter(|answer| !answer.trim().is_empty())
                    .cloned(),
            );

            BenchmarkQuestion {
                id: item.question_id.clone(),
                sessions,
                question: item.question.clone(),
                expected_answers,
                expected_evidence_refs: clean_refs(&item.evidence_refs),
                category: item.question_type.clone(),
            }
        }))
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

fn validate_items(
    items: &[LongMemEvalItem],
    options: &BenchmarkValidationOptions,
) -> BenchmarkValidationReport {
    let mut report = BenchmarkValidationReport::new(DATASET_NAME, options);
    if items.is_empty() {
        report.error(
            None,
            None,
            "dataset",
            "dataset must contain at least one question",
        );
        return report;
    }

    let mut seen_ids = BTreeSet::new();
    for item in items {
        let question_id = optional_string(&item.question_id);
        validate_duplicate_id(&mut report, &mut seen_ids, question_id.clone());
        validate_item_shape(&mut report, item, options, question_id);
    }
    report
}

fn validate_duplicate_id(
    report: &mut BenchmarkValidationReport,
    seen_ids: &mut BTreeSet<String>,
    question_id: Option<String>,
) {
    let Some(id) = question_id else {
        return;
    };
    if !seen_ids.insert(id.clone()) {
        report.error(
            Some(id.clone()),
            Some(id),
            "question_id",
            "duplicate question id",
        );
    }
}

fn validate_item_shape(
    report: &mut BenchmarkValidationReport,
    item: &LongMemEvalItem,
    options: &BenchmarkValidationOptions,
    question_id: Option<String>,
) {
    if item.question_id.trim().is_empty() {
        report.issue(
            options,
            None,
            None,
            "question_id",
            "question id must not be empty",
        );
    }
    if item.question.trim().is_empty() {
        report.issue(
            options,
            question_id.clone(),
            question_id.clone(),
            "question",
            "question must not be empty",
        );
    }
    let has_expected_answer = !item.answer.trim().is_empty()
        || item
            .answer_alternatives
            .iter()
            .any(|answer| !answer.trim().is_empty());
    if !has_expected_answer {
        report.issue(
            options,
            question_id.clone(),
            question_id.clone(),
            "answer",
            "expected answers must not be empty",
        );
    }
    if item.question_type.trim().is_empty()
        || !VALID_CATEGORIES.contains(&item.question_type.as_str())
    {
        report.issue(
            options,
            question_id.clone(),
            question_id.clone(),
            "question_type",
            format!("category must be one of {}", VALID_CATEGORIES.join(", ")),
        );
    }
    validate_sessions(report, item, options, question_id.clone());
    if options.require_retrieval_evidence && clean_refs(&item.evidence_refs).is_empty() {
        report.issue(
            options,
            question_id.clone(),
            question_id,
            "evidence_refs",
            "retrieval metrics require expected evidence or fact references",
        );
    }
}

fn validate_sessions(
    report: &mut BenchmarkValidationReport,
    item: &LongMemEvalItem,
    options: &BenchmarkValidationOptions,
    question_id: Option<String>,
) {
    if item.haystack_sessions.is_empty() {
        report.issue(
            options,
            question_id.clone(),
            question_id,
            "haystack_sessions",
            "at least one non-empty session is required",
        );
        return;
    }
    for (session_index, session) in item.haystack_sessions.iter().enumerate() {
        if session.is_empty() {
            report.issue(
                options,
                question_id.clone(),
                question_id.clone(),
                format!("haystack_sessions[{session_index}]"),
                "session must not be empty",
            );
        }
        for (turn_index, turn) in session.iter().enumerate() {
            if turn.role.trim().is_empty() {
                report.issue(
                    options,
                    question_id.clone(),
                    question_id.clone(),
                    format!("haystack_sessions[{session_index}][{turn_index}].role"),
                    "turn role must not be empty",
                );
            }
            if turn.content.trim().is_empty() {
                report.issue(
                    options,
                    question_id.clone(),
                    question_id.clone(),
                    format!("haystack_sessions[{session_index}][{turn_index}].content"),
                    "turn content must not be empty",
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
        let dataset =
            LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        assert_eq!(dataset.len(), 2);
        assert!(!dataset.is_empty());
        assert_eq!(dataset.name(), "LongMemEval");
    }

    #[test]
    fn questions_preserve_metadata() {
        let dataset =
            LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions.len(), 2);

        let q1 = &questions[0];
        assert_eq!(q1.id, "q1");
        assert_eq!(q1.category, "single-session-user");
        assert_eq!(q1.expected_answers, vec!["blue"]);
        assert_eq!(q1.sessions.len(), 1);
        assert_eq!(q1.sessions[0].len(), 2);
        assert_eq!(q1.sessions[0][0].role, "user");
        assert_eq!(q1.sessions[0][0].content, "My favorite color is blue");
        assert_eq!(q1.sessions[0][0].speaker, None);
        assert_eq!(
            q1.sessions[0][0].provenance,
            Some("LongMemEval:q1:session_0:turn_0".to_owned())
        );
    }

    #[test]
    fn answer_alternatives_included() {
        let dataset =
            LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        let q2 = &questions[1];
        assert_eq!(
            q2.expected_answers,
            vec![
                "2024".to_owned(),
                "year 2024".to_owned(),
                "2024-01".to_owned()
            ]
        );
    }

    #[test]
    fn multi_session_preserved() {
        let dataset =
            LongMemEvalDataset::from_bytes(SAMPLE_JSON.as_bytes()).expect("valid sample JSON");
        let questions: Vec<_> = dataset.questions().collect();
        let q2 = &questions[1];
        assert_eq!(q2.sessions.len(), 2, "should have 2 sessions");
    }

    #[test]
    fn empty_dataset_is_rejected() {
        let err = LongMemEvalDataset::from_bytes(b"[]").unwrap_err();
        assert!(err.to_string().contains("at least one question"));
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = LongMemEvalDataset::from_bytes(b"not json");
        assert!(result.is_err());
    }

    #[test]
    fn missing_question_type_is_rejected() {
        let json = r#"[{
            "question_id": "q1",
            "question": "test",
            "answer": "yes",
            "haystack_sessions": [[{"role": "user", "content": "test"}]]
        }]"#;
        let err = LongMemEvalDataset::from_bytes(json.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("question_type"));
    }

    #[test]
    fn best_effort_allows_missing_question_type_with_warning() {
        let json = r#"[{
            "question_id": "q1",
            "question": "test",
            "answer": "yes",
            "haystack_sessions": [[{"role": "user", "content": "test"}]]
        }]"#;
        let (dataset, report) = LongMemEvalDataset::from_bytes_with_options(
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
        let json = r#"[{
            "question_id": "q1",
            "question_type": "single-session-user",
            "question": "What is my favorite color?",
            "answer": "blue",
            "evidence": [" fact-blue "],
            "haystack_sessions": [[{"role": "user", "content": "My favorite color is blue"}]]
        }]"#;
        let dataset = LongMemEvalDataset::from_bytes(json.as_bytes()).expect("valid JSON");
        let questions: Vec<_> = dataset.questions().collect();
        assert_eq!(questions[0].expected_evidence_refs, vec!["fact-blue"]);
    }

    #[test]
    fn retrieval_evidence_required_when_requested() {
        let err = LongMemEvalDataset::from_bytes_with_options(
            SAMPLE_JSON.as_bytes(),
            &BenchmarkValidationOptions {
                require_retrieval_evidence: true,
                ..BenchmarkValidationOptions::strict()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("evidence_refs"));
    }
}
