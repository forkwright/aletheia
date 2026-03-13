#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;
use crate::skills::heuristics::PatternType;
use crate::skills::signature::SequenceSignature;

// -- Mock provider --------------------------------------------------------

struct MockProvider {
    response: Result<String, SkillExtractionError>,
}

impl MockProvider {
    fn ok(response: &str) -> Self {
        Self {
            response: Ok(response.to_owned()),
        }
    }

    fn err(msg: &str) -> Self {
        Self {
            response: Err(LlmCallSnafu {
                message: msg.to_owned(),
            }
            .build()),
        }
    }
}

impl SkillExtractionProvider for MockProvider {
    fn complete<'a>(
        &'a self,
        _system: &'a str,
        _user_message: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, SkillExtractionError>> + Send + 'a>,
    > {
        Box::pin(async {
            self.response.as_ref().cloned().map_err(|_prev| {
                LlmCallSnafu {
                    message: "mock error".to_owned(),
                }
                .build()
            })
        })
    }
}

fn sample_candidate() -> SkillCandidate {
    SkillCandidate {
        id: "cand-001".to_owned(),
        nous_id: "test-nous".to_owned(),
        signature: SequenceSignature {
            normalized: vec![
                "Grep".to_owned(),
                "Read".to_owned(),
                "Edit".to_owned(),
                "Bash".to_owned(),
            ],
            hash: 12345,
        },
        recurrence_count: 3,
        session_refs: vec!["s1".to_owned(), "s2".to_owned(), "s3".to_owned()],
        first_seen: jiff::Timestamp::now(),
        last_seen: jiff::Timestamp::now(),
        heuristic_score: 0.72,
        pattern_type: Some(PatternType::Diagnostic),
    }
}

fn sample_sequences() -> Vec<Vec<ToolCallRecord>> {
    vec![
        vec![
            ToolCallRecord::new("Grep", 120),
            ToolCallRecord::new("Read", 80),
            ToolCallRecord::new("Read", 90),
            ToolCallRecord::new("Edit", 150),
            ToolCallRecord::new("Bash", 200),
            ToolCallRecord::new("Bash", 100),
        ],
        vec![
            ToolCallRecord::new("Grep", 110),
            ToolCallRecord::new("Read", 75),
            ToolCallRecord::new("Edit", 160),
            ToolCallRecord::new("Edit", 90),
            ToolCallRecord::new("Bash", 180),
            ToolCallRecord::new("Bash", 120),
        ],
    ]
}

fn valid_json_response() -> String {
    r#"{
        "name": "test-driven-bug-fix",
        "description": "Diagnose test failures, fix source code, and validate",
        "steps": [
            "Search for failing test references",
            "Read relevant source files",
            "Edit source to fix the issue",
            "Run tests to verify"
        ],
        "tools_used": ["Grep", "Read", "Edit", "Bash"],
        "domain_tags": ["testing", "debugging"],
        "when_to_use": "When a test failure needs diagnosis and fix"
    }"#
    .to_owned()
}

// -- Prompt construction --------------------------------------------------

#[test]
fn build_prompt_includes_candidate_metadata() {
    let candidate = sample_candidate();
    let seqs = sample_sequences();
    let prompt = build_extraction_prompt(&candidate, &seqs);

    assert!(prompt.contains("Recurrence count: 3"));
    assert!(prompt.contains("Diagnostic"));
    assert!(prompt.contains("0.72"));
    assert!(prompt.contains("Grep → Read → Edit → Bash"));
}

#[test]
fn build_prompt_includes_all_sessions() {
    let candidate = sample_candidate();
    let seqs = sample_sequences();
    let prompt = build_extraction_prompt(&candidate, &seqs);

    assert!(prompt.contains("Session 1"));
    assert!(prompt.contains("Session 2"));
    assert!(prompt.contains("6 calls"));
}

#[test]
fn build_prompt_includes_tool_call_details() {
    let candidate = sample_candidate();
    let seqs = vec![vec![
        ToolCallRecord::new("Read", 50),
        ToolCallRecord::errored("Bash", 300),
    ]];
    let prompt = build_extraction_prompt(&candidate, &seqs);

    assert!(prompt.contains("Read (50ms)"));
    assert!(prompt.contains("Bash (300ms) [ERROR]"));
}

#[test]
fn build_prompt_handles_empty_sequences() {
    let candidate = sample_candidate();
    let prompt = build_extraction_prompt(&candidate, &[]);

    assert!(prompt.contains("Candidate Pattern"));
    assert!(prompt.contains("Observed Tool Call Sequences"));
}

