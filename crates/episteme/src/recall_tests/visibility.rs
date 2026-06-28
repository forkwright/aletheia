//! Tests for visibility filtering in the recall pipeline.
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions: index access is guarded by prior length checks"
)]
#![expect(clippy::expect_used, reason = "test assertions")]

use crate::knowledge::{MemoryScope, Visibility};
use crate::recall::{FactorScores, ProjectRecallScope, ScoredResult};
use crate::recall::{filter_by_cohort_visibility, filter_by_project_scope, filter_by_visibility};

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
        scope: None,
        project_id: None,
    }
}

fn make_scored_with_project(project_id: Option<eidos::workspace::ProjectId>) -> ScoredResult {
    ScoredResult {
        project_id,
        ..make_scored_with_visibility("alice", Visibility::Private)
    }
}

fn make_scored_with_project_scope(
    source_id: &str,
    project_id: Option<eidos::workspace::ProjectId>,
    scope: Option<MemoryScope>,
) -> ScoredResult {
    ScoredResult {
        source_id: source_id.to_owned(),
        content: source_id.to_owned(),
        project_id,
        scope,
        ..make_scored_with_visibility("alice", Visibility::Private)
    }
}

#[test]
fn visibility_filter_keeps_private_for_owner() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Private)];
    let filtered = filter_by_cohort_visibility(candidates, "alice");
    assert_eq!(
        filtered.len(),
        1,
        "Private memory should be visible to its owner"
    );
}

#[test]
fn visibility_filter_drops_private_for_other() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Private)];
    let filtered = filter_by_cohort_visibility(candidates, "bob");
    assert!(
        filtered.is_empty(),
        "Private memory should be hidden from other nous"
    );
}

#[test]
fn visibility_filter_keeps_shared_for_anyone() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Shared)];
    let filtered = filter_by_cohort_visibility(candidates, "bob");
    assert_eq!(
        filtered.len(),
        1,
        "Shared memory should be visible to any nous"
    );
}

#[test]
fn visibility_filter_keeps_published_for_anyone() {
    let candidates = vec![make_scored_with_visibility("alice", Visibility::Published)];
    let filtered = filter_by_cohort_visibility(candidates, "bob");
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
        filter_by_cohort_visibility(own, "alice").len(),
        1,
        "Restricted memory should be visible to its owner"
    );

    let other = vec![make_scored_with_visibility("alice", Visibility::Restricted)];
    assert!(
        filter_by_cohort_visibility(other, "bob").is_empty(),
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
    let filtered = filter_by_cohort_visibility(candidates, "alice");
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
    let filtered: Vec<ScoredResult> = filter_by_cohort_visibility(vec![], "alice");
    assert!(
        filtered.is_empty(),
        "empty input should produce empty output"
    );
}

// ── filter_by_visibility (minimum-level) tests ───────────────────────────────

#[test]
fn min_visibility_private_keeps_all() {
    let candidates = vec![
        make_scored_with_visibility("alice", Visibility::Private),
        make_scored_with_visibility("alice", Visibility::Shared),
        make_scored_with_visibility("alice", Visibility::Published),
    ];
    let filtered = filter_by_visibility(candidates, Visibility::Private);
    assert_eq!(
        filtered.len(),
        3,
        "Private minimum should keep all visibilities"
    );
}

#[test]
fn min_visibility_shared_drops_private() {
    let candidates = vec![
        make_scored_with_visibility("alice", Visibility::Private),
        make_scored_with_visibility("alice", Visibility::Shared),
        make_scored_with_visibility("alice", Visibility::Published),
    ];
    let filtered = filter_by_visibility(candidates, Visibility::Shared);
    assert_eq!(filtered.len(), 2, "Shared minimum should drop Private");
    assert!(
        filtered.iter().all(|c| c.visibility >= Visibility::Shared),
        "all remaining should be Shared or higher"
    );
}

#[test]
fn min_visibility_published_keeps_only_published() {
    let candidates = vec![
        make_scored_with_visibility("alice", Visibility::Private),
        make_scored_with_visibility("alice", Visibility::Shared),
        make_scored_with_visibility("alice", Visibility::Restricted),
        make_scored_with_visibility("alice", Visibility::Published),
    ];
    let filtered = filter_by_visibility(candidates, Visibility::Published);
    assert_eq!(
        filtered.len(),
        1,
        "Published minimum should keep only Published"
    );
    assert_eq!(filtered[0].visibility, Visibility::Published);
}

#[test]
fn project_scope_keeps_current_project_and_global_results() {
    let project_alpha =
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/alpha.git")
            .expect("valid remote");
    let project_beta =
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/beta.git")
            .expect("valid remote");

    let candidates = vec![
        make_scored_with_project(Some(project_alpha.clone())),
        make_scored_with_project(Some(project_beta)),
        make_scored_with_project(None),
    ];

    let filtered = filter_by_project_scope(
        candidates,
        &ProjectRecallScope::Project(project_alpha.clone()),
    );

    assert_eq!(
        filtered.len(),
        2,
        "project-scoped recall should keep current project plus global facts"
    );
    assert!(
        filtered.iter().all(|result| result
            .project_id
            .as_ref()
            .is_none_or(|id| id == &project_alpha)),
        "other project facts should be excluded"
    );
}

#[test]
fn project_scope_excludes_other_project_and_malformed_project_rows() {
    let project_alpha =
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/alpha.git")
            .expect("valid remote");
    let project_beta =
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/beta.git")
            .expect("valid remote");

    let candidates = vec![
        make_scored_with_project_scope(
            "project-alpha",
            Some(project_alpha),
            Some(MemoryScope::Project),
        ),
        make_scored_with_project_scope(
            "project-beta",
            Some(project_beta.clone()),
            Some(MemoryScope::Project),
        ),
        make_scored_with_project_scope("global", None, None),
        make_scored_with_project_scope("malformed-project", None, Some(MemoryScope::Project)),
    ];

    let filtered = filter_by_project_scope(candidates, &ProjectRecallScope::Project(project_beta));

    let source_ids: Vec<&str> = filtered
        .iter()
        .map(|result| result.source_id.as_str())
        .collect();
    assert_eq!(
        source_ids,
        vec!["project-beta", "global"],
        "project B recall should keep project B and intentionally global rows only"
    );
}

#[test]
fn global_project_scope_keeps_all_projects() {
    let project_alpha =
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/alpha.git")
            .expect("valid remote");
    let project_beta =
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/beta.git")
            .expect("valid remote");

    let candidates = vec![
        make_scored_with_project(Some(project_alpha)),
        make_scored_with_project(Some(project_beta)),
        make_scored_with_project(None),
    ];

    let filtered = filter_by_project_scope(candidates, &ProjectRecallScope::Global);

    assert_eq!(
        filtered.len(),
        3,
        "global recall should be an explicit all-project read"
    );
}

#[test]
fn min_visibility_empty_candidates() {
    let filtered: Vec<ScoredResult> = filter_by_visibility(vec![], Visibility::Shared);
    assert!(
        filtered.is_empty(),
        "empty input should produce empty output"
    );
}
