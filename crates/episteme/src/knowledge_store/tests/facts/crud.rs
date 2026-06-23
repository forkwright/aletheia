//! Tests for basic fact CRUD: insert, retrieve, forget, access.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::collections::BTreeMap;

use super::super::super::*;
use crate::knowledge::ForgetReason;
use crate::test_fixtures::{make_fact, make_store};
#[test]
fn query_timeout_returns_typed_error() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: 4,
        ..Default::default()
    })
    .expect("open_mem");

    // WHY: Recursive transitive closure on a linear chain of N nodes requires N-1 semi-naive
    // fixpoint epochs. Each epoch checks the Poison flag. With N=2000 and timeout=50ms
    // the engine will hit the Poison kill well before all epochs complete.
    let result = store.run_query_with_timeout(
        r"
edge[a, b] := a in int_range(2000), b = a + 1
reach[a, b] := edge[a, b]
reach[a, c] := reach[a, b], edge[b, c]
?[a, c] := reach[a, c]
",
        BTreeMap::new(),
        Some(std::time::Duration::from_millis(50)),
    );

    assert!(result.is_err(), "expected timeout error");
    let err = result.expect_err("timeout query must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("timed out"),
        "error should mention timeout, got: {msg}"
    );
    assert!(
        matches!(err, crate::error::Error::QueryTimeout { .. }),
        "error type should be QueryTimeout"
    );
}

#[test]
fn query_killed_maps_to_typed_timeout() {
    // WHY: Construct the typed Krites error directly so the test is independent
    // of the `:timeout` poison mechanism and proves the mapping contract.
    let engine_err = crate::engine::error::QueryKilledSnafu.build();
    let err = KnowledgeStore::map_engine_err(
        engine_err,
        Some(std::time::Duration::from_millis(50)),
    );

    assert!(
        matches!(err, crate::error::Error::QueryTimeout { secs } if (secs - 0.05).abs() < f64::EPSILON),
        "QueryKilled should map to QueryTimeout with the configured duration"
    );
}

#[test]
fn query_killed_without_timeout_maps_to_zero_secs() {
    let engine_err = crate::engine::error::QueryKilledSnafu.build();
    let err = KnowledgeStore::map_engine_err(engine_err, None);

    assert!(
        matches!(err, crate::error::Error::QueryTimeout { secs } if secs == 0.0),
        "QueryKilled without a timeout should report 0.0s"
    );
}

#[test]
fn non_killed_engine_error_preserves_diagnostics() {
    let message = "deliberate engine failure".to_owned();
    let engine_err = crate::engine::error::EngineSnafu {
        message: message.clone(),
    }
    .build();
    let err = KnowledgeStore::map_engine_err(engine_err, Some(std::time::Duration::from_secs(1)));

    assert!(
        matches!(err, crate::error::Error::EngineQuery { message: m } if m == message),
        "non-QueryKilled engine errors should stay EngineQuery and keep their message"
    );
}

#[test]
fn query_without_timeout_succeeds() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: 4,
        ..Default::default()
    })
    .expect("open_mem");

    let result = store.run_query_with_timeout("?[x] := x = 42", BTreeMap::new(), None);

    assert!(result.is_ok(), "query without timeout should succeed");
    let rows = result.expect("query without timeout must succeed");
    assert_eq!(rows.row_count(), 1, "simple query should return one row");
}

#[test]
fn insert_fact_and_retrieve() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Rust is a systems programming language");
    store.insert_fact(&fact).expect("insert fact");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query facts");
    assert_eq!(
        results.len(),
        1,
        "should retrieve exactly one inserted fact"
    );
    assert_eq!(
        results[0].id.as_str(),
        "f1",
        "retrieved fact should have expected id"
    );
    assert_eq!(
        results[0].content, "Rust is a systems programming language",
        "retrieved fact should have expected content"
    );
    assert!(
        (results[0].provenance.confidence - 0.9).abs() < f64::EPSILON,
        "retrieved fact confidence should match inserted value"
    );
}

#[test]
fn insert_fact_with_scope_and_visibility_roundtrips() {
    let store = make_store();
    let mut fact = make_fact("f-scoped", "agent-a", "Scoped fact content");
    fact.scope = Some(crate::knowledge::MemoryScope::Project);
    fact.project_id = Some(
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/alpha.git")
            .expect("valid remote"),
    );
    fact.visibility = crate::knowledge::Visibility::Shared;
    store.insert_fact(&fact).expect("insert fact");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query facts");
    assert_eq!(results.len(), 1, "should retrieve exactly one fact");
    assert_eq!(
        results[0].scope,
        Some(crate::knowledge::MemoryScope::Project),
        "scope should roundtrip through storage"
    );
    assert_eq!(
        results[0].visibility,
        crate::knowledge::Visibility::Shared,
        "visibility should roundtrip through storage"
    );
    assert_eq!(
        results[0].project_id, fact.project_id,
        "project_id should roundtrip through storage"
    );
}

