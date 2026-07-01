#![expect(clippy::expect_used, reason = "test assertions")]

use mneme::meta::Stamped;

use super::*;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_state(nous_id: &str) -> ProsocheState {
    ProsocheState {
        nous_id: nous_id.to_owned(),
        checked_at: "2026-04-22T12:00:00Z".to_owned(),
        ..Default::default()
    }
}

fn fact(id: &str, content: &str, days: f64) -> FactSnapshot {
    FactSnapshot {
        fact_id: id.to_owned(),
        content: content.to_owned(),
        days_since_touched: Some(days),
    }
}

fn session(id: &str, turns: u32, errors: u32, completed: bool) -> SessionSnapshot {
    session_with_age(id, turns, errors, completed, Some(0.0))
}

fn session_with_age(
    id: &str,
    turns: u32,
    errors: u32,
    completed: bool,
    session_age_days: Option<f64>,
) -> SessionSnapshot {
    SessionSnapshot {
        session_id: id.to_owned(),
        turn_count: turns,
        error_count: errors,
        completed,
        session_age_days,
        turn_text: format!("turn text for session {id}"),
    }
}

// ── ConsistencyCheck ─────────────────────────────────────────────────────────

#[tokio::test]
async fn consistency_check_empty_state_returns_no_findings() {
    let check = ConsistencyCheck;
    let state = make_state("test-nous");
    let findings = check.check(&state).await;
    assert!(
        findings.is_empty(),
        "empty state should produce no findings"
    );
}

#[tokio::test]
async fn consistency_check_detects_negation_pair() {
    let check = ConsistencyCheck;
    let mut state = make_state("test-nous");
    state.facts = vec![
        fact("f-001", "Rust memory safety is guaranteed", 1.0),
        fact(
            "f-002",
            "not guaranteed: Rust memory safety in unsafe blocks",
            1.0,
        ),
    ];
    let findings = check.check(&state).await;
    assert!(
        !findings.is_empty(),
        "contradicting facts should produce findings"
    );
    assert!(
        findings
            .first()
            .expect("at least one finding")
            .finding_id
            .starts_with("PROSOCHE-CONSISTENCY"),
        "finding_id should match check kind"
    );
    assert_eq!(
        findings
            .first()
            .expect("at least one finding")
            .evidence_level,
        EvidenceLevel::Exploratory
    );
}

#[tokio::test]
async fn consistency_check_no_contradiction_produces_no_findings() {
    let check = ConsistencyCheck;
    let mut state = make_state("test-nous");
    state.facts = vec![
        fact("f-001", "Rust is a systems language", 1.0),
        fact("f-002", "Python is a scripting language", 1.0),
    ];
    let findings = check.check(&state).await;
    assert!(
        findings.is_empty(),
        "non-contradicting facts should produce no findings: {findings:?}"
    );
}

// ── StalenessCheck ────────────────────────────────────────────────────────────

#[tokio::test]
async fn staleness_check_flags_stale_fact() {
    let check = StalenessCheck::default();
    let mut state = make_state("test-nous");
    state.facts = vec![fact("f-001", "old fact content", 120.0)]; // 120 days > 90-day threshold
    let findings = check.check(&state).await;
    assert_eq!(findings.len(), 1, "should flag one stale fact");
    assert!(
        findings
            .first()
            .expect("at least one finding")
            .finding_id
            .contains("STALENESS-FACT")
    );
    assert_eq!(
        findings
            .first()
            .expect("at least one finding")
            .evidence_level,
        EvidenceLevel::Exploratory
    );
}

#[tokio::test]
async fn staleness_check_ignores_recent_fact() {
    let check = StalenessCheck::default();
    let mut state = make_state("test-nous");
    state.facts = vec![fact("f-001", "fresh fact", 5.0)]; // 5 days, well within threshold
    let findings = check.check(&state).await;
    assert!(findings.is_empty(), "recent fact should not be flagged");
}

