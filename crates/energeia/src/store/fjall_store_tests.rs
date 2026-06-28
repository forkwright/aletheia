#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
#![expect(clippy::float_cmp, reason = "test assertions on exact float values")]

use tempfile::TempDir;

use super::*;

fn setup_test_store() -> (TempDir, EnergeiaStore) {
    let temp_dir = TempDir::new().unwrap();
    let db = fjall::Database::builder(temp_dir.path()).open().unwrap();
    let store = EnergeiaStore::new(&db).unwrap();
    (temp_dir, store)
}

fn sample_dispatch_spec() -> DispatchSpec {
    DispatchSpec {
        prompt_numbers: vec![1, 2, 3],
        project: "acme".to_owned(),
        dag_ref: None,
        max_parallel: Some(2),
        max_turns: None,
        budget_usd: None,
    }
}

fn ulid_for_millisecond(ms: u64) -> String {
    koina::ulid::Ulid::from_u128(u128::from(ms) << 80).to_string()
}

fn timestamp_ms(ms: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_millisecond(ms).unwrap()
}

// ── Dispatch tests ──

#[test]
fn create_and_get_dispatch() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();

    let id = store.create_dispatch("acme", &spec).unwrap();
    let record = store.get_dispatch(&id).unwrap().unwrap();

    assert_eq!(record.project, "acme");
    assert_eq!(record.status, DispatchStatus::Running);
    assert_eq!(record.total_cost_usd, 0.0);
    assert!(record.finished_at.is_none());
}

#[test]
fn get_nonexistent_dispatch_returns_none() {
    let (_dir, store) = setup_test_store();
    let id = DispatchId::new("01NONEXISTENT");
    assert!(store.get_dispatch(&id).unwrap().is_none());
}

#[test]
fn finish_dispatch_aggregates_sessions() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();
    let dispatch_id = store.create_dispatch("acme", &spec).unwrap();

    let sess1 = store.create_session(&dispatch_id, 1).unwrap();
    let sess2 = store.create_session(&dispatch_id, 2).unwrap();

    store
        .update_session(
            &sess1,
            SessionUpdate {
                status: Some(SessionStatus::Success),
                cost_usd: Some(1.50),
                num_turns: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .update_session(
            &sess2,
            SessionUpdate {
                status: Some(SessionStatus::Success),
                cost_usd: Some(2.25),
                num_turns: Some(8),
                ..Default::default()
            },
        )
        .unwrap();

    store
        .finish_dispatch(&dispatch_id, DispatchStatus::Completed)
        .unwrap();

    let record = store.get_dispatch(&dispatch_id).unwrap().unwrap();
    assert_eq!(record.status, DispatchStatus::Completed);
    assert!(record.finished_at.is_some());
    assert!((record.total_cost_usd - 3.75).abs() < 0.01);
    assert_eq!(record.total_sessions, 2);
}

#[test]
fn finish_nonexistent_dispatch_returns_not_found() {
    let (_dir, store) = setup_test_store();
    let id = DispatchId::new("01NONEXISTENT");
    let result = store.finish_dispatch(&id, DispatchStatus::Failed);
    assert!(result.is_err());
}

#[test]
fn list_dispatches_since_starts_at_cutoff_ulid() {
    let (_dir, store) = setup_test_store();
    let old = DispatchRecord {
        id: DispatchId::new(ulid_for_millisecond(1_000)),
        project: "acme".to_owned(),
        spec: "{}".to_owned(),
        status: DispatchStatus::Completed,
        created_at: timestamp_ms(1_000),
        finished_at: Some(timestamp_ms(1_100)),
        total_cost_usd: 0.0,
        total_sessions: 0,
    };
    let new = DispatchRecord {
        id: DispatchId::new(ulid_for_millisecond(3_000)),
        project: "acme".to_owned(),
        spec: "{}".to_owned(),
        status: DispatchStatus::Completed,
        created_at: timestamp_ms(3_000),
        finished_at: Some(timestamp_ms(3_100)),
        total_cost_usd: 0.0,
        total_sessions: 0,
    };
    store.insert_dispatch_record_for_test(&old).unwrap();
    store.insert_dispatch_record_for_test(&new).unwrap();

    let dispatches = store.list_dispatches_since(Some(2_000), 100).unwrap();

    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].id, new.id);
}

// ── Session tests ──