#[test]
fn insert_fact_with_each_sensitivity_roundtrips() {
    let store = make_store();
    for sensitivity in [
        crate::knowledge::FactSensitivity::Public,
        crate::knowledge::FactSensitivity::Internal,
        crate::knowledge::FactSensitivity::Confidential,
    ] {
        let mut fact = make_fact(
            &format!("f-{}", sensitivity.as_str()),
            "agent-a",
            &format!("{} sensitivity fact", sensitivity.as_str()),
        );
        fact.sensitivity = sensitivity;
        store.insert_fact(&fact).expect("insert fact");

        let results = store
            .read_facts_by_id(fact.id.as_str())
            .expect("read fact by id");
        assert_eq!(results.len(), 1, "should retrieve inserted fact");
        assert_eq!(
            results[0].sensitivity, sensitivity,
            "sensitivity should roundtrip through storage"
        );
    }
}

#[cfg(feature = "storage-fjall")]
#[test]
fn sensitivity_survives_fjall_reopen_for_each_level() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("knowledge.fjall").join("shared");

    {
        let store = KnowledgeStore::open_fjall(
            &path,
            KnowledgeConfig {
                dim: crate::test_fixtures::DIM,
                ..Default::default()
            },
        )
        .expect("open fjall store");
        for sensitivity in [
            crate::knowledge::FactSensitivity::Public,
            crate::knowledge::FactSensitivity::Internal,
            crate::knowledge::FactSensitivity::Confidential,
        ] {
            let mut fact = make_fact(
                &format!("f-reload-{}", sensitivity.as_str()),
                "agent-a",
                &format!("{} reload fact", sensitivity.as_str()),
            );
            fact.sensitivity = sensitivity;
            store.insert_fact(&fact).expect("insert fact");
        }
    }

    let reopened = KnowledgeStore::open_fjall(
        &path,
        KnowledgeConfig {
            dim: crate::test_fixtures::DIM,
            ..Default::default()
        },
    )
    .expect("reopen fjall store");

    for sensitivity in [
        crate::knowledge::FactSensitivity::Public,
        crate::knowledge::FactSensitivity::Internal,
        crate::knowledge::FactSensitivity::Confidential,
    ] {
        let facts = reopened
            .read_facts_by_id(&format!("f-reload-{}", sensitivity.as_str()))
            .expect("read fact after reopen");
        assert_eq!(facts.len(), 1, "reopened store should return fact");
        assert_eq!(
            facts[0].sensitivity, sensitivity,
            "sensitivity should survive store reopen"
        );
    }
}

#[test]
fn insert_multiple_facts_and_retrieve() {
    let store = make_store();
    for i in 0..5 {
        let fact = make_fact(&format!("f{i}"), "agent-a", &format!("Fact number {i}"));
        store.insert_fact(&fact).expect("insert fact");
    }

    let results = store
        .query_facts("agent-a", "2026-06-01", 100)
        .expect("query facts");
    assert_eq!(results.len(), 5, "should retrieve all five inserted facts");
}

#[test]
fn upsert_fact_overwrites() {
    let store = make_store();
    let mut fact = make_fact("f1", "agent-a", "Original content");
    store.insert_fact(&fact).expect("insert fact");

    fact.content = "Updated content".to_owned();
    fact.provenance.confidence = 0.95;
    store.insert_fact(&fact).expect("upsert fact");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query facts");
    assert_eq!(
        results.len(),
        1,
        "upsert should not create a duplicate fact"
    );
    assert_eq!(
        results[0].content, "Updated content",
        "upsert should overwrite content"
    );
    assert!(
        (results[0].provenance.confidence - 0.95).abs() < f64::EPSILON,
        "upsert should overwrite confidence"
    );
}