#[tokio::test]
async fn staleness_check_flags_old_large_incomplete_session() {
    let check = StalenessCheck::default();
    let mut state = make_state("test-nous");
    state.sessions = vec![session_with_age(
        "s-001",
        15,
        0,
        false,
        Some(check.session_stale_days + 1.0),
    )];
    let findings = check.check(&state).await;
    assert_eq!(
        findings.len(),
        1,
        "should flag one stale incomplete session"
    );
    assert!(
        findings
            .first()
            .expect("at least one finding")
            .finding_id
            .contains("STALENESS-SESSION")
    );
    assert_eq!(
        findings
            .first()
            .expect("at least one finding")
            .evidence_level,
        EvidenceLevel::Interpretive
    );
}

#[tokio::test]
async fn staleness_check_ignores_recent_large_incomplete_session() {
    let check = StalenessCheck::default();
    let mut state = make_state("test-nous");
    state.sessions = vec![session_with_age(
        "s-001",
        15,
        0,
        false,
        Some(check.session_stale_days - 1.0),
    )];
    let findings = check.check(&state).await;
    assert!(
        findings.is_empty(),
        "recent incomplete session should not be stale: {findings:?}"
    );
}

#[tokio::test]
async fn staleness_check_ignores_large_incomplete_session_without_age() {
    let check = StalenessCheck::default();
    let mut state = make_state("test-nous");
    state.sessions = vec![session_with_age("s-001", 15, 0, false, None)];
    let findings = check.check(&state).await;
    assert!(
        findings.is_empty(),
        "age-unknown incomplete session should not be stale: {findings:?}"
    );
}

// ── GoalAlignmentCheck ────────────────────────────────────────────────────────

#[tokio::test]
async fn goal_alignment_check_no_goals_returns_empty() {
    let check = GoalAlignmentCheck;
    let mut state = make_state("test-nous");
    state.sessions = vec![session("s-001", 5, 0, true)];
    let findings = check.check(&state).await;
    assert!(findings.is_empty(), "no goals → no alignment findings");
}

#[tokio::test]
async fn goal_alignment_check_matching_session_no_finding() {
    let check = GoalAlignmentCheck;
    let mut state = make_state("test-nous");
    state.stated_goals = vec!["ship authentication system".to_owned()];
    state.sessions = vec![SessionSnapshot {
        session_id: "s-001".to_owned(),
        turn_count: 5,
        error_count: 0,
        completed: true,
        session_age_days: Some(0.0),
        turn_text: "implementing authentication and login functionality".to_owned(),
    }];
    let findings = check.check(&state).await;
    assert!(
        findings.is_empty(),
        "session sharing goal keywords should not be flagged: {findings:?}"
    );
}

#[tokio::test]
async fn goal_alignment_check_unrelated_session_flagged() {
    let check = GoalAlignmentCheck;
    let mut state = make_state("test-nous");
    state.stated_goals = vec!["build authentication system".to_owned()];
    state.sessions = vec![SessionSnapshot {
        session_id: "s-002".to_owned(),
        turn_count: 5,
        error_count: 0,
        completed: true,
        session_age_days: Some(0.0),
        turn_text: "discussing unrelated topics about coffee and weather".to_owned(),
    }];
    let findings = check.check(&state).await;
    assert_eq!(findings.len(), 1, "unaligned session should be flagged");
    assert!(
        findings
            .first()
            .expect("at least one finding")
            .finding_id
            .contains("GOAL-ALIGNMENT")
    );
    assert_eq!(
        findings
            .first()
            .expect("at least one finding")
            .evidence_level,
        EvidenceLevel::Interpretive
    );
}

// ── SessionQualityCheck ───────────────────────────────────────────────────────

#[tokio::test]
async fn session_quality_check_high_error_rate_flagged() {
    let check = SessionQualityCheck::default();
    let mut state = make_state("test-nous");
    // 4 errors out of 6 turns = 66% error rate > 50% threshold
    state.sessions = vec![session("s-001", 6, 4, true)];
    let findings = check.check(&state).await;
    assert_eq!(
        findings.len(),
        1,
        "high error rate should produce one finding"
    );
    assert!(
        findings
            .first()
            .expect("at least one finding")
            .finding_id
            .contains("SESSION-QUALITY")
    );
    assert_eq!(
        findings
            .first()
            .expect("at least one finding")
            .evidence_level,
        EvidenceLevel::Exploratory
    );
}