#[test]
fn create_and_list_sessions() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();
    let dispatch_id = store.create_dispatch("acme", &spec).unwrap();

    store.create_session(&dispatch_id, 1).unwrap();
    store.create_session(&dispatch_id, 2).unwrap();
    store.create_session(&dispatch_id, 3).unwrap();

    let sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0].prompt_number, 1);
    assert_eq!(sessions[1].prompt_number, 2);
    assert_eq!(sessions[2].prompt_number, 3);
}

#[test]
fn sessions_isolated_between_dispatches() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();

    let d1 = store.create_dispatch("project-a", &spec).unwrap();
    let d2 = store.create_dispatch("project-b", &spec).unwrap();

    store.create_session(&d1, 1).unwrap();
    store.create_session(&d1, 2).unwrap();
    store.create_session(&d2, 1).unwrap();

    assert_eq!(store.list_sessions_for_dispatch(&d1).unwrap().len(), 2);
    assert_eq!(store.list_sessions_for_dispatch(&d2).unwrap().len(), 1);
}

#[test]
fn update_session_partial() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();
    let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
    let session_id = store.create_session(&dispatch_id, 1).unwrap();

    store
        .update_session(
            &session_id,
            SessionUpdate {
                status: Some(SessionStatus::Success),
                cost_usd: Some(0.42),
                pr_url: Some("https://github.com/acme/repo/pull/7".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();

    let sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
    let session = &sessions[0];
    assert_eq!(session.status, SessionStatus::Success);
    assert_eq!(session.cost_usd, 0.42);
    assert_eq!(
        session.pr_url.as_deref(),
        Some("https://github.com/acme/repo/pull/7")
    );
    assert_eq!(session.num_turns, 0);
}

#[test]
fn update_session_uses_reverse_index_not_prefix_scan() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();
    let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
    let session_id = store.create_session(&dispatch_id, 42).unwrap();
    let mut decoy = store.list_sessions_for_dispatch(&dispatch_id).unwrap()[0].clone();
    let decoy_dispatch_id = DispatchId::new("00000000000000000000000000");
    decoy.dispatch_id = decoy_dispatch_id.clone();
    decoy.prompt_number = 1;
    decoy.status = SessionStatus::Skipped;
    let decoy_key = schema::session_key(&decoy_dispatch_id, decoy.prompt_number);
    let decoy_value = serialize_msgpack(&decoy, "decoy session").unwrap();
    store
        .keyspace
        .insert(decoy_key.as_bytes(), decoy_value)
        .unwrap();

    store
        .update_session(
            &session_id,
            SessionUpdate {
                status: Some(SessionStatus::Success),
                cost_usd: Some(3.14),
                ..Default::default()
            },
        )
        .unwrap();

    let target_sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
    assert_eq!(target_sessions[0].status, SessionStatus::Success);
    assert_eq!(target_sessions[0].cost_usd, 3.14);

    let decoy_sessions =
        queries::list_sessions_for_dispatch(&store.keyspace, &decoy_dispatch_id).unwrap();
    assert_eq!(decoy_sessions[0].status, SessionStatus::Skipped);
    assert_eq!(decoy_sessions[0].cost_usd, 0.0);
}

#[test]
fn update_nonexistent_session_returns_not_found() {
    let (_dir, store) = setup_test_store();
    let id = SessionId::new("01NONEXISTENT");
    let result = store.update_session(&id, SessionUpdate::default());
    assert!(result.is_err());
}

#[test]
fn list_sessions_since_filters_outside_window() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();
    let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
    let old_id = store.create_session(&dispatch_id, 1).unwrap();
    let new_id = store.create_session(&dispatch_id, 2).unwrap();
    let mut sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
    sessions.sort_by_key(|session| session.prompt_number);
    sessions[0].created_at = timestamp_ms(1_000);
    sessions[0].updated_at = timestamp_ms(1_000);
    sessions[1].created_at = timestamp_ms(3_000);
    sessions[1].updated_at = timestamp_ms(3_000);
    for session in &sessions {
        let key = schema::session_key(&session.dispatch_id, session.prompt_number);
        let value = serialize_msgpack(session, "windowed session").unwrap();
        store.keyspace.insert(key.as_bytes(), value).unwrap();
    }

    let filtered = store.list_all_sessions_since(Some(2_000), 100).unwrap();

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, new_id);
    assert_ne!(filtered[0].id, old_id);
}

// ── Lesson tests ──

