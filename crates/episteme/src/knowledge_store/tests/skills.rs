#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use crate::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
};
use crate::test_fixtures::{make_fact, make_store, test_ts};
fn make_skill_fact(id: &str, nous_id: &str, skill_name: &str, domain_tags: &[&str]) -> Fact {
    let content = serde_json::to_string(&crate::skill::SkillContent {
        name: skill_name.to_owned(),
        description: format!("Skill: {skill_name}"),
        steps: vec!["step 1".to_owned()],
        tools_used: vec!["Read".to_owned()],
        domain_tags: domain_tags.iter().map(|t| (*t).to_owned()).collect(),
        origin: "seeded".to_owned(),
        triggers: vec![],
        always: false,
        pending_fact_id: None,
        source_evidence: vec![],
        extraction_audit: None,
        review_decision: None,
    })
    .expect("skill content serializes to JSON");
    Fact {
        id: crate::id::FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        content,
        fact_type: "skill".to_owned(),
        temporal: FactTemporal {
            valid_from: test_ts("2026-01-01"),
            valid_to: crate::knowledge::far_future(),
            recorded_at: test_ts("2026-03-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.5,
            tier: EpistemicTier::Assumed,
            source_session_id: None,
            stability_hours: 2190.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: crate::knowledge::FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    }
}

#[test]
fn find_skills_for_nous_returns_only_skills() {
    let store = make_store();
    let skill = make_skill_fact("sk-1", "alice", "rust-errors", &["rust"]);
    store.insert_fact(&skill).expect("insert skill");
    let non_skill = make_fact("f-1", "alice", "Alice likes cats");
    store.insert_fact(&non_skill).expect("insert non-skill");
    let results = store.find_skills_for_nous("alice", 100).expect("query");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].fact_type, "skill");
}

#[test]
fn find_skills_for_nous_ordered_by_confidence() {
    let store = make_store();
    let mut low = make_skill_fact("sk-low", "alice", "low-conf", &["test"]);
    low.provenance.confidence = 0.3;
    store.insert_fact(&low).expect("insert low");
    let mut high = make_skill_fact("sk-high", "alice", "high-conf", &["test"]);
    high.provenance.confidence = 0.9;
    store.insert_fact(&high).expect("insert high");
    let results = store.find_skills_for_nous("alice", 100).expect("query");
    assert_eq!(results.len(), 2);
    assert!(
        results[0].provenance.confidence >= results[1].provenance.confidence,
        "skills should be ordered by confidence descending"
    );
}

#[test]
fn find_skills_nous_scoping() {
    let store = make_store();
    let alice_skill = make_skill_fact("sk-a", "alice", "alice-skill", &["rust"]);
    store.insert_fact(&alice_skill).expect("insert alice");
    let bob_skill = make_skill_fact("sk-b", "bob", "bob-skill", &["python"]);
    store.insert_fact(&bob_skill).expect("insert bob");
    let alice_results = store
        .find_skills_for_nous("alice", 100)
        .expect("query alice");
    assert_eq!(alice_results.len(), 1);
    assert_eq!(alice_results[0].id.as_str(), "sk-a");
    let bob_results = store.find_skills_for_nous("bob", 100).expect("query bob");
    assert_eq!(bob_results.len(), 1);
    assert_eq!(bob_results[0].id.as_str(), "sk-b");
}

#[test]
fn find_skills_by_domain_filters_tags() {
    let store = make_store();
    let rust_skill = make_skill_fact("sk-r", "alice", "rust-errors", &["rust", "errors"]);
    store.insert_fact(&rust_skill).expect("insert rust");
    let py_skill = make_skill_fact("sk-p", "alice", "python-web", &["python", "web"]);
    store.insert_fact(&py_skill).expect("insert python");
    let results = store
        .find_skills_by_domain("alice", &["rust"], 100)
        .expect("query");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id.as_str(), "sk-r");
    let results = store
        .find_skills_by_domain("alice", &["web"], 100)
        .expect("query");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id.as_str(), "sk-p");
    let results = store
        .find_skills_by_domain("alice", &["rust", "python"], 100)
        .expect("query");
    assert_eq!(results.len(), 2);
}

#[test]
fn find_skills_by_domain_empty_tags() {
    let store = make_store();
    let skill = make_skill_fact("sk-1", "alice", "some-skill", &["rust"]);
    store.insert_fact(&skill).expect("insert");
    let results = store
        .find_skills_by_domain("alice", &[], 100)
        .expect("query");
    assert!(results.is_empty(), "empty tags should match nothing");
}

#[test]
fn find_skill_by_name_found() {
    let store = make_store();
    let skill = make_skill_fact("sk-named", "alice", "rust-error-handling", &["rust"]);
    store.insert_fact(&skill).expect("insert");
    let found = store
        .find_skill_by_name("alice", "rust-error-handling")
        .expect("query");
    assert_eq!(found, Some("sk-named".to_owned()));
}