#[test]
fn build_prompt_no_pattern_type() {
    let mut candidate = sample_candidate();
    candidate.pattern_type = None;
    let seqs = sample_sequences();
    let prompt = build_extraction_prompt(&candidate, &seqs);

    assert!(!prompt.contains("pattern type"));
}

// -- Response parsing -----------------------------------------------------

#[test]
fn parse_valid_json_response() {
    let response = valid_json_response();
    let skill = parse_skill_response(&response).expect("valid JSON response should parse");

    assert_eq!(skill.name, "test-driven-bug-fix");
    assert_eq!(skill.steps.len(), 4);
    assert_eq!(skill.tools_used, vec!["Grep", "Read", "Edit", "Bash"]);
    assert_eq!(skill.domain_tags, vec!["testing", "debugging"]);
    assert!(!skill.when_to_use.is_empty());
}

#[test]
fn parse_json_in_markdown_fences() {
    let response = format!("```json\n{}\n```", valid_json_response());
    let skill = parse_skill_response(&response).expect("json in markdown fences should parse");
    assert_eq!(skill.name, "test-driven-bug-fix");
}

#[test]
fn parse_json_in_bare_fences() {
    let response = format!("```\n{}\n```", valid_json_response());
    let skill = parse_skill_response(&response).expect("json in bare fences should parse");
    assert_eq!(skill.name, "test-driven-bug-fix");
}

#[test]
fn parse_json_with_surrounding_text() {
    let response = format!(
        "Here is the extracted skill:\n{}\nDone.",
        valid_json_response()
    );
    let skill = parse_skill_response(&response).expect("json with surrounding text should parse");
    assert_eq!(skill.name, "test-driven-bug-fix");
}

#[test]
fn parse_malformed_json_returns_error() {
    let response = "not json at all";
    let result = parse_skill_response(response);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("invalid JSON"));
}

#[test]
fn parse_incomplete_json_returns_error() {
    let response = r#"{"name": "test", "description": "d"}"#;
    let result = parse_skill_response(response);
    // Missing required fields
    assert!(result.is_err());
}

#[test]
fn parse_empty_response_returns_error() {
    let result = parse_skill_response("");
    assert!(result.is_err());
}

#[test]
fn parse_empty_fields_succeeds() {
    let response = r#"{
        "name": "minimal-skill",
        "description": "A minimal skill",
        "steps": [],
        "tools_used": [],
        "domain_tags": [],
        "when_to_use": ""
    }"#;
    let skill = parse_skill_response(response).expect("minimal skill JSON should parse");
    assert_eq!(skill.name, "minimal-skill");
    assert!(skill.steps.is_empty());
}

// -- Extractor end-to-end -------------------------------------------------

#[tokio::test]
async fn extractor_returns_skill_on_valid_response() {
    let provider = MockProvider::ok(&valid_json_response());
    let extractor = SkillExtractor::new(provider);
    let candidate = sample_candidate();
    let seqs = sample_sequences();

    let skill = extractor
        .extract_skill(&candidate, &seqs)
        .await
        .expect("mock provider returns valid response");
    assert_eq!(skill.name, "test-driven-bug-fix");
}

