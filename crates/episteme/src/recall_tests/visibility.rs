//! Tests for visibility filtering in the recall pipeline.

use crate::knowledge::Visibility;
use crate::recall::filter_by_visibility;
use crate::recall::{FactorScores, ScoredResult};

fn make_scored_with_visibility(nous_id: &str, visibility: Visibility) -> ScoredResult {
    ScoredResult {
        content: "test".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "f1".to_owned(),
        nous_id: nous_id.to_owned(),
        factors: FactorScores::default(),
        score: 0.0,
        sensitivity: crate::knowledge::FactSensitivity::Public,
        visibility,
    }
}

#[test]
fn visibility_filter_keeps_private_for_owner() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Private)];
    let filtered = filter_by_visibility(candidates, "alice");
    assert_eq!(
        filtered.len(),
        1,
        "Private memory should be visible to its owner"
    );
}

#[test]
fn visibility_filter_drops_private_for_other() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Private)];
    let filtered = filter_by_visibility(candidates, "bob");
    assert!(
        filtered.is_empty(),
        "Private memory should be hidden from other nous"
    );
}

#[test]
fn visibility_filter_keeps_shared_for_anyone() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Shared)];
    let filtered = filter_by_visibility(candidates, "bob");
    assert_eq!(
        filtered.len(),
        1,
        "Shared memory should be visible to any nous"
    );
}

#[test]
fn visibility_filter_keeps_published_for_anyone() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Published)];
    let filtered = filter_by_visibility(candidates, "bob");
    assert_eq!(
        filtered.len(),
        1,
        "Published memory should be visible to any nous"
    );
}

#[test]
fn visibility_filter_restricted_treated_as_private() {
    // NOTE: Until an access-list model exists, Restricted is retained only
    // for the owning nous.
    let own = vec![make_scored_with_visibility("alice", Visibility::Restricted)];
    assert_eq!(
        filter_by_visibility(own, "alice").len(),
        1,
        "Restricted memory should be visible to its owner"
    );

    let other = vec![make_scored_with_visibility("alice", Visibility::Restricted)];
    assert!(
        filter_by_visibility(other, "bob").is_empty(),
        "Restricted memory should be hidden from other nous until access-list lands"
    );
}

#[test]
fn visibility_filter_mixed_set() {
    let candidates = vec![
        make_scored_with_visibility("alice", Visibility::Private),
        make_scored_with_visibility("alice", Visibility::Shared),
        make_scored_with_visibility("bob", Visibility::Private),
        make_scored_with_visibility("alice", Visibility::Published),
    ];
    let filtered = filter_by_visibility(candidates, "alice");
    assert_eq!(
        filtered.len(),
        3,
        "alice should see her Private, Shared, and Published results"
    );
    assert!(
        filtered
            .iter()
            .all(|c| c.nous_id == "alice" || c.visibility != Visibility::Private),
        "no private results from other nous should leak"
    );
}

#[test]
fn visibility_filter_empty_candidates() {
    let filtered: Vec<ScoredResult> = filter_by_visibility(vec![], "alice");
    assert!(
        filtered.is_empty(),
        "empty input should produce empty output"
    );
}