#[test]
fn find_skill_by_name_not_found() {
    let store = make_store();
    let skill = make_skill_fact("sk-1", "alice", "actual-name", &["test"]);
    store.insert_fact(&skill).expect("insert");
    let found = store
        .find_skill_by_name("alice", "nonexistent")
        .expect("query");
    assert!(found.is_none());
}

#[test]
fn find_skills_excludes_forgotten() {
    let store = make_store();
    let skill = make_skill_fact("sk-forget", "alice", "forgotten-skill", &["test"]);
    store.insert_fact(&skill).expect("insert");
    store
        .forget_fact(
            &crate::id::FactId::new("sk-forget").expect("valid test id"),
            crate::knowledge::ForgetReason::Outdated,
        )
        .expect("forget");
    let results = store.find_skills_for_nous("alice", 100).expect("query");
    assert!(
        results.is_empty(),
        "forgotten skills should not be returned"
    );
}

#[test]
fn search_skills_bm25() {
    let store = make_store();
    let skill1 = make_skill_fact("sk-docker", "alice", "docker-deploy", &["docker"]);
    store.insert_fact(&skill1).expect("insert docker");
    let skill2 = make_skill_fact("sk-k8s", "alice", "kubernetes-deploy", &["k8s"]);
    store.insert_fact(&skill2).expect("insert k8s");
    let results = store.search_skills("alice", "docker", 10).expect("search");
    assert!(
        results.iter().any(|f| f.id.as_str() == "sk-docker"),
        "search should find docker skill"
    );
}

#[test]
fn skill_usage_tracking_via_increment_access() {
    let store = make_store();
    let skill = make_skill_fact("sk-usage", "alice", "usage-test", &["rust"]);
    store.insert_fact(&skill).expect("insert skill");
    store
        .increment_access(&[crate::id::FactId::new("sk-usage").expect("valid test id")])
        .expect("increment");
    store
        .increment_access(&[crate::id::FactId::new("sk-usage").expect("valid test id")])
        .expect("increment again");
    let results = store.find_skills_for_nous("alice", 100).expect("query");
    let found = results
        .iter()
        .find(|f| f.id.as_str() == "sk-usage")
        .expect("find skill");
    assert_eq!(
        found.access.access_count, 2,
        "usage_count should be 2 after two increments"
    );
    assert!(
        found.access.last_accessed_at.is_some(),
        "last_accessed_at should be set"
    );
}

#[test]
fn skill_decay_retires_stale_skills() {
    let store = make_store();
    let mut stale = make_skill_fact("sk-stale", "alice", "stale-skill", &["test"]);
    stale.temporal.valid_from = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(24 * 120))
        .expect("subtract 120 days");
    stale.provenance.confidence = 0.5;
    stale.access.access_count = 0;
    store.insert_fact(&stale).expect("insert stale skill");
    let mut fresh = make_skill_fact("sk-fresh", "alice", "fresh-skill", &["test"]);
    // WHY: Override defaults so the skill is clearly fresh (valid_from=now, high confidence).
    // make_skill_fact defaults to valid_from=2026-01-01 which can look stale to decay logic.
    fresh.temporal.valid_from = jiff::Timestamp::now();
    fresh.provenance.confidence = 0.9;
    fresh.access.access_count = 5;
    store.insert_fact(&fresh).expect("insert fresh skill");
    let (active, _needs_review, retired) = store.run_skill_decay("alice").expect("run skill decay");
    assert!(
        retired >= 1,
        "stale skill should be retired, got retired={retired}"
    );
    assert!(
        active >= 1,
        "fresh skill should still be active, got active={active}"
    );
}

#[test]
fn skill_quality_metrics_returns_correct_counts() {
    let store = make_store();
    let skill1 = make_skill_fact("sk-m1", "alice", "skill-one", &["rust"]);
    store.insert_fact(&skill1).expect("insert skill 1");
    let mut skill2 = make_skill_fact("sk-m2", "alice", "skill-two", &["python"]);
    skill2.access.access_count = 5;
    store.insert_fact(&skill2).expect("insert skill 2");
    let metrics = store.skill_quality_metrics("alice").expect("get metrics");
    assert_eq!(metrics.total_active, 2);
    assert!(metrics.avg_usage_count > 0.0);
}

#[test]
fn skill_decay_high_usage_survives_longer() {
    let store = make_store();
    let past = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(24 * 50))
        .expect("subtract 50 days");
    let mut low = make_skill_fact("sk-low-use", "alice", "low-usage", &["test"]);
    low.temporal.valid_from = past;
    low.access.access_count = 1;
    low.provenance.confidence = 0.7;
    store.insert_fact(&low).expect("insert low usage");
    let mut high = make_skill_fact("sk-high-use", "alice", "high-usage", &["test"]);
    high.temporal.valid_from = past;
    high.access.access_count = 15;
    high.provenance.confidence = 0.7;
    store.insert_fact(&high).expect("insert high usage");
    let (active, _needs_review, retired) = store.run_skill_decay("alice").expect("run decay");
    let remaining = store.find_skills_for_nous("alice", 100).expect("query");
    let high_survived = remaining.iter().any(|f| f.id.as_str() == "sk-high-use");
    assert!(
        high_survived,
        "high-usage skill should survive 50-day decay, active={active}, retired={retired}"
    );
}