#[test]
fn add_and_query_lessons() {
    let (_dir, store) = setup_test_store();

    store
        .add_lesson(&NewLesson {
            source: "steward".to_owned(),
            category: "testing".to_owned(),
            lesson: "Always check clippy".to_owned(),
            evidence: None,
            project: Some("acme".to_owned()),
            prompt_number: Some(1),
        })
        .unwrap();

    store
        .add_lesson(&NewLesson {
            source: "qa".to_owned(),
            category: "style".to_owned(),
            lesson: "Use snafu not thiserror".to_owned(),
            evidence: Some("RUST.md".to_owned()),
            project: Some("acme".to_owned()),
            prompt_number: None,
        })
        .unwrap();

    let all = store.query_lessons(None, None, None, 100).unwrap();
    assert_eq!(all.len(), 2);

    let by_source = store
        .query_lessons(Some("steward"), None, None, 100)
        .unwrap();
    assert_eq!(by_source.len(), 1);
    assert_eq!(by_source[0].lesson, "Always check clippy");

    let by_category = store.query_lessons(None, Some("style"), None, 100).unwrap();
    assert_eq!(by_category.len(), 1);

    let by_project = store.query_lessons(None, None, Some("acme"), 100).unwrap();
    assert_eq!(by_project.len(), 2);
}

#[test]
fn query_lessons_respects_limit() {
    let (_dir, store) = setup_test_store();
    for i in 0..5 {
        store
            .add_lesson(&NewLesson {
                source: "steward".to_owned(),
                category: "testing".to_owned(),
                lesson: format!("Lesson {i}"),
                evidence: None,
                project: None,
                prompt_number: None,
            })
            .unwrap();
    }
    let results = store.query_lessons(None, None, None, 3).unwrap();
    assert_eq!(results.len(), 3);
}

// ── Observation tests ──

#[test]
fn add_and_query_observations() {
    let (_dir, store) = setup_test_store();

    store
        .add_observation(&NewObservation {
            project: "acme".to_owned(),
            source: "qa".to_owned(),
            content: "Flaky test in auth module".to_owned(),
            observation_type: "bug".to_owned(),
            session_id: None,
        })
        .unwrap();

    store
        .add_observation(&NewObservation {
            project: "other".to_owned(),
            source: "steward".to_owned(),
            content: "Missing docs".to_owned(),
            observation_type: "doc_gap".to_owned(),
            session_id: None,
        })
        .unwrap();

    let all = store.query_observations(None, None, 100).unwrap();
    assert_eq!(all.len(), 2);

    let acme_only = store.query_observations(Some("acme"), None, 100).unwrap();
    assert_eq!(acme_only.len(), 1);
    assert_eq!(acme_only[0].content, "Flaky test in auth module");
}

#[test]
fn query_observations_respects_limit() {
    let (_dir, store) = setup_test_store();
    for i in 0..5 {
        store
            .add_observation(&NewObservation {
                project: "acme".to_owned(),
                source: "qa".to_owned(),
                content: format!("Observation {i}"),
                observation_type: "idea".to_owned(),
                session_id: None,
            })
            .unwrap();
    }
    let results = store.query_observations(None, None, 3).unwrap();
    assert_eq!(results.len(), 3);
}

// ── CI Validation tests ──

#[test]
fn add_and_list_ci_validations() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();
    let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
    let session_id = store.create_session(&dispatch_id, 1).unwrap();

    store
        .add_ci_validation(&session_id, "clippy", 42, CiValidationStatus::Pass, None)
        .unwrap();

    store
        .add_ci_validation(
            &session_id,
            "tests",
            42,
            CiValidationStatus::Fail,
            Some("3 tests failed".to_owned()),
        )
        .unwrap();

    let validations =
        queries::list_ci_validations_for_session(&store.keyspace, &session_id).unwrap();
    assert_eq!(validations.len(), 2);
}

// ── Training data tests ──