#[tokio::test]
async fn session_quality_check_low_error_rate_no_finding() {
    let check = SessionQualityCheck::default();
    let mut state = make_state("test-nous");
    // 1 error out of 10 turns = 10% error rate, well below 50% threshold
    state.sessions = vec![session("s-001", 10, 1, true)];
    let findings = check.check(&state).await;
    assert!(
        findings.is_empty(),
        "low error rate should produce no findings"
    );
}

#[tokio::test]
async fn session_quality_check_all_abandoned_flagged_when_five_or_more() {
    let check = SessionQualityCheck::default();
    let mut state = make_state("test-nous");
    state.sessions = (0..5)
        .map(|i| session(&format!("s-00{i}"), 5, 0, false))
        .collect();
    let findings = check.check(&state).await;
    assert!(
        !findings.is_empty(),
        "5 abandoned sessions should produce at least one finding"
    );
    let all_completed_finding = findings
        .iter()
        .find(|f| f.claim.contains("reached completion"));
    assert!(
        all_completed_finding.is_some(),
        "should have a 'no sessions completed' finding"
    );
}

// ── InstinctPatternsCheck ─────────────────────────────────────────────────────

#[tokio::test]
async fn instinct_patterns_check_empty_state_returns_no_findings() {
    let check = InstinctPatternsCheck;
    let state = make_state("test-nous");
    let findings = check.check(&state).await;
    assert!(
        findings.is_empty(),
        "empty behavior input should not emit a fixed stub finding"
    );
}

#[tokio::test]
async fn instinct_patterns_check_detects_behavioral_patterns() {
    let check = InstinctPatternsCheck;
    let mut state = make_state("test-nous");
    state.sessions = vec![SessionSnapshot {
        session_id: "s-pattern".to_owned(),
        turn_count: 8,
        error_count: 4,
        completed: false,
        session_age_days: Some(0.0),
        turn_text: "synthetic session transcript".to_owned(),
    }];
    state.behavior_patterns = vec![BehaviorPatternSnapshot {
        session_id: "s-pattern".to_owned(),
        tool_call_count: 6,
        tool_error_count: 4,
        repeated_action_count: 2,
        no_progress_turns: 2,
        avoidance_markers: 3,
        confidence_claims: 3,
    }];

    let findings = check.check(&state).await;
    assert!(
        findings.len() >= 3,
        "behavior counters should produce multiple pattern findings: {findings:?}"
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.finding_id.contains("INSTINCT-LOOP")),
        "loop/no-progress pattern should be detected"
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.finding_id.contains("INSTINCT-TOOLS")),
        "tool outcome pattern should be detected"
    );
    assert!(
        findings
            .iter()
            .all(|finding| !finding.finding_id.contains("STUB")),
        "real implementation must not emit stub IDs"
    );

    for finding in findings {
        let support = finding.stats.support.expect("support metadata");
        assert!(!support.is_stub, "instinct findings must not be stubs");
        assert!(support.is_heuristic, "instinct findings are heuristic");
        assert_eq!(finding.evidence_level, EvidenceLevel::Exploratory);
    }
}

#[tokio::test]
async fn instinct_patterns_text_fallback_is_speculative() {
    let check = InstinctPatternsCheck;
    let mut state = make_state("test-nous");
    state.sessions = vec![SessionSnapshot {
        session_id: "s-text".to_owned(),
        turn_count: 6,
        error_count: 1,
        completed: false,
        session_age_days: Some(0.0),
        turn_text: "still failing same error retry again definitely will work".to_owned(),
    }];

    let findings = check.check(&state).await;
    assert!(
        !findings.is_empty(),
        "text fallback should detect weak pattern evidence"
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding.evidence_level == EvidenceLevel::Speculative),
        "text-only evidence must stay speculative"
    );
}