#[test]
fn forget_fact_excludes_from_query() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Secret fact");
    store.insert_fact(&fact).expect("insert fact");

    let forgotten = store
        .forget_fact(
            &crate::id::FactId::new("f1").expect("valid test id"),
            ForgetReason::UserRequested,
        )
        .expect("forget fact");
    assert!(
        forgotten.lifecycle.is_forgotten,
        "returned fact should be marked as forgotten"
    );
    assert_eq!(
        forgotten.lifecycle.forget_reason,
        Some(ForgetReason::UserRequested),
        "forget reason should be preserved"
    );
    assert!(
        forgotten.lifecycle.forgotten_at.is_some(),
        "forgotten_at timestamp should be set"
    );

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query facts");
    assert!(
        results.is_empty(),
        "forgotten fact must not appear in recall"
    );
}

#[test]
fn forget_fact_then_unforget_restores_recall() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Recoverable fact");
    store.insert_fact(&fact).expect("insert fact");

    store
        .forget_fact(
            &crate::id::FactId::new("f1").expect("valid test id"),
            ForgetReason::Outdated,
        )
        .expect("forget");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    assert!(results.is_empty(), "forgotten fact excluded from recall");

    let restored = store
        .unforget_fact(&crate::id::FactId::new("f1").expect("valid test id"))
        .expect("unforget");
    assert!(
        !restored.lifecycle.is_forgotten,
        "unforgotten fact should not be marked as forgotten"
    );
    assert!(
        restored.lifecycle.forgotten_at.is_none(),
        "unforgotten fact should have no forgotten_at timestamp"
    );
    assert!(
        restored.lifecycle.forget_reason.is_none(),
        "unforgotten fact should have no forget reason"
    );

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query after unforget");
    assert_eq!(results.len(), 1, "unforget must restore recall visibility");
    assert_eq!(
        results[0].id.as_str(),
        "f1",
        "the restored fact should have the original id"
    );
}

#[test]
fn forget_preserves_in_audit() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Auditable fact");
    store.insert_fact(&fact).expect("insert fact");

    store
        .forget_fact(
            &crate::id::FactId::new("f1").expect("valid test id"),
            ForgetReason::Privacy,
        )
        .expect("forget");

    let all = store.audit_all_facts("agent-a", 100).expect("audit all");
    let found = all.iter().find(|f| f.id.as_str() == "f1");
    assert!(found.is_some(), "audit must return forgotten facts");
    let found = found.expect("f1 must appear in audit after forget");
    assert!(
        found.lifecycle.is_forgotten,
        "audited fact should be marked as forgotten"
    );
    assert_eq!(
        found.lifecycle.forget_reason,
        Some(ForgetReason::Privacy),
        "audit should preserve forget reason"
    );
}

/// #4677/#4549: temporal, forgotten, and audit reads use distinct query
/// builders from the normal current-facts path. Each must hydrate the policy
/// fields (scope, `project_id`, visibility) of a project-scoped non-private fact.
/// A regression here silently widened shared/project facts to private/global.
#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_forgotten_audit_preserve_scope_project_visibility() {
    let store = make_store();
    let project = eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/alpha.git")
        .expect("valid remote");

    let mut live = make_fact("f-live", "agent-a", "Project-scoped shared fact");
    live.scope = Some(crate::knowledge::MemoryScope::Project);
    live.project_id = Some(project.clone());
    live.visibility = crate::knowledge::Visibility::Shared;
    store.insert_fact(&live).expect("insert live fact");

    let assert_policy = |fact: &crate::knowledge::Fact, ctx: &str| {
        assert_eq!(
            fact.scope,
            Some(crate::knowledge::MemoryScope::Project),
            "{ctx}: scope must hydrate"
        );
        assert_eq!(
            fact.project_id,
            Some(project.clone()),
            "{ctx}: project_id must hydrate"
        );
        assert_eq!(
            fact.visibility,
            crate::knowledge::Visibility::Shared,
            "{ctx}: visibility must hydrate, not default to private"
        );
    };

    let temporal = store
        .query_facts_temporal("agent-a", "2026-06-01", None)
        .expect("temporal query");
    assert_policy(
        temporal
            .iter()
            .find(|f| f.id.as_str() == "f-live")
            .expect("live fact in temporal read"),
        "temporal",
    );

    let audit = store.audit_all_facts("agent-a", 100).expect("audit query");
    assert_policy(
        audit
            .iter()
            .find(|f| f.id.as_str() == "f-live")
            .expect("live fact in audit read"),
        "audit",
    );

    store
        .forget_fact(
            &crate::id::FactId::new("f-live").expect("valid test id"),
            ForgetReason::Privacy,
        )
        .expect("forget");
    let forgotten = store
        .list_forgotten("agent-a", 100)
        .expect("list_forgotten");
    assert_policy(
        forgotten
            .iter()
            .find(|f| f.id.as_str() == "f-live")
            .expect("fact in forgotten read"),
        "forgotten",
    );
}

