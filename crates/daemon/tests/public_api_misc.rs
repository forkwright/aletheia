#![expect(
    clippy::unwrap_used,
    reason = "test assertions — panicking on failure is the point"
)]
#![expect(
    clippy::expect_used,
    reason = "test assertions — panicking on failure is the point"
)]
#![expect(
    unused_imports,
    reason = "split public_api_*.rs files share the same import block; not every file uses every item"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use oikonomos::bridge::{DaemonBridge, NoopBridge};
use oikonomos::coordination::Coordinator;
use oikonomos::cron::{
    CronConfig, CronEvolutionConfig, CronGraphCleanupConfig, CronReflectionConfig,
};
use oikonomos::error::Error as DaemonError;
use oikonomos::maintenance::{
    AutoDreamConfig, DbMonitor, DbMonitoringConfig, DbStatus, DriftDetectionConfig, DriftDetector,
    KnowledgeMaintenanceConfig, MaintenanceConfig, MaintenanceReport, ProposeRulesConfig,
    RetentionConfig, RetentionExecutor, RetentionSummary, TraceRotationConfig, TraceRotator,
};
use oikonomos::probe::{
    Probe, ProbeAuditConfig, ProbeAuditSummary, ProbeCategory, ProbeResult, ProbeSet,
    build_probe_audit_prompt,
};
use oikonomos::runner::{DaemonOutputMode, ExecutionResult, TaskRunner};
use oikonomos::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, TaskStatus};
use oikonomos::self_prompt::{SELF_PROMPT_SESSION_KEY, SelfPromptConfig};
use oikonomos::state::{AllowedTriggers, DaemonConfig, WorkspaceGuard};
use oikonomos::triggers::TriggerRouter;

mod common;
use common::{make_runner, write_fixture};

// Split: DaemonBridge/NoopBridge + WorkspaceGuard + Coordinator / TriggerRouter / DaemonError.

// Section 6: DaemonBridge + NoopBridge
// ---------------------------------------------------------------------------

#[tokio::test]
async fn noop_bridge_returns_unsuccessful_with_diagnostic_message() {
    let bridge = NoopBridge;
    let result = bridge
        .send_prompt("test-nous", "test-session", "hello world")
        .await
        .expect("NoopBridge must not error");

    assert!(!result.success, "NoopBridge must flag success=false");
    let output = result
        .output
        .expect("NoopBridge must return diagnostic output");
    assert!(
        output.contains("no bridge configured"),
        "output must explain why the dispatch was skipped, got: {output}"
    );
}

#[tokio::test]
async fn noop_bridge_is_object_safe_behind_arc_dyn() {
    // WHY: production wiring holds the bridge as `Arc<dyn DaemonBridge>`, so
    // the trait must be object-safe and the Arc<dyn ...> wrapper must also
    // forward the call (implemented in bridge_impl for Arc<dyn DaemonBridge>).
    let bridge: Arc<dyn DaemonBridge> = Arc::new(NoopBridge);
    let result = bridge
        .send_prompt("test-nous", "sess", "ping")
        .await
        .expect("arc-dyn dispatch succeeds");
    assert!(!result.success);
}

// ---------------------------------------------------------------------------
// Section 9: WorkspaceGuard single-instance locking
// ---------------------------------------------------------------------------

#[test]
fn workspace_guard_acquires_and_exposes_lock_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let guard = WorkspaceGuard::acquire(tmp.path()).expect("first lock acquires");

    let lock_path = guard.lock_path();
    assert!(lock_path.exists(), "lock file must be created at lock_path");
    assert!(
        lock_path.ends_with(".aletheia/daemon.lock"),
        "lock path must be .aletheia/daemon.lock, got {}",
        lock_path.display()
    );
}

#[test]
fn workspace_guard_acquires_releases_and_reacquires_cleanly() {
    // WHY: verify the full lifecycle — acquire → valid lock_path → drop →
    // re-acquire succeeds. Double-acquisition exclusion (second acquire
    // fails while first is held) is tested in state.rs unit tests where
    // rustix flock properly enforces it within the same process.
    let tmp = tempfile::tempdir().expect("tempdir");

    let first = WorkspaceGuard::acquire(tmp.path()).expect("first lock acquires");
    let path_first = first.lock_path().to_path_buf();
    assert!(path_first.exists());
    drop(first);

    let second =
        WorkspaceGuard::acquire(tmp.path()).expect("second acquisition after drop must succeed");
    assert!(second.lock_path().exists());
    drop(second);
}

// ---------------------------------------------------------------------------
// Section 10: Misc helpers — Coordinator, TriggerRouter, DaemonError traits
// ---------------------------------------------------------------------------

#[test]
fn coordinator_preserves_max_children_limit() {
    let coord = Coordinator::new(4);
    assert_eq!(coord.max_children(), 4);
    // Coordinator is a reserved boundary today: it preserves configuration but
    // does not spawn or track children yet. Verify it survives zero capacity.
    let zero = Coordinator::new(0);
    assert_eq!(zero.max_children(), 0);
}

#[test]
fn trigger_router_default_and_new_produce_equivalent_routers() {
    // TriggerRouter is a reserved boundary today with no observable event
    // dispatch state. Verify both constructors succeed and the type remains
    // Debug-printable while it is unwired.
    let via_new = TriggerRouter::new();
    let via_default = TriggerRouter::default();
    let new_debug = format!("{via_new:?}");
    let default_debug = format!("{via_default:?}");
    assert_eq!(
        new_debug, default_debug,
        "TriggerRouter::new and default must produce identical Debug output"
    );
}

#[test]
fn daemon_error_satisfies_send_sync_and_std_error() {
    // WHY: the error type flows across task boundaries, so it must be Send,
    // Sync, and implement std::error::Error.
    fn assert_traits<T: std::error::Error + Send + Sync + 'static>() {}
    assert_traits::<DaemonError>();
}

#[test]
fn probe_category_serde_uses_snake_case() {
    // Serde rename_all = "snake_case" is part of the observability contract:
    // downstream consumers parse these strings. Any rename here is a breaking
    // change and must be caught by this test.
    assert_eq!(
        serde_json::to_string(&ProbeCategory::Consistency).unwrap(),
        "\"consistency\""
    );
    assert_eq!(
        serde_json::to_string(&ProbeCategory::Boundary).unwrap(),
        "\"boundary\""
    );
    assert_eq!(
        serde_json::to_string(&ProbeCategory::Recall).unwrap(),
        "\"recall\""
    );
}

// ---------------------------------------------------------------------------
