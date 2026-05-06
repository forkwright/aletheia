//! Tests for the verification protocol.

#![expect(clippy::expect_used, reason = "test assertions")]

use eidos::id::FactId;
use eidos::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    VerificationProposal, VerificationVerdict, VerificationVote,
};
use jiff::Timestamp;
use koina::id::NousId;

use super::conflict::resolve_conflict;
use super::proposal::{
    DEFAULT_VERIFICATION_THRESHOLD, VerificationOutcome, publish_fact, vote_on_proposal,
};

fn make_fact(id: &str, confidence: f64, tier: EpistemicTier, recorded_at: Timestamp) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid test id"),
        nous_id: "test-nous".to_owned(),
        fact_type: "preference".to_owned(),
        content: format!("test fact {id}"),
        scope: None,
        sensitivity: FactSensitivity::Public,
        temporal: FactTemporal {
            valid_from: recorded_at,
            valid_to: recorded_at,
            recorded_at,
        },
        provenance: FactProvenance {
            confidence,
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
    }
}

#[test]
fn publish_fact_produces_distinct_published_ids() {
    let now = Timestamp::UNIX_EPOCH;
    let f = make_fact("f1", 0.9, EpistemicTier::Inferred, now);
    let nous = NousId::new("publisher").expect("valid test id");

    let p1 = publish_fact(&f, &nous);
    let p2 = publish_fact(&f, &nous);

    assert_ne!(p1.id.0, p2.id.0, "PublishedFactIds must be unique");
    assert_eq!(
        p1.original_fact_id.to_string(),
        p2.original_fact_id.to_string()
    );
    assert_eq!(p1.verification_count, 0);
    assert!(p1.contested_by.is_empty());
}

#[test]
fn three_distinct_accepts_promote() {
    let mut proposal = VerificationProposal {
        fact_id: FactId::new("fact-1").expect("valid test id"),
        proposing_nous: NousId::new("a").expect("valid test id"),
        proposed_tier: EpistemicTier::Verified,
        votes: Vec::new(),
    };

    let now = Timestamp::UNIX_EPOCH;
    let outcome1 = vote_on_proposal(
        &mut proposal,
        VerificationVote {
            voter: NousId::new("a").expect("valid test id"),
            verdict: VerificationVerdict::Accept,
            at: now,
        },
        DEFAULT_VERIFICATION_THRESHOLD,
    );
    assert_eq!(outcome1, VerificationOutcome::Pending);

    let outcome2 = vote_on_proposal(
        &mut proposal,
        VerificationVote {
            voter: NousId::new("b").expect("valid test id"),
            verdict: VerificationVerdict::Accept,
            at: now,
        },
        DEFAULT_VERIFICATION_THRESHOLD,
    );
    assert_eq!(outcome2, VerificationOutcome::Pending);

    let outcome3 = vote_on_proposal(
        &mut proposal,
        VerificationVote {
            voter: NousId::new("c").expect("valid test id"),
            verdict: VerificationVerdict::Accept,
            at: now,
        },
        DEFAULT_VERIFICATION_THRESHOLD,
    );
    assert!(matches!(
        outcome3,
        VerificationOutcome::Promoted {
            new_tier: EpistemicTier::Verified
        }
    ));
}

#[test]
fn duplicate_voter_does_not_double_count() {
    let mut proposal = VerificationProposal {
        fact_id: FactId::new("fact-1").expect("valid test id"),
        proposing_nous: NousId::new("a").expect("valid test id"),
        proposed_tier: EpistemicTier::Verified,
        votes: Vec::new(),
    };
    let now = Timestamp::UNIX_EPOCH;

    for _ in 0..5 {
        let outcome = vote_on_proposal(
            &mut proposal,
            VerificationVote {
                voter: NousId::new("a").expect("valid test id"),
                verdict: VerificationVerdict::Accept,
                at: now,
            },
            DEFAULT_VERIFICATION_THRESHOLD,
        );
        // Same voter five times — never reaches threshold of 3 distinct.
        assert_eq!(outcome, VerificationOutcome::Pending);
    }
}

#[test]
fn contest_vote_short_circuits() {
    let mut proposal = VerificationProposal {
        fact_id: FactId::new("fact-1").expect("valid test id"),
        proposing_nous: NousId::new("a").expect("valid test id"),
        proposed_tier: EpistemicTier::Verified,
        votes: Vec::new(),
    };
    let now = Timestamp::UNIX_EPOCH;

    vote_on_proposal(
        &mut proposal,
        VerificationVote {
            voter: NousId::new("a").expect("valid test id"),
            verdict: VerificationVerdict::Accept,
            at: now,
        },
        DEFAULT_VERIFICATION_THRESHOLD,
    );
    let outcome = vote_on_proposal(
        &mut proposal,
        VerificationVote {
            voter: NousId::new("b").expect("valid test id"),
            verdict: VerificationVerdict::Contest,
            at: now,
        },
        DEFAULT_VERIFICATION_THRESHOLD,
    );
    assert!(matches!(outcome, VerificationOutcome::Contested { .. }));

    // Even adding more accepts doesn't recover.
    let outcome2 = vote_on_proposal(
        &mut proposal,
        VerificationVote {
            voter: NousId::new("c").expect("valid test id"),
            verdict: VerificationVerdict::Accept,
            at: now,
        },
        DEFAULT_VERIFICATION_THRESHOLD,
    );
    assert!(matches!(outcome2, VerificationOutcome::Contested { .. }));
}

#[test]
fn resolve_conflict_picks_highest_score() {
    let now = Timestamp::UNIX_EPOCH;
    let strong = make_fact("strong", 0.9, EpistemicTier::Verified, now);
    let weak = make_fact("weak", 0.3, EpistemicTier::Assumed, now);

    let resolution =
        resolve_conflict(&[&strong, &weak], &[3, 1], now).expect("non-empty matching slices");
    assert_eq!(resolution.winner.to_string(), "strong");
    assert_eq!(resolution.losers.len(), 1);
    assert_eq!(
        resolution.losers.first().map(ToString::to_string),
        Some("weak".to_owned())
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn fresh_store_migrates_to_v10() {
    use std::collections::BTreeMap;

    use crate::knowledge_store::KnowledgeStore;

    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");
    let v = store.schema_version().expect("read schema version");
    assert_eq!(v, 10, "fresh store should initialize at SCHEMA_VERSION 10");

    // Probe the new relations exist by querying them (empty result is fine).
    let probe_pubs = store.run_query("?[id] := *published_facts{id}", BTreeMap::new());
    assert!(
        probe_pubs.is_ok(),
        "published_facts relation must exist: {:?}",
        probe_pubs.err()
    );

    let probe_prov = store.run_query(
        "?[pid] := *provenance{published_fact_id: pid}",
        BTreeMap::new(),
    );
    assert!(
        probe_prov.is_ok(),
        "provenance relation must exist: {:?}",
        probe_prov.err()
    );
}