/// #4552: superseding a fact must preserve scope, project partitioning, and
/// visibility on both the closed old row and the new row, and must record the
/// supersession link. A regression here would drift the memory graph into
/// incorrect public/private or cross-project state when facts are replaced.
#[cfg(feature = "mneme-engine")]
#[test]
fn supersede_preserves_scope_project_visibility_and_links() {
    let store = make_store();
    let project = eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/beta.git")
        .expect("valid remote");

    let mut old_fact = make_fact("sup-old", "agent-a", "old shared fact");
    old_fact.scope = Some(crate::knowledge::MemoryScope::Project);
    old_fact.project_id = Some(project.clone());
    old_fact.visibility = crate::knowledge::Visibility::Shared;
    store.insert_fact(&old_fact).expect("insert old");

    let mut new_fact = make_fact("sup-new", "agent-a", "new shared fact");
    new_fact.scope = Some(crate::knowledge::MemoryScope::Project);
    new_fact.project_id = Some(project.clone());
    new_fact.visibility = crate::knowledge::Visibility::Shared;

    store
        .supersede_fact(&old_fact, &new_fact)
        .expect("supersede");

    let all = store.audit_all_facts("agent-a", 100).expect("audit");
    let old_row = all
        .iter()
        .find(|f| f.id.as_str() == "sup-old")
        .expect("old row in audit");
    let new_row = all
        .iter()
        .find(|f| f.id.as_str() == "sup-new")
        .expect("new row in audit");

    for (row, label) in [(old_row, "old"), (new_row, "new")] {
        assert_eq!(
            row.scope,
            Some(crate::knowledge::MemoryScope::Project),
            "{label} row scope must be preserved across supersession"
        );
        assert_eq!(
            row.project_id,
            Some(project.clone()),
            "{label} row project_id must be preserved across supersession"
        );
        assert_eq!(
            row.visibility,
            crate::knowledge::Visibility::Shared,
            "{label} row visibility must be preserved across supersession"
        );
    }

    assert_eq!(
        old_row.lifecycle.superseded_by.as_ref().map(AsRef::as_ref),
        Some("sup-new"),
        "old row must link to its successor"
    );
    assert!(
        new_row.lifecycle.superseded_by.is_none(),
        "new row must not be marked superseded"
    );
}

#[test]
fn forget_reason_roundtrips() {
    let store = make_store();

    let reasons = [
        ("f-ur", ForgetReason::UserRequested),
        ("f-od", ForgetReason::Outdated),
        ("f-ic", ForgetReason::Incorrect),
        ("f-pr", ForgetReason::Privacy),
    ];

    for (id, reason) in reasons {
        let fact = make_fact(id, "agent-a", &format!("fact for {reason}"));
        store.insert_fact(&fact).expect("insert");

        let forgotten = store
            .forget_fact(&crate::id::FactId::new(id).expect("valid test id"), reason)
            .expect("forget");
        assert_eq!(
            forgotten.lifecycle.forget_reason,
            Some(reason),
            "reason must round-trip for {reason}"
        );
    }

    let forgotten_list = store
        .list_forgotten("agent-a", 100)
        .expect("list_forgotten");
    assert_eq!(
        forgotten_list.len(),
        reasons.len(),
        "list_forgotten should return all forgotten facts"
    );
    for (id, reason) in reasons {
        let found = forgotten_list
            .iter()
            .find(|f| f.id.as_str() == id)
            .unwrap_or_else(|| panic!("missing {id} in list_forgotten"));
        assert_eq!(
            found.lifecycle.forget_reason,
            Some(reason),
            "forget reason should round-trip for {reason}"
        );
    }
}

#[test]
fn forget_nonexistent_fact_errors() {
    let store = make_store();
    let result = store.forget_fact(
        &crate::id::FactId::new("nonexistent").expect("valid test id"),
        ForgetReason::UserRequested,
    );
    assert!(result.is_err(), "forgetting non-existent fact must error");
    let err = result
        .expect_err("forgetting non-existent fact must error")
        .to_string();
    assert!(
        err.contains("not found"),
        "error should mention not found: {err}"
    );
}

#[test]
fn forget_excluded_from_temporal_diff() {
    let store = make_store();
    let fact = make_fact("f-diff", "agent-a", "Temporal diff fact");
    store.insert_fact(&fact).expect("insert");

    store
        .forget_fact(
            &crate::id::FactId::new("f-diff").expect("valid test id"),
            ForgetReason::Incorrect,
        )
        .expect("forget");

    let diff = store
        .query_facts_diff("agent-a", "2025-01-01", "2027-01-01")
        .expect("diff");
    assert!(
        !diff.added.iter().any(|f| f.id.as_str() == "f-diff"),
        "forgotten fact must not appear in temporal diff added"
    );
}