// ── ProsocheAuditRunner integration ───────────────────────────────────────────

#[tokio::test]
async fn audit_runner_runs_all_checks() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::default_checks(dir.path());

    let state = make_state("test-nous");
    let report = runner.run_audit(&state).await;

    assert_eq!(report.nous_id, "test-nous");
    // All 5 check kinds must appear in the summary.
    let kinds: Vec<ProsocheCheckKind> = report.check_summary.iter().map(|s| s.kind).collect();
    assert!(
        kinds.contains(&ProsocheCheckKind::Consistency),
        "Consistency check missing"
    );
    assert!(
        kinds.contains(&ProsocheCheckKind::Staleness),
        "Staleness check missing"
    );
    assert!(
        kinds.contains(&ProsocheCheckKind::GoalAlignment),
        "GoalAlignment check missing"
    );
    assert!(
        kinds.contains(&ProsocheCheckKind::SessionQuality),
        "SessionQuality check missing"
    );
    assert!(
        kinds.contains(&ProsocheCheckKind::InstinctPatterns),
        "InstinctPatterns check missing"
    );
    assert_eq!(report.check_summary.len(), 5, "exactly 5 checks should run");
}

#[tokio::test]
async fn audit_findings_are_stamped() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::default_checks(dir.path());

    let state = make_state("test-nous");
    let report = runner.run_audit(&state).await;

    let meta = report.stamp();
    // Producer must follow the "<crate>@<version>" convention.
    assert!(
        meta.producer.contains('@'),
        "producer must contain '@' separator, got: {}",
        meta.producer
    );
    assert!(
        meta.producer.starts_with("oikonomos@"),
        "producer must start with 'oikonomos@', got: {}",
        meta.producer
    );
    assert_eq!(meta.schema_version, 1, "schema_version must be 1");
    // findings count in stamp must match actual findings.
    let findings_count = meta.row_counts.get("findings").copied().unwrap_or(0);
    assert_eq!(
        findings_count,
        u64::try_from(report.findings.len()).expect("fits"),
        "stamped findings count must match report findings"
    );
}

#[tokio::test]
async fn audit_runner_persists_report_to_disk() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::default_checks(dir.path());

    let state = make_state("test-nous");
    let report = runner.run_audit(&state).await;

    // At least one JSON file should exist in the audit dir.
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "exactly one JSON report should be written"
    );

    // The JSON file should deserialise back to an AuditReport.
    let content = std::fs::read_to_string(entries.first().expect("at least one entry").path())
        .expect("read file");
    let back: AuditReport = serde_json::from_str(&content).expect("deserialise report");
    assert_eq!(back.nous_id, report.nous_id);
    assert_eq!(back.audited_at, report.audited_at);
    assert!(
        report.persisted_path.is_some(),
        "persisted_path should be set on success"
    );
    assert!(
        report.last_persist_error.is_none(),
        "last_persist_error should be None on success"
    );
}

#[tokio::test]
async fn audit_report_serde_round_trip() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::default_checks(dir.path());
    let state = make_state("serde-test-nous");
    let report = runner.run_audit(&state).await;

    let json = serde_json::to_string_pretty(&report.report).expect("serialize");
    let back: AuditReport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.nous_id, report.nous_id);
    assert_eq!(back.findings.len(), report.findings.len());
    assert_eq!(back.check_summary.len(), report.check_summary.len());
}

#[tokio::test]
async fn report_provenance_records_check_versions_and_hashes() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::default_checks(dir.path());
    let state = make_state("provenance-nous");
    let report = runner.run_audit(&state).await;

    let provenance = report.provenance.as_ref().expect("provenance envelope");
    assert_eq!(provenance.report_version, "1.1.0");
    assert_eq!(provenance.checks.len(), 5);
    assert!(provenance.source_query_hash.starts_with("sha256:"));
    assert!(provenance.source_snapshot_hash.starts_with("sha256:"));
    assert!(
        provenance
            .checks
            .iter()
            .any(|check| check.kind == ProsocheCheckKind::InstinctPatterns
                && check.maturity == CheckMaturity::Heuristic)
    );
}