#[tokio::test]
async fn extractor_returns_error_on_provider_failure() {
    let provider = MockProvider::err("API rate limited");
    let extractor = SkillExtractor::new(provider);
    let candidate = sample_candidate();
    let seqs = sample_sequences();

    let result = extractor.extract_skill(&candidate, &seqs).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn extractor_returns_error_on_malformed_response() {
    let provider = MockProvider::ok("this is not json");
    let extractor = SkillExtractor::new(provider);
    let candidate = sample_candidate();
    let seqs = sample_sequences();

    let result = extractor.extract_skill(&candidate, &seqs).await;
    assert!(result.is_err());
}

// -- ExtractedSkill → SkillContent ----------------------------------------

#[test]
fn to_skill_content_sets_origin_extracted() {
    let extracted = ExtractedSkill {
        name: "test-skill".to_owned(),
        description: "A test skill".to_owned(),
        steps: vec!["step 1".to_owned()],
        tools_used: vec!["Read".to_owned()],
        domain_tags: vec!["test".to_owned()],
        when_to_use: "When testing".to_owned(),
    };
    let content = extracted.to_skill_content();
    assert_eq!(content.origin, "extracted");
}

#[test]
fn to_skill_content_includes_when_to_use_in_description() {
    let extracted = ExtractedSkill {
        name: "test-skill".to_owned(),
        description: "A test skill".to_owned(),
        steps: vec![],
        tools_used: vec![],
        domain_tags: vec![],
        when_to_use: "When you need to test".to_owned(),
    };
    let content = extracted.to_skill_content();
    assert!(content.description.contains("When to Use"));
    assert!(content.description.contains("When you need to test"));
}

#[test]
fn to_skill_content_omits_when_to_use_if_empty() {
    let extracted = ExtractedSkill {
        name: "test-skill".to_owned(),
        description: "A test skill".to_owned(),
        steps: vec![],
        tools_used: vec![],
        domain_tags: vec![],
        when_to_use: String::new(),
    };
    let content = extracted.to_skill_content();
    assert_eq!(content.description, "A test skill");
}

// -- PendingSkill ---------------------------------------------------------

#[test]
fn pending_skill_new_sets_status() {
    let extracted = ExtractedSkill {
        name: "test".to_owned(),
        description: "d".to_owned(),
        steps: vec![],
        tools_used: vec![],
        domain_tags: vec![],
        when_to_use: String::new(),
    };
    let pending = PendingSkill::new(&extracted, "cand-001");
    assert!(pending.is_pending());
    assert!(!pending.is_approved());
    assert_eq!(pending.candidate_id, "cand-001");
    assert_eq!(pending.status, "pending_review");
}

#[test]
fn pending_skill_serialization_roundtrip() {
    let extracted = ExtractedSkill {
        name: "roundtrip-skill".to_owned(),
        description: "Tests serialization".to_owned(),
        steps: vec!["step 1".to_owned()],
        tools_used: vec!["Read".to_owned()],
        domain_tags: vec!["test".to_owned()],
        when_to_use: "For tests".to_owned(),
    };
    let pending = PendingSkill::new(&extracted, "cand-002");
    let json = pending.to_json().expect("pending skill serializes to JSON");
    let back = PendingSkill::from_json(&json).expect("pending skill deserializes from JSON");
    assert_eq!(back.skill.name, "roundtrip-skill");
    assert_eq!(back.candidate_id, "cand-002");
    assert!(back.is_pending());
}

#[test]
fn pending_skill_approved_status() {
    let mut pending = PendingSkill {
        skill: SkillContent {
            name: "s".to_owned(),
            description: "d".to_owned(),
            steps: vec![],
            tools_used: vec![],
            domain_tags: vec![],
            origin: "extracted".to_owned(),
        },
        candidate_id: "c".to_owned(),
        status: "approved".to_owned(),
        extracted_at: jiff::Timestamp::now(),
    };
    assert!(pending.is_approved());
    assert!(!pending.is_pending());

    pending.status = "rejected".to_owned();
    assert!(!pending.is_approved());
    assert!(!pending.is_pending());
}

// -- System prompt --------------------------------------------------------

#[test]
fn system_prompt_requests_json() {
    assert!(EXTRACTION_SYSTEM_PROMPT.contains("JSON"));
    assert!(EXTRACTION_SYSTEM_PROMPT.contains("name"));
    assert!(EXTRACTION_SYSTEM_PROMPT.contains("steps"));
    assert!(EXTRACTION_SYSTEM_PROMPT.contains("tools_used"));
    assert!(EXTRACTION_SYSTEM_PROMPT.contains("domain_tags"));
}

// -- Dedup ----------------------------------------------------------------

fn make_skill(name: &str, tools: &[&str]) -> SkillContent {
    SkillContent {
        name: name.to_owned(),
        description: format!("Skill for {name}"),
        steps: vec!["step 1".to_owned()],
        tools_used: tools.iter().map(|t| (*t).to_owned()).collect(),
        domain_tags: vec!["test".to_owned()],
        origin: "extracted".to_owned(),
    }
}

#[test]
fn dedup_unique_skills() {
    let candidate = make_skill("rust-testing", &["Bash", "Read"]);
    let existing = make_skill("python-deploy", &["Write", "Bash"]);
    let result = check_dedup(&DedupInput {
        candidate: &candidate,
        candidate_confidence: 0.8,
        candidate_usage: 0,
        existing: &existing,
        existing_confidence: 0.9,
        existing_usage: 5,
        existing_id: "sk-1",
        candidate_embedding: None,
        existing_embedding: None,
    });
    assert_eq!(result, DedupOutcome::Unique);
}

#[test]
fn dedup_similar_tools_discard_candidate() {
    let candidate = make_skill("rust-build", &["Read", "Edit", "Bash"]);
    let existing = make_skill("rust-build-v2", &["Read", "Edit", "Bash"]);
    let result = check_dedup(&DedupInput {
        candidate: &candidate,
        candidate_confidence: 0.7,
        candidate_usage: 0,
        existing: &existing,
        existing_confidence: 0.9,
        existing_usage: 10,
        existing_id: "sk-1",
        candidate_embedding: None,
        existing_embedding: None,
    });
    assert_eq!(
        result,
        DedupOutcome::DiscardCandidate {
            existing_id: "sk-1".to_owned()
        }
    );
}

#[test]
fn dedup_candidate_better_supersedes() {
    let candidate = make_skill("rust-build", &["Read", "Edit", "Bash"]);
    let existing = make_skill("rust-build-old", &["Read", "Edit", "Bash"]);
    let result = check_dedup(&DedupInput {
        candidate: &candidate,
        candidate_confidence: 0.95,
        candidate_usage: 5,
        existing: &existing,
        existing_confidence: 0.5,
        existing_usage: 0,
        existing_id: "sk-1",
        candidate_embedding: None,
        existing_embedding: None,
    });
    assert_eq!(
        result,
        DedupOutcome::SupersedeExisting {
            existing_id: "sk-1".to_owned()
        }
    );
}

#[test]
fn dedup_embedding_similarity_above_threshold() {
    let candidate = make_skill("skill-a", &["Read"]);
    let existing = make_skill("skill-b", &["Write"]);
    let cand_emb = [0.9, 0.1, 0.0, 0.0];
    let exist_emb = [0.91, 0.09, 0.01, 0.0];
    let result = check_dedup(&DedupInput {
        candidate: &candidate,
        candidate_confidence: 0.7,
        candidate_usage: 0,
        existing: &existing,
        existing_confidence: 0.9,
        existing_usage: 5,
        existing_id: "sk-1",
        candidate_embedding: Some(&cand_emb),
        existing_embedding: Some(&exist_emb),
    });
    assert_eq!(
        result,
        DedupOutcome::DiscardCandidate {
            existing_id: "sk-1".to_owned()
        }
    );
}

#[test]
fn dedup_embedding_below_threshold_is_unique() {
    let candidate = make_skill("skill-a", &["Read"]);
    let existing = make_skill("skill-b", &["Write"]);
    let cand_emb = [1.0, 0.0, 0.0, 0.0];
    let exist_emb = [0.0, 1.0, 0.0, 0.0];
    let result = check_dedup(&DedupInput {
        candidate: &candidate,
        candidate_confidence: 0.7,
        candidate_usage: 0,
        existing: &existing,
        existing_confidence: 0.9,
        existing_usage: 5,
        existing_id: "sk-1",
        candidate_embedding: Some(&cand_emb),
        existing_embedding: Some(&exist_emb),
    });
    assert_eq!(result, DedupOutcome::Unique);
}

#[test]
fn cosine_similarity_identical_vectors() {
    let a = [1.0, 0.0, 0.0];
    let b = [1.0, 0.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!(
        (sim - 1.0).abs() < 1e-6,
        "identical vectors should have sim=1.0, got {sim}"
    );
}

#[test]
fn cosine_similarity_orthogonal_vectors() {
    let a = [1.0, 0.0, 0.0];
    let b = [0.0, 1.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!(
        sim.abs() < 1e-6,
        "orthogonal vectors should have sim=0, got {sim}"
    );
}

#[test]
fn cosine_similarity_empty_vectors() {
    let sim = cosine_similarity(&[], &[]);
    assert!(sim.abs() < f64::EPSILON);
}

#[test]
fn tool_overlap_identical_sets() {
    let a = vec!["Read".to_owned(), "Edit".to_owned()];
    let b = vec!["Read".to_owned(), "Edit".to_owned()];
    let overlap = compute_tool_overlap(&a, &b);
    assert!((overlap - 1.0).abs() < f64::EPSILON);
}

#[test]
fn tool_overlap_disjoint_sets() {
    let a = vec!["Read".to_owned()];
    let b = vec!["Write".to_owned()];
    let overlap = compute_tool_overlap(&a, &b);
    assert!(overlap.abs() < f64::EPSILON);
}

#[test]
fn name_similarity_identical() {
    let sim = compute_name_similarity("rust-errors", "rust-errors");
    assert!((sim - 1.0).abs() < f64::EPSILON);
}

#[test]
fn name_similarity_partial() {
    let sim = compute_name_similarity("rust-error-handling", "rust-errors");
    assert!(
        sim > 0.5,
        "partially overlapping names should have >0.5 similarity: {sim}"
    );
}

#[test]
fn name_similarity_totally_different() {
    let sim = compute_name_similarity("abc", "xyz");
    assert!(
        sim < 0.1,
        "totally different names should have low similarity: {sim}"
    );
}