fn make_pending_skill_fact(
    id: &str,
    nous_id: &str,
    skill_name: &str,
    session_id: &str,
) -> (Fact, crate::skills::PendingSkill) {
    use crate::skills::{ExtractionAudit, PendingSkill, SkillEvidence, ToolCallRecord, sha256_hex};

    let extracted = crate::skills::extract::ExtractedSkill {
        name: skill_name.to_owned(),
        description: format!("Skill: {skill_name}"),
        steps: vec!["step 1".to_owned()],
        tools_used: vec!["Read".to_owned()],
        domain_tags: vec!["test".to_owned()],
        when_to_use: "For tests".to_owned(),
    };
    let audit = ExtractionAudit {
        prompt_hash: sha256_hex(b"prompt"),
        response_hash: Some(sha256_hex(b"response")),
        model: Some("test-model".to_owned()),
        extracted_at: test_ts("2026-03-01T00:00:00Z"),
    };
    let evidence = vec![SkillEvidence {
        session_id: session_id.to_owned(),
        turn_sequence_hash: Some(sha256_hex(b"seq")),
        tool_calls: vec![ToolCallRecord::with_hashes(
            "Read",
            100,
            false,
            Some(sha256_hex(b"input")),
            Some(sha256_hex(b"result")),
        )],
    }];
    let pending = PendingSkill::with_provenance(&extracted, "cand-test", evidence, audit);
    let content = pending.to_json().expect("pending skill serializes to JSON");
    let fact = Fact {
        id: crate::id::FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        content,
        fact_type: "skill_pending".to_owned(),
        temporal: FactTemporal {
            valid_from: test_ts("2026-03-01"),
            valid_to: crate::knowledge::far_future(),
            recorded_at: test_ts("2026-03-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.6,
            tier: EpistemicTier::Inferred,
            source_session_id: Some(session_id.to_owned()),
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: crate::knowledge::FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    };
    (fact, pending)
}

#[test]
fn approve_pending_skill_transfers_provenance() {
    let store = make_store();
    let session_id = "session-approve";
    let (pending_fact, _) = make_pending_skill_fact("ps-1", "alice", "approve-skill", session_id);
    store
        .insert_fact(&pending_fact)
        .expect("insert pending skill");

    let new_id = store
        .approve_pending_skill(
            &crate::id::FactId::new("ps-1").expect("valid test id"),
            "alice",
            "reviewer-alice",
            Some("looks useful"),
        )
        .expect("approve pending skill");

    let skills = store
        .find_skills_for_nous("alice", 100)
        .expect("query skills");
    let approved = skills
        .iter()
        .find(|f| f.id == new_id)
        .expect("approved skill exists");
    assert_eq!(
        approved.provenance.source_session_id.as_deref(),
        Some(session_id)
    );

    let content =
        serde_json::from_str::<crate::skill::SkillContent>(&approved.content).expect("parse skill");
    assert_eq!(
        content.pending_fact_id.as_deref(),
        Some("ps-1"),
        "approved skill should link back to pending fact"
    );
    assert_eq!(content.source_evidence.len(), 1);
    assert_eq!(content.source_evidence[0].session_id, session_id);
    assert!(content.extraction_audit.is_some());
    let decision = content
        .review_decision
        .expect("approved skill should record review decision");
    assert_eq!(decision.actor, "reviewer-alice");
    assert_eq!(decision.action, "approved");
    assert_eq!(decision.reason.as_deref(), Some("looks useful"));
}

#[test]
fn reject_pending_skill_records_review_decision() {
    let store = make_store();
    let session_id = "session-reject";
    let (pending_fact, _) = make_pending_skill_fact("ps-2", "alice", "reject-skill", session_id);
    store
        .insert_fact(&pending_fact)
        .expect("insert pending skill");

    store
        .reject_pending_skill(
            &crate::id::FactId::new("ps-2").expect("valid test id"),
            "reviewer-bob",
            Some("not generalizable"),
        )
        .expect("reject pending skill");

    let pending_facts = store.find_pending_skills("alice").expect("query pending");
    assert!(
        pending_facts.is_empty(),
        "rejected skill should no longer be pending"
    );

    let rejected = store
        .audit_all_facts("alice", 10)
        .expect("audit query")
        .into_iter()
        .find(|f| f.id.as_str() == "ps-2")
        .expect("rejected pending fact remains in audit trail");
    assert!(rejected.lifecycle.is_forgotten);
    let content = serde_json::from_str::<crate::skills::PendingSkill>(&rejected.content)
        .expect("parse pending");
    assert_eq!(content.status, "rejected");
    let decision = content
        .review_decision
        .expect("rejected skill should record review decision");
    assert_eq!(decision.actor, "reviewer-bob");
    assert_eq!(decision.action, "rejected");
    assert_eq!(decision.reason.as_deref(), Some("not generalizable"));
}