#[tokio::test]
async fn findings_carry_denominators_and_ref_only_support() {
    let check = StalenessCheck::default();
    let mut state = make_state("support-nous");
    state.facts = vec![fact("sensitive-fact", "SECRET-PSYCHE-CONTENT", 120.0)];

    let findings = check.check(&state).await;
    let finding = findings.first().expect("stale fact finding");
    assert_eq!(finding.stats.sample_sizes, Some([1, 1]));
    assert_eq!(finding.stats.rate, Some(1.0));

    let support = finding.stats.support.as_ref().expect("support metadata");
    assert!(support.is_heuristic);
    assert!(!support.is_stub);
    assert!(support.evidence_refs.iter().any(|evidence| matches!(
        evidence,
        EvidenceRef::Fact {
            fact_id,
            content_hash
        } if fact_id == "sensitive-fact" && content_hash.starts_with("sha256:")
    )));
}

#[tokio::test]
async fn persisted_report_does_not_copy_fact_or_session_content() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::default_checks(dir.path());
    let mut state = make_state("privacy-nous");
    state.facts = vec![fact("fact-secret", "SECRET-PSYCHE-CONTENT", 120.0)];
    state.sessions = vec![SessionSnapshot {
        session_id: "session-secret".to_owned(),
        turn_count: 12,
        error_count: 0,
        completed: false,
        session_age_days: Some(30.0),
        turn_text: "VERY-SENSITIVE-SESSION-TURN".to_owned(),
    }];

    let report = runner.run_audit(&state).await;
    let json = serde_json::to_string_pretty(&report.report).expect("serialize report");

    assert!(!json.contains("SECRET-PSYCHE-CONTENT"));
    assert!(!json.contains("VERY-SENSITIVE-SESSION-TURN"));
    assert!(json.contains("sha256:"));
    assert!(json.contains("evidence_refs"));
}

struct PanicCheck;

impl ProsocheCheck for PanicCheck {
    fn check<'a>(
        &'a self,
        _state: &'a ProsocheState,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<Finding>> + Send + 'a>> {
        Box::pin(async move {
            panic!("simulated prosoche check panic");
        })
    }

    fn kind(&self) -> ProsocheCheckKind {
        ProsocheCheckKind::Consistency
    }
}

#[tokio::test]
async fn check_failures_are_recorded_in_provenance() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::new(
        vec![std::sync::Arc::new(PanicCheck)],
        AuditStorage::new(dir.path()),
    );
    let state = make_state("failure-nous");

    let report = runner.run_audit(&state).await;
    let provenance = report.provenance.as_ref().expect("provenance envelope");
    assert_eq!(provenance.check_failures.len(), 1);
    let failure = provenance
        .check_failures
        .first()
        .expect("one failure recorded");
    let summary = report.check_summary.first().expect("one summary row");
    assert_eq!(failure.kind, ProsocheCheckKind::Consistency);
    assert!(failure.reason.contains("panic"));
    assert_eq!(summary.findings_count, 0);
}

#[tokio::test]
async fn audit_runner_reports_persist_error_for_unwritable_storage() {
    let file = tempfile::NamedTempFile::new().expect("create tempfile");
    let runner = ProsocheAuditRunner::default_checks(file.path());

    let mut state = make_state("persist-error-nous");
    state.sessions = vec![session("s-001", 6, 4, true)];

    let outcome = runner.run_audit(&state).await;

    assert_eq!(outcome.report.nous_id, "persist-error-nous");
    assert_eq!(outcome.report.check_summary.len(), 5);

    assert!(
        outcome.persisted_path.is_none(),
        "no path should be reported when persistence fails"
    );
    let err = outcome
        .last_persist_error
        .expect("last_persist_error should be set");
    assert!(!err.is_empty(), "error message should not be empty");
}
