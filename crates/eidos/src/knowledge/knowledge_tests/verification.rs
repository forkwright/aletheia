//! (See parent module for full rationale.)

use super::super::*;
use super::test_timestamp;
use crate::id::FactId;

fn test_nous_id(name: &str) -> koina::id::NousId {
    koina::id::NousId::new(name).expect("valid test nous id")
}

fn make_test_fact(tier: EpistemicTier, recorded_at: jiff::Timestamp) -> Fact {
    Fact {
        id: FactId::new("f-test").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "test fact".to_owned(),
        fact_type: "observation".to_owned(),
        scope: None,
        temporal: FactTemporal {
            valid_from: recorded_at,
            valid_to: far_future(),
            recorded_at,
        },
        provenance: FactProvenance {
            confidence: 0.8,
            tier,
            source_session_id: None,
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
        sensitivity: FactSensitivity::Public,
    }
}

#[test]
fn published_fact_serde_roundtrip() {
    let original = PublishedFact {
        id: PublishedFactId("pf-001".to_owned()),
        original_fact_id: FactId::new("f-orig").expect("valid test id"),
        published_by: test_nous_id("alice"),
        published_at: test_timestamp("2026-01-01T00:00:00Z"),
        verification_count: 3,
        contested_by: vec![test_nous_id("bob")],
        contest_reason: Some("source unclear".to_owned()),
    };
    let json = serde_json::to_string(&original).expect("PublishedFact serializes");
    let back: PublishedFact = serde_json::from_str(&json).expect("PublishedFact deserializes");
    assert_eq!(original.id.0, back.id.0);
    assert_eq!(
        original.original_fact_id.as_str(),
        back.original_fact_id.as_str()
    );
    assert_eq!(original.published_by.as_str(), back.published_by.as_str());
    assert_eq!(original.verification_count, back.verification_count);
    assert_eq!(original.contested_by.len(), back.contested_by.len());
    assert_eq!(original.contest_reason, back.contest_reason);
}

#[test]
fn fact_access_grant_serde_roundtrip() {
    let original = FactAccessGrant {
        fact_id: FactId::new("f-001").expect("valid test id"),
        grantee: test_nous_id("charlie"),
        granted_at: test_timestamp("2026-02-01T12:00:00Z"),
    };
    let json = serde_json::to_string(&original).expect("FactAccessGrant serializes");
    let back: FactAccessGrant = serde_json::from_str(&json).expect("FactAccessGrant deserializes");
    assert_eq!(original.fact_id.as_str(), back.fact_id.as_str());
    assert_eq!(original.grantee.as_str(), back.grantee.as_str());
}

#[test]
fn verification_proposal_serde_roundtrip() {
    let original = VerificationProposal {
        fact_id: FactId::new("f-prop").expect("valid test id"),
        proposing_nous: test_nous_id("dave"),
        proposed_tier: EpistemicTier::Verified,
        votes: vec![VerificationVote {
            voter: test_nous_id("eve"),
            verdict: VerificationVerdict::Accept,
            at: test_timestamp("2026-03-01T00:00:00Z"),
        }],
    };
    let json = serde_json::to_string(&original).expect("VerificationProposal serializes");
    let back: VerificationProposal =
        serde_json::from_str(&json).expect("VerificationProposal deserializes");
    assert_eq!(original.fact_id.as_str(), back.fact_id.as_str());
    assert_eq!(original.votes.len(), back.votes.len());
    assert_eq!(
        original.votes.first().map(|v| v.verdict),
        back.votes.first().map(|v| v.verdict)
    );
}

#[test]
fn verification_vote_serde_roundtrip() {
    let original = VerificationVote {
        voter: test_nous_id("frank"),
        verdict: VerificationVerdict::Contest,
        at: test_timestamp("2026-04-01T00:00:00Z"),
    };
    let json = serde_json::to_string(&original).expect("VerificationVote serializes");
    let back: VerificationVote =
        serde_json::from_str(&json).expect("VerificationVote deserializes");
    assert_eq!(original.voter.as_str(), back.voter.as_str());
    assert_eq!(original.verdict, back.verdict);
}

#[test]
fn verification_verdict_serde_roundtrip() {
    for verdict in [
        VerificationVerdict::Accept,
        VerificationVerdict::Contest,
        VerificationVerdict::Abstain,
    ] {
        let json = serde_json::to_string(&verdict).expect("VerificationVerdict serializes");
        let back: VerificationVerdict =
            serde_json::from_str(&json).expect("VerificationVerdict deserializes");
        assert_eq!(verdict, back, "roundtrip failed for {verdict:?}");
    }
}

#[test]
fn conflict_resolution_serde_roundtrip() {
    let original = ConflictResolution {
        winner: FactId::new("f-win").expect("valid test id"),
        losers: vec![FactId::new("f-lose").expect("valid test id")],
        winning_score: 0.91,
        resolved_at: test_timestamp("2026-05-01T00:00:00Z"),
    };
    let json = serde_json::to_string(&original).expect("ConflictResolution serializes");
    let back: ConflictResolution =
        serde_json::from_str(&json).expect("ConflictResolution deserializes");
    assert_eq!(original.winner.as_str(), back.winner.as_str());
    assert_eq!(original.losers.len(), back.losers.len());
    assert!((original.winning_score - back.winning_score).abs() < f64::EPSILON);
}

#[test]
fn compute_score_determinism() {
    let now = test_timestamp("2026-06-01T00:00:00Z");
    let fact = make_test_fact(EpistemicTier::Verified, now);
    let score1 = ConflictResolution::compute_score(&fact, 2, now);
    let score2 = ConflictResolution::compute_score(&fact, 2, now);
    assert!(
        (score1 - score2).abs() < f64::EPSILON,
        "compute_score must be deterministic"
    );
}

#[test]
fn compute_score_tier_ordering() {
    let now = test_timestamp("2026-06-01T00:00:00Z");
    let recorded_at = test_timestamp("2026-06-01T00:00:00Z");

    let verified = make_test_fact(EpistemicTier::Verified, recorded_at);
    let inferred = make_test_fact(EpistemicTier::Inferred, recorded_at);
    let training = make_test_fact(EpistemicTier::Training, recorded_at);
    let assumed = make_test_fact(EpistemicTier::Assumed, recorded_at);

    let s_verified = ConflictResolution::compute_score(&verified, 1, now);
    let s_inferred = ConflictResolution::compute_score(&inferred, 1, now);
    let s_training = ConflictResolution::compute_score(&training, 1, now);
    let s_assumed = ConflictResolution::compute_score(&assumed, 1, now);

    assert!(s_verified > s_inferred, "Verified should outscore Inferred");
    assert!(s_inferred > s_training, "Inferred should outscore Training");
    assert!(s_training > s_assumed, "Training should outscore Assumed");
}

#[test]
fn compute_score_recency_boundary() {
    let now = test_timestamp("2026-06-01T00:00:00Z");
    let fresh = make_test_fact(EpistemicTier::Verified, now);
    let stale = make_test_fact(
        EpistemicTier::Verified,
        test_timestamp("2026-05-01T00:00:00Z"),
    );

    let s_fresh = ConflictResolution::compute_score(&fresh, 1, now);
    let s_stale = ConflictResolution::compute_score(&stale, 1, now);

    assert!(
        s_fresh > s_stale,
        "fresh fact (now) should outscore 31-day-old fact"
    );
}

#[test]
fn compute_score_supporter_saturation() {
    let now = test_timestamp("2026-06-01T00:00:00Z");
    let fact = make_test_fact(EpistemicTier::Verified, now);

    let s_5 = ConflictResolution::compute_score(&fact, 5, now);
    let s_50 = ConflictResolution::compute_score(&fact, 50, now);

    assert!(
        (s_5 - s_50).abs() < f64::EPSILON,
        "5 supporters and 50 supporters should yield identical score (saturation at 5)"
    );
}
