//! Benchmark question contract shared by dataset loaders and the live runner.

/// A single question/answer pair backed by prior conversation context.
#[derive(Debug, Clone)]
pub struct BenchmarkQuestion {
    // kanon:ignore RUST/primitive-for-domain-id — benchmark question id from external dataset JSON, not a domain newtype
    /// Unique identifier for this question within the benchmark.
    pub(super) id: String,
    /// The conversations (sessions) to ingest before asking this question.
    ///
    /// Each session is a list of turns; each turn is (role, content).
    pub(super) sessions: Vec<Vec<(String, String)>>,
    /// The question text to ask after ingestion.
    pub(super) question: String,
    /// The ground-truth answer(s). Multiple acceptable answers may be listed.
    pub(super) expected_answers: Vec<String>,
    /// Expected evidence or fact references supplied by the source dataset.
    pub(super) expected_evidence_refs: Vec<String>,
    /// Category label for per-ability scoring (e.g. "temporal", "multi-session").
    pub(super) category: String,
}

impl BenchmarkQuestion {
    /// Build a benchmark question from dataset-provided fields.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        sessions: Vec<Vec<(String, String)>>,
        question: impl Into<String>,
        expected_answers: Vec<String>,
        expected_evidence_refs: Vec<String>,
        category: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            sessions,
            question: question.into(),
            expected_answers,
            expected_evidence_refs,
            category: category.into(),
        }
    }

    /// Unique identifier for this question within the benchmark.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Sessions to ingest before asking this question.
    #[must_use]
    pub fn sessions(&self) -> &[Vec<(String, String)>] {
        &self.sessions
    }

    /// The question text to ask after ingestion.
    #[must_use]
    pub fn question(&self) -> &str {
        &self.question
    }

    /// Ground-truth answers accepted by the dataset.
    #[must_use]
    pub fn expected_answers(&self) -> &[String] {
        &self.expected_answers
    }

    /// Evidence references supplied by the source dataset.
    #[must_use]
    pub fn expected_evidence_refs(&self) -> &[String] {
        &self.expected_evidence_refs
    }

    /// Category label for per-ability scoring.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }
}

/// A memory benchmark dataset: a collection of questions.
pub trait MemoryBenchmark {
    /// Human-readable benchmark name (e.g. "`LongMemEval`", "`LoCoMo`").
    fn name(&self) -> &'static str;

    /// Iterator over all questions in the dataset.
    fn questions(&self) -> Box<dyn Iterator<Item = BenchmarkQuestion> + '_>;

    /// Total question count (for progress reporting).
    fn len(&self) -> usize;

    /// Whether the dataset has no questions.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