#[test]
fn increment_access_updates_count() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Accessed fact");
    store.insert_fact(&fact).expect("insert fact");

    store
        .increment_access(&[crate::id::FactId::new("f1").expect("valid test id")])
        .expect("increment");
    store
        .increment_access(&[crate::id::FactId::new("f1").expect("valid test id")])
        .expect("increment again");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    let found = results
        .iter()
        .find(|f| f.id.as_str() == "f1")
        .expect("found");
    assert_eq!(
        found.access.access_count, 2,
        "access count should reflect two increments"
    );
}

#[test]
fn increment_access_empty_ids_is_noop() {
    let store = make_store();
    store
        .increment_access(&[])
        .expect("empty increment should succeed");
}

#[test]
fn increment_access_nonexistent_id_is_silent() {
    let store = make_store();
    store
        .increment_access(&[crate::id::FactId::new("nonexistent").expect("valid test id")])
        .expect("increment nonexistent should not error");
}

#[test]
fn insert_fact_empty_content_rejected() {
    let store = make_store();
    let fact = make_fact("f-empty", "agent-a", "");
    let result = store.insert_fact(&fact);
    assert!(result.is_err(), "empty content must be rejected");
    assert!(
        matches!(
            result.expect_err("empty content must fail"),
            crate::error::Error::EmptyContent { .. }
        ),
        "error type should be EmptyContent"
    );
}

#[test]
fn insert_fact_confidence_out_of_range_rejected() {
    let store = make_store();

    let mut high = make_fact("f-high", "agent-a", "High confidence");
    high.provenance.confidence = 1.5;
    let result = store.insert_fact(&high);
    assert!(result.is_err(), "confidence > 1.0 must be rejected");
    assert!(
        matches!(
            result.expect_err("confidence > 1.0 must fail"),
            crate::error::Error::InvalidConfidence { .. }
        ),
        "error type should be InvalidConfidence for confidence > 1.0"
    );

    let mut negative = make_fact("f-neg", "agent-a", "Negative confidence");
    negative.provenance.confidence = -0.5;
    let result = store.insert_fact(&negative);
    assert!(result.is_err(), "confidence < 0.0 must be rejected");
    assert!(
        matches!(
            result.expect_err("confidence < 0.0 must fail"),
            crate::error::Error::InvalidConfidence { .. }
        ),
        "error type should be InvalidConfidence for confidence < 0.0"
    );
}

#[test]
fn schema_version_returns_current() {
    let store = make_store();
    let version = store.schema_version().expect("schema version");
    assert_eq!(
        version,
        KnowledgeStore::SCHEMA_VERSION,
        "schema version should match current constant"
    );
}

#[test]
fn query_facts_filters_by_nous_id() {
    let store = make_store();
    store
        .insert_fact(&make_fact("f1", "agent-a", "Fact for A"))
        .expect("insert f1");
    store
        .insert_fact(&make_fact("f2", "agent-b", "Fact for B"))
        .expect("insert f2");
    store
        .insert_fact(&make_fact("f3", "agent-a", "Another fact for A"))
        .expect("insert f3");

    let results_a = store
        .query_facts("agent-a", "2026-06-01", 100)
        .expect("query agent-a");
    assert_eq!(results_a.len(), 2, "agent-a should have exactly two facts");
    assert!(
        results_a.iter().all(|f| f.nous_id == "agent-a"),
        "all agent-a results should have correct nous_id"
    );

    let results_b = store
        .query_facts("agent-b", "2026-06-01", 100)
        .expect("query agent-b");
    assert_eq!(results_b.len(), 1, "agent-b should have exactly one fact");
    assert_eq!(
        results_b[0].id.as_str(),
        "f2",
        "agent-b's fact should have id f2"
    );
}

#[test]
fn query_facts_respects_limit() {
    let store = make_store();
    for i in 0..20 {
        store
            .insert_fact(&make_fact(
                &format!("f{i}"),
                "agent-a",
                &format!("Fact {i}"),
            ))
            .expect("insert");
    }

    let results = store
        .query_facts("agent-a", "2026-06-01", 5)
        .expect("query with limit");
    assert_eq!(
        results.len(),
        5,
        "query with limit 5 should return exactly 5 results"
    );
}
