#[test]
fn now_iso_is_utc() {
    let ts = super::super::now_iso();
    assert!(
        ts.ends_with('Z'),
        "canonical store timestamp must end with Z: {ts}"
    );
    let parsed: jiff::Timestamp = ts.parse().expect("timestamp parses as UTC");
    let now = jiff::Timestamp::now();
    let diff = (now.as_millisecond() - parsed.as_millisecond()).unsigned_abs();
    assert!(
        diff < 2000,
        "timestamp should be within 2s of now, got {ts}"
    );
}

#[test]
fn now_iso_ignores_host_timezone() {
    // WHY: `Zoned::now()` reads `TZ`, but `Timestamp::now()` does not. The
    // previous helper mixed the two and stamped local wall time with a `Z`
    // suffix (#4742).
    #[expect(
        unsafe_code,
        reason = "std::env::set_var requires unsafe in edition 2024; single-threaded test process"
    )]
    // SAFETY: `set_var` is only used in this single-threaded test process.
    unsafe {
        std::env::set_var("TZ", "Pacific/Auckland");
    }
    let ts = super::super::now_iso();
    let parsed: jiff::Timestamp = ts.parse().expect("timestamp parses as UTC");
    let now = jiff::Timestamp::now();
    let diff = (now.as_millisecond() - parsed.as_millisecond()).unsigned_abs();
    assert!(
        diff < 2000,
        "now_iso must equal UTC now regardless of TZ, got {ts}"
    );
}

#[cfg(feature = "portability")]
#[test]
fn session_listing_order_is_utc_stable() {
    use crate::test_fixtures::test_store;

    let store = test_store();
    let older = store
        .import_session(
            &crate::types::Session {
                id: "ses-older".to_owned(),
                nous_id: "alice".to_owned(),
                session_key: "older".to_owned(),
                status: crate::types::SessionStatus::Active,
                model: None,
                session_type: crate::types::SessionType::Primary,
                created_at: "2026-01-01T00:00:00.000Z".to_owned(),
                updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
                metrics: crate::types::SessionMetrics {
                    token_count_estimate: 0,
                    message_count: 0,
                    last_input_tokens: 0,
                    bootstrap_hash: None,
                    distillation_count: 0,
                    last_distilled_at: None,
                    computed_context_tokens: 0,
                },
                origin: crate::types::SessionOrigin {
                    parent_session_id: None,
                    thread_id: None,
                    transport: None,
                    display_name: None,
                },
                artefact_meta: None,
            },
            false,
        )
        .expect("import older");
    let newer = store
        .import_session(
            &crate::types::Session {
                id: "ses-newer".to_owned(),
                nous_id: "alice".to_owned(),
                session_key: "newer".to_owned(),
                status: crate::types::SessionStatus::Active,
                model: None,
                session_type: crate::types::SessionType::Primary,
                created_at: "2026-06-01T00:00:00.000Z".to_owned(),
                updated_at: "2026-06-01T00:00:00.000Z".to_owned(),
                metrics: older.metrics.clone(),
                origin: older.origin.clone(),
                artefact_meta: None,
            },
            false,
        )
        .expect("import newer");

    let all = store.list_sessions(None).expect("list all");
    let ids: Vec<_> = all.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["ses-newer", "ses-older"], "newer must come first");
}
