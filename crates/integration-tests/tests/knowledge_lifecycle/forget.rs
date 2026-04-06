//! Forget, unforget, supersession, and forget lifecycle integration tests.
use super::*;

#[test]
fn supersession_chain() {
    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-09-01T00:00:00Z";

    // v1: original fact
    let v1 = make_fact(
        "v1",
        nous,
        "Project uses Python",
        0.8,
        EpistemicTier::Inferred,
    );
    store.insert_fact(&v1).expect("insert v1");

    // v1 -> v2: first correction
    correct_fact(
        &store,
        "v1",
        "v2",
        "Project uses Python and Rust",
        nous,
        "2026-04-01T00:00:00Z",
    );

    // v2 -> v3: second correction
    correct_fact(
        &store,
        "v2",
        "v3",
        "Project migrated fully to Rust",
        nous,
        "2026-07-01T00:00:00Z",
    );

    // Only v3 visible in query
    let results = store.query_facts(nous, query_time, 10).expect("query");
    assert_eq!(results.len(), 1, "only latest fact should be visible");
    assert_eq!(results[0].id.as_str(), "v3");
    assert_eq!(results[0].content, "Project migrated fully to Rust");

    // Audit shows full chain
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 3, "audit should show all 3 versions");

    let a_v1 = audit.iter().find(|r| r.id == "v1").expect("v1 in audit");
    let a_v2 = audit.iter().find(|r| r.id == "v2").expect("v2 in audit");
    let a_v3 = audit.iter().find(|r| r.id == "v3").expect("v3 in audit");

    // Supersession chain: v1 -> v2 -> v3
    assert_eq!(
        a_v1.superseded_by.as_deref(),
        Some("v2"),
        "v1 superseded by v2"
    );
    assert_eq!(
        a_v2.superseded_by.as_deref(),
        Some("v3"),
        "v2 superseded by v3"
    );
    assert!(
        a_v3.superseded_by.is_none(),
        "v3 is current — not superseded"
    );

    // Temporal validity: each version expired when the next was created
    assert_eq!(
        a_v1.valid_to, "2026-04-01T00:00:00Z",
        "v1 expired at v2 creation"
    );
    assert_eq!(
        a_v2.valid_to, "2026-07-01T00:00:00Z",
        "v2 expired at v3 creation"
    );
    assert_eq!(a_v3.valid_to, "9999-01-01T00:00:00Z", "v3 still current");
}

// --- Forget lifecycle tests ---

#[test]
fn forget_excludes_from_recall() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    let fact = make_fact(
        "f-forget",
        nous,
        "Sensitive credential: token-abc-12345",
        0.9,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    // Visible before forget
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query before forget");
    assert_eq!(results.len(), 1);

    let fid = FactId::new("f-forget").expect("valid test id");
    store
        .forget_fact(&fid, ForgetReason::Privacy)
        .expect("forget");

    // Not visible after forget
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after forget");
    assert!(
        results.is_empty(),
        "forgotten fact should be excluded from recall"
    );
}

#[test]
fn forget_preserves_for_audit() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";

    let fact = make_fact(
        "f-audit",
        nous,
        "sensitive data",
        0.9,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    let fid = FactId::new("f-audit").expect("valid test id");
    store
        .forget_fact(&fid, ForgetReason::Privacy)
        .expect("forget");

    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 1, "forgotten fact should appear in audit");

    let row = &audit[0];
    assert!(row.is_forgotten, "should be marked forgotten");
    assert!(
        row.forgotten_at.is_some(),
        "should have forgotten_at timestamp"
    );
    assert_eq!(
        row.forget_reason.as_deref(),
        Some("privacy"),
        "should have privacy reason"
    );
}

#[test]
fn unforget_restores_to_search() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    let fact = make_fact(
        "f-unforget",
        nous,
        "reinstated fact",
        0.9,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    let fid = FactId::new("f-unforget").expect("valid test id");
    store
        .forget_fact(&fid, ForgetReason::Outdated)
        .expect("forget");

    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after forget");
    assert!(results.is_empty(), "should be excluded after forget");

    store.unforget_fact(&fid).expect("unforget");

    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after unforget");
    assert_eq!(results.len(), 1, "should be restored after unforget");
    assert_eq!(results[0].id.as_str(), "f-unforget");

    // Audit should show cleared forget metadata
    let audit = audit_all_facts(&store, nous);
    let row = &audit[0];
    assert!(
        !row.is_forgotten,
        "should not be marked forgotten after unforget"
    );
    assert!(row.forgotten_at.is_none(), "forgotten_at should be cleared");
    assert!(
        row.forget_reason.is_none(),
        "forget_reason should be cleared"
    );
}

#[test]
fn forget_with_each_reason() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";

    for (i, (reason, reason_str)) in [
        (ForgetReason::UserRequested, "user_requested"),
        (ForgetReason::Outdated, "outdated"),
        (ForgetReason::Incorrect, "incorrect"),
        (ForgetReason::Privacy, "privacy"),
    ]
    .iter()
    .enumerate()
    {
        let id = format!("f-reason-{i}");
        let fact = make_fact(
            &id,
            nous,
            &format!("fact for {reason_str}"),
            0.9,
            EpistemicTier::Verified,
        );
        store.insert_fact(&fact).expect("insert");
        let fid = FactId::new(&id).expect("valid test id");
        store.forget_fact(&fid, *reason).expect("forget");
    }

    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 4);
    for (i, reason_str) in ["user_requested", "outdated", "incorrect", "privacy"]
        .iter()
        .enumerate()
    {
        let row = audit
            .iter()
            .find(|r| r.id == format!("f-reason-{i}"))
            .expect("find fact");
        assert!(row.is_forgotten);
        assert_eq!(row.forget_reason.as_deref(), Some(*reason_str));
    }
}

#[test]
fn full_forget_lifecycle() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    // 1. Insert
    let fact = make_fact(
        "f-lifecycle",
        nous,
        "The user stores a private note here",
        0.95,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    // 2. Search: found
    let results = store.query_facts(nous, query_time, 10).expect("query");
    assert_eq!(results.len(), 1);

    // 3. Forget: privacy
    let fid = FactId::new("f-lifecycle").expect("valid test id");
    store
        .forget_fact(&fid, ForgetReason::Privacy)
        .expect("forget");

    // 4. Search: not found
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after forget");
    assert!(results.is_empty());

    // 5. Audit: found with metadata
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 1);
    assert!(audit[0].is_forgotten);
    assert_eq!(audit[0].forget_reason.as_deref(), Some("privacy"));

    // 6. Unforget
    store.unforget_fact(&fid).expect("unforget");

    // 7. Search: found again
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after unforget");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "The user stores a private note here");
}
