#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use crate::knowledge::{EpistemicTier, Fact};
use std::sync::Arc;

const DIM: usize = 4;

fn make_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM }).expect("open_mem")
}

fn test_ts(s: &str) -> jiff::Timestamp {
    crate::knowledge::parse_timestamp(s).expect("valid test timestamp in test helper")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: crate::id::FactId::new_unchecked(id),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: test_ts("2026-01-01"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: test_ts("2026-03-01T00:00:00Z"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    }
}

fn make_skill_fact(id: &str, nous_id: &str, skill_name: &str, domain_tags: &[&str]) -> Fact {
    let content = serde_json::to_string(&crate::skill::SkillContent {
        name: skill_name.to_owned(),
        description: format!("Skill: {skill_name}"),
        steps: vec!["step 1".to_owned()],
        tools_used: vec!["Read".to_owned()],
        domain_tags: domain_tags.iter().map(|t| (*t).to_owned()).collect(),
        origin: "seeded".to_owned(),
    })
    .expect("skill content serializes to JSON");
    Fact {
        id: crate::id::FactId::new_unchecked(id),
        nous_id: nous_id.to_owned(),
        content,
        confidence: 0.5,
        tier: EpistemicTier::Assumed,
        valid_from: test_ts("2026-01-01"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: test_ts("2026-03-01T00:00:00Z"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 2190.0,
        fact_type: "skill".to_owned(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
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
    low.confidence = 0.3;
    store.insert_fact(&low).expect("insert low");
    let mut high = make_skill_fact("sk-high", "alice", "high-conf", &["test"]);
    high.confidence = 0.9;
    store.insert_fact(&high).expect("insert high");
    let results = store.find_skills_for_nous("alice", 100).expect("query");
    assert_eq!(results.len(), 2);
    assert!(
        results[0].confidence >= results[1].confidence,
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
            &crate::id::FactId::new_unchecked("sk-forget"),
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
        .increment_access(&[crate::id::FactId::new_unchecked("sk-usage")])
        .expect("increment");
    store
        .increment_access(&[crate::id::FactId::new_unchecked("sk-usage")])
        .expect("increment again");
    let results = store.find_skills_for_nous("alice", 100).expect("query");
    let found = results
        .iter()
        .find(|f| f.id.as_str() == "sk-usage")
        .expect("find skill");
    assert_eq!(
        found.access_count, 2,
        "usage_count should be 2 after two increments"
    );
    assert!(
        found.last_accessed_at.is_some(),
        "last_accessed_at should be set"
    );
}

#[test]
fn skill_decay_retires_stale_skills() {
    let store = make_store();
    let mut stale = make_skill_fact("sk-stale", "alice", "stale-skill", &["test"]);
    stale.valid_from = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(24 * 120))
        .expect("subtract 120 days");
    stale.confidence = 0.5;
    stale.access_count = 0;
    store.insert_fact(&stale).expect("insert stale skill");
    let mut fresh = make_skill_fact("sk-fresh", "alice", "fresh-skill", &["test"]);
    // WHY: Override defaults so the skill is clearly fresh (valid_from=now, high confidence).
    // make_skill_fact defaults to valid_from=2026-01-01 which can look stale to decay logic.
    fresh.valid_from = jiff::Timestamp::now();
    fresh.confidence = 0.9;
    fresh.access_count = 5;
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
    skill2.access_count = 5;
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
    low.valid_from = past;
    low.access_count = 1;
    low.confidence = 0.7;
    store.insert_fact(&low).expect("insert low usage");
    let mut high = make_skill_fact("sk-high-use", "alice", "high-usage", &["test"]);
    high.valid_from = past;
    high.access_count = 15;
    high.confidence = 0.7;
    store.insert_fact(&high).expect("insert high usage");
    let (active, _needs_review, retired) = store.run_skill_decay("alice").expect("run decay");
    let remaining = store.find_skills_for_nous("alice", 100).expect("query");
    let high_survived = remaining.iter().any(|f| f.id.as_str() == "sk-high-use");
    assert!(
        high_survived,
        "high-usage skill should survive 50-day decay, active={active}, retired={retired}"
    );
}
