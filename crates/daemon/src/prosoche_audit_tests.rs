#![expect(clippy::expect_used, reason = "test assertions")]

use eidos::meta::Stamped;

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
    SessionSnapshot {
        session_id: id.to_owned(),
        turn_count: turns,
        error_count: errors,
        completed,
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
async fn staleness_check_flags_large_incomplete_session() {
    let check = StalenessCheck::default();
    let mut state = make_state("test-nous");
    state.sessions = vec![session("s-001", 15, 0, false)]; // 15 turns, not completed
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
async fn instinct_patterns_check_stub_returns_speculative_finding() {
    let check = InstinctPatternsCheck;
    let state = make_state("test-nous");
    let findings = check.check(&state).await;
    assert_eq!(findings.len(), 1, "stub should return exactly one finding");
    assert_eq!(
        findings
            .first()
            .expect("at least one finding")
            .evidence_level,
        EvidenceLevel::Speculative,
        "stub finding must be Speculative"
    );
    assert!(
        findings
            .first()
            .expect("at least one finding")
            .finding_id
            .contains("INSTINCT-STUB"),
        "stub finding ID should include INSTINCT-STUB"
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
}

#[tokio::test]
async fn audit_report_serde_round_trip() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let runner = ProsocheAuditRunner::default_checks(dir.path());
    let state = make_state("serde-test-nous");
    let report = runner.run_audit(&state).await;

    let json = serde_json::to_string_pretty(&report).expect("serialize");
    let back: AuditReport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.nous_id, report.nous_id);
    assert_eq!(back.findings.len(), report.findings.len());
    assert_eq!(back.check_summary.len(), report.check_summary.len());
}
