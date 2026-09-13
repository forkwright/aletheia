//! INVARIANT: `GET /api/v1/system/status`'s collector bounds every
//! subsystem check individually (`SUBSYSTEM_CHECK_TIMEOUT`) and runs the
//! hang-prone ones concurrently, so one stuck subsystem reports a typed
//! `"timeout"` record while every sibling subsystem still reports its real
//! status — the response never depends on the hung subsystem resolving.
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use tower::ServiceExt;

use super::helpers::*;

#[tokio::test]
async fn system_status_reports_typed_timeout_for_a_hung_subsystem() {
    let (state, _dir) = test_state().await;
    let app = build_router(Arc::clone(&state), &test_security_config());

    // WHY: never released for the life of this test -- simulates
    // `session_store` genuinely hanging rather than merely being slow.
    let _held_lock = state.session_store.lock().await;

    // WHY: race the request against a bound well above the collector's own
    // per-subsystem timeout, rather than measuring elapsed wall-clock —
    // the structural property under test is "the response does not wait
    // on the hung subsystem", which a timeout race proves directly.
    let resp = tokio::time::timeout(
        Duration::from_secs(2),
        app.oneshot(authed_get("/api/v1/system/status")),
    )
    .await
    .expect("endpoint must resolve within 2s even with a hung subsystem")
    .unwrap();

    // WHY(#7288): an unanswered check is unconfirmed, not a proven failure
    // — `session_store`'s lock is also taken synchronously by ordinary
    // request handling elsewhere, so a timeout here can mean real
    // contention rather than a genuine hang. The aggregate must read
    // "degraded" and 200, not "failed" and a 503 that would make a
    // busy-but-live gateway report itself down.
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "degraded");
    let subsystems = body["subsystems"].as_array().expect("subsystems array");

    assert!(
        !subsystems
            .iter()
            .any(|s| s["id"] == "system_status_collector"),
        "hang must be attributed to the real subsystem, not the opaque collector fallback"
    );

    // The hung subsystem itself is reported with the typed "timeout" state,
    // never silently folded into an indistinguishable "failed".
    for id in ["session_store", "tool_execution_history"] {
        let subsystem = subsystems
            .iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("missing subsystem: {id}"));
        assert_eq!(
            subsystem["status"], "timeout",
            "subsystem {id}: {subsystem:?}"
        );
        assert!(
            subsystem["failure_reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("timed out")),
            "subsystem {id} failure_reason should name the timeout: {subsystem:?}"
        );
    }

    // Subsystems unrelated to the held lock still report their real status
    // rather than being swept up into the hang.
    for id in [
        "nous_runtime",
        "disk_space",
        "domain_packs",
        "event_bus",
        "provider_reachability",
        "embeddings",
    ] {
        let subsystem = subsystems
            .iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("missing subsystem: {id}"));
        assert_ne!(
            subsystem["status"], "timeout",
            "subsystem {id} should be unaffected by the held session_store lock: {subsystem:?}"
        );
    }
}