#[test]
fn record_training_data_produces_fact() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();
    let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
    let session_id = store.create_session(&dispatch_id, 1).unwrap();

    let sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
    let session = &sessions[0];

    let outcome = SessionOutcome {
        prompt_number: 1,
        status: SessionStatus::Success,
        session_id: Some("cc-sess-abc".to_owned()),
        cost_usd: 0.42,
        num_turns: 15,
        duration_ms: 30_000,
        resume_count: 0,
        pr_url: Some("https://github.com/acme/repo/pull/42".to_owned()),
        error: None,
        model: Some("claude-3-5-sonnet".to_owned()),
        blast_radius: vec!["crates/test/".to_owned()],
        corrective_attempts: 0,
        cache_hit_tokens: 0,
        cache_miss_tokens: 0,
        structured_output: None,
    };

    let fact = store.record_training_data(session, &outcome).unwrap();

    assert_eq!(fact.fact_type, "training");
    assert_eq!(fact.provenance.tier, EpistemicTier::Training);
    assert_eq!(fact.provenance.confidence, 1.0);
    assert!(fact.id.as_str().starts_with("training:"));
    assert_eq!(
        fact.id.as_str(),
        format!("training:{}", session_id.as_str())
    );

    let data: SessionOutcomeData = serde_json::from_str(&fact.content).unwrap();
    assert_eq!(data.prompt_number, 1);
    assert_eq!(data.cost_usd, 0.42);
}

// ── Store construction tests ──

#[test]
fn store_roundtrip_across_reopen() {
    let temp_dir = TempDir::new().unwrap();
    let spec = sample_dispatch_spec();
    let dispatch_id = {
        let db = fjall::Database::builder(temp_dir.path()).open().unwrap();
        let store = EnergeiaStore::new(&db).unwrap();
        let id = store.create_dispatch("acme", &spec).unwrap();
        assert!(store.get_dispatch(&id).unwrap().is_some());
        id
    };
    // Reopen the database and verify the record survived.
    let db2 = fjall::Database::builder(temp_dir.path()).open().unwrap();
    let store2 = EnergeiaStore::new(&db2).unwrap();
    assert!(
        store2.get_dispatch(&dispatch_id).unwrap().is_some(),
        "dispatch record must survive database reopen"
    );
}

#[test]
fn debug_format() {
    let temp_dir = TempDir::new().unwrap();
    let db = fjall::Database::builder(temp_dir.path()).open().unwrap();
    let store = EnergeiaStore::new(&db).unwrap();
    let debug = format!("{store:?}");
    assert!(debug.contains("energeia"));
}

#[test]
fn store_is_send_sync() {
    const _: fn() = || {
        fn assert<T: Send + Sync>() {}
        assert::<EnergeiaStore>();
    };
}

// ── Stale running dispatch reconciliation ──

#[test]
fn reconcile_stale_running_dispatches_marks_old_as_failed() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();

    let stale_id = store.create_dispatch("acme", &spec).unwrap();
    let fresh_id = store.create_dispatch("acme", &spec).unwrap();
    let done_id = store.create_dispatch("acme", &spec).unwrap();

    store
        .backdate_dispatch_for_test(&stale_id, jiff::SignedDuration::from_hours(2))
        .unwrap();
    store
        .finish_dispatch(&done_id, DispatchStatus::Completed)
        .unwrap();

    let threshold = jiff::SignedDuration::from_hours(1);
    let count = store.reconcile_stale_running_dispatches(threshold).unwrap();
    assert_eq!(
        count, 1,
        "exactly one stale Running dispatch should be reconciled"
    );

    let stale = store.get_dispatch(&stale_id).unwrap().unwrap();
    assert_eq!(
        stale.status,
        DispatchStatus::Failed,
        "stale dispatch must become Failed"
    );

    let fresh = store.get_dispatch(&fresh_id).unwrap().unwrap();
    assert_eq!(
        fresh.status,
        DispatchStatus::Running,
        "recent dispatch must stay Running"
    );

    let done = store.get_dispatch(&done_id).unwrap().unwrap();
    assert_eq!(
        done.status,
        DispatchStatus::Completed,
        "completed dispatch must be unchanged"
    );
}

#[test]
fn stale_running_dispatch_count_does_not_mutate() {
    let (_dir, store) = setup_test_store();
    let spec = sample_dispatch_spec();

    let id = store.create_dispatch("acme", &spec).unwrap();
    store
        .backdate_dispatch_for_test(&id, jiff::SignedDuration::from_hours(3))
        .unwrap();

    let threshold = jiff::SignedDuration::from_hours(1);
    let count = store.stale_running_dispatch_count(threshold).unwrap();
    assert_eq!(count, 1);

    let after = store.get_dispatch(&id).unwrap().unwrap();
    assert_eq!(
        after.status,
        DispatchStatus::Running,
        "count must not mutate records"
    );
}
