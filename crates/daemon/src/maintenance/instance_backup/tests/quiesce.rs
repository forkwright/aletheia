use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::*;

/// #6442 regression: the default path (no quiesce hook) must stay honest —
/// `quiesced: false` — and per-entry `snapshot_time` must no longer be one
/// stamp copy-pasted onto every store, because that silently implied
/// simultaneity a raw sequential filesystem copy never had.
#[test]
fn create_backup_default_is_not_quiesced_and_records_real_per_entry_skew() {
    let (_tmp, backup_path) = create_basic_instance_backup();

    let manifest_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    let manifest: BackupManifest = serde_json::from_value(manifest_json.clone()).unwrap();

    assert!(
        !manifest.quiesced,
        "no hook was supplied; quiesced must stay false"
    );
    assert!(
        manifest_json
            .get(MANIFEST_QUIESCE_MECHANISM_FIELD)
            .is_none(),
        "no quiesce mechanism ran; the field must be absent, not a placeholder"
    );

    let knowledge_time = manifest
        .stores
        .iter()
        .find(|s| s.name == "knowledge.fjall")
        .expect("knowledge entry")
        .snapshot_time
        .clone();
    let sessions_time = manifest
        .stores
        .iter()
        .find(|s| s.name == "sessions.db")
        .expect("sessions entry")
        .snapshot_time
        .clone();
    assert_ne!(
        knowledge_time, sessions_time,
        "each store must carry its own real capture time, not a shared stamp"
    );

    let skew = manifest_json
        .get(MANIFEST_OBSERVED_SKEW_SECONDS_FIELD)
        .and_then(serde_json::Value::as_f64)
        .expect("observed skew must be recorded once two-plus entries were copied");
    assert!(
        skew > 0.0,
        "two real, separately-copied entries must show a positive observed skew, got {skew}"
    );
}

/// #6442: `quiesced: true` is a fact derived from the hook actually running,
/// never a bare declaration — supplying one must be reflected honestly, and
/// naming a mechanism must show up as manifest evidence.
#[test]
fn create_backup_with_quiesce_derives_quiesced_true_and_records_mechanism() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    make_fjall_store(&instance_root.join("data").join("sessions.db"));

    let manager = InstanceBackup::new(InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir: tmp.path().join("backups"),
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    });

    let mut hook_called = false;
    let report = manager
        .create_backup_with_quiesce(|| {
            hook_called = true;
            Ok(Some(String::from("test-writer-pause+flush")))
        })
        .expect("backup with quiesce succeeds");
    assert!(hook_called, "quiesce hook must be invoked");

    let backup_path = report.backup_path.expect("backup path set");
    let manifest_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    let manifest: BackupManifest = serde_json::from_value(manifest_json.clone()).unwrap();

    assert!(
        manifest.quiesced,
        "a hook that ran and named a mechanism must derive quiesced: true"
    );
    assert_eq!(
        manifest_json
            .get(MANIFEST_QUIESCE_MECHANISM_FIELD)
            .and_then(serde_json::Value::as_str),
        Some("test-writer-pause+flush"),
        "the mechanism the hook named must be recorded as evidence"
    );
}

/// #6442: a hook that declines to name a mechanism (`Ok(None)`) — e.g. a
/// caller that ran but could not actually establish a checkpoint — must
/// still leave the backup honestly `quiesced: false`. Running is not the
/// same as succeeding.
#[test]
fn create_backup_with_quiesce_hook_running_without_mechanism_stays_unquiesced() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    make_fjall_store(&instance_root.join("data").join("sessions.db"));

    let manager = InstanceBackup::new(InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir: tmp.path().join("backups"),
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    });

    let report = manager
        .create_backup_with_quiesce(|| Ok(None))
        .expect("backup succeeds even without a mechanism");
    let backup_path = report.backup_path.expect("backup path set");
    let manifest: BackupManifest =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    assert!(!manifest.quiesced);
}

/// #6442: a failing quiesce hook must abort before anything is copied, and
/// must not leave a diagnostic-free empty staging directory behind.
#[test]
fn create_backup_with_quiesce_hook_failure_aborts_before_any_copy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    make_fjall_store(&instance_root.join("data").join("sessions.db"));
    let backup_dir = tmp.path().join("backups");

    let manager = InstanceBackup::new(InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir: backup_dir.clone(),
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    });

    let result = manager.create_backup_with_quiesce(|| {
        error::MaintenanceInvariantSnafu {
            context: String::from("simulated writer-pause failure"),
        }
        .fail()
    });
    assert!(result.is_err(), "a failing hook must fail the backup");

    let leftovers: Vec<_> = fs::read_dir(&backup_dir)
        .map(|dir| dir.flatten().collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "an empty pre-copy staging dir must not be left behind: {leftovers:?}"
    );
}

/// #6442: proves the mechanism this issue asks for actually works when
/// wired — a concurrent, correlated writer to two independently mutable
/// stores, paused and flushed by the quiesce hook before either store is
/// copied, restores with the cross-store invariant intact at the captured
/// boundary. `instance_backup` cannot pause a real daemon's writers itself
/// (no live handle, and the CLI path is a separate OS process from any live
/// daemon — see `InstanceBackup::create_backup_with_quiesce`'s doc comment);
/// this test plays the role of the caller that DOES own such handles, to
/// prove the coordination point is sound for whoever wires it up for real.
#[test]
fn create_backup_with_quiesce_preserves_cross_store_invariant_under_concurrent_writes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();

    let knowledge_path = instance_root.join("data").join("knowledge.fjall");
    let sessions_path = instance_root.join("data").join("sessions.db");
    fs::create_dir_all(&knowledge_path).unwrap();
    fs::create_dir_all(&sessions_path).unwrap();

    let knowledge_db = fjall::SingleWriterTxDatabase::builder(&knowledge_path)
        .worker_threads_unchecked(0)
        .open()
        .expect("open knowledge store");
    let sessions_db = fjall::SingleWriterTxDatabase::builder(&sessions_path)
        .worker_threads_unchecked(0)
        .open()
        .expect("open sessions store");
    let knowledge_ks = knowledge_db
        .keyspace("seq", fjall::KeyspaceCreateOptions::default)
        .unwrap();
    let sessions_ks = sessions_db
        .keyspace("seq", fjall::KeyspaceCreateOptions::default)
        .unwrap();

    // WHY: models two independently mutable stores that must reflect the
    // SAME logical "seq" at any instant a backup boundary observes them —
    // the cross-store invariant a whole-instance backup is supposed to
    // protect. `write_in_progress` lets the quiesce hook wait for any
    // in-flight pair to finish rather than racing it, so the assertion below
    // is deterministic rather than merely probable.
    let running = Arc::new(AtomicBool::new(true));
    let paused = Arc::new(AtomicBool::new(false));
    let write_in_progress = Arc::new(AtomicBool::new(false));
    let seq = Arc::new(AtomicU64::new(0));

    let writer = {
        let running = running.clone();
        let paused = paused.clone();
        let write_in_progress = write_in_progress.clone();
        let seq = seq.clone();
        let knowledge_ks = knowledge_ks.clone();
        let sessions_ks = sessions_ks.clone();
        std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                if paused.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                    continue;
                }
                write_in_progress.store(true, Ordering::SeqCst);
                let n = seq.fetch_add(1, Ordering::SeqCst) + 1;
                let bytes = n.to_le_bytes().to_vec();
                knowledge_ks.insert("seq", bytes.clone()).unwrap();
                sessions_ks.insert("seq", bytes).unwrap();
                write_in_progress.store(false, Ordering::SeqCst);
            }
        })
    };

    while seq.load(Ordering::SeqCst) < 5 {
        std::thread::yield_now();
    }

    let manager = InstanceBackup::new(InstanceBackupConfig {
        enabled: true,
        instance_root: instance_root.clone(),
        backup_dir: tmp.path().join("backups"),
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    });

    let hook_paused = paused.clone();
    let hook_in_progress = write_in_progress.clone();
    let report = manager
        .create_backup_with_quiesce(move || {
            hook_paused.store(true, Ordering::SeqCst);
            // WHY: wait out any write pair already in flight so the flush
            // below never observes a knowledge write with no matching
            // sessions write (or vice versa).
            while hook_in_progress.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
            knowledge_db.persist(fjall::PersistMode::SyncAll).unwrap();
            sessions_db.persist(fjall::PersistMode::SyncAll).unwrap();
            Ok(Some(String::from("test-writer-pause+flush")))
        })
        .expect("backup with quiesce succeeds");

    // WHY: tear down by clearing `running`, not by unpausing — the quiesce
    // hook's `knowledge_db`/`sessions_db` were moved into it and are dropped
    // the instant that `FnOnce` call returns (already flushed and durable by
    // then). Unpausing here would let the writer thread call `.insert` again
    // on keyspace clones backed by an already-dropped database. `running`
    // alone is sufficient: the loop rechecks it on every iteration, paused
    // or not.
    running.store(false, Ordering::SeqCst);
    writer.join().expect("writer thread joins");

    let backup_path = report.backup_path.expect("backup path set");
    let manifest: BackupManifest =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    assert!(
        manifest.quiesced,
        "the wired hook must derive quiesced: true"
    );

    let restore_root = tmp.path().join("restored");
    fs::create_dir_all(&restore_root).unwrap();
    let restore_manager = InstanceBackup::new(InstanceBackupConfig {
        enabled: true,
        instance_root: restore_root.clone(),
        backup_dir: tmp.path().join("backups"),
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    });
    restore_manager
        .restore_backup(&InstanceRestoreOptions::all_entries(backup_path))
        .expect("restore succeeds");

    assert_eq!(
        restored_seq(&restore_root, "knowledge.fjall"),
        restored_seq(&restore_root, "sessions.db"),
        "cross-store invariant must hold at the quiesced boundary: knowledge \
         and sessions must show the identical seq the writer paused at"
    );
}

fn restored_seq(restore_root: &Path, store_name: &str) -> Vec<u8> {
    let database =
        fjall::SingleWriterTxDatabase::builder(restore_root.join("data").join(store_name))
            .worker_threads_unchecked(0)
            .open()
            .unwrap_or_else(|error| panic!("open restored {store_name}: {error}"));
    database
        .keyspace("seq", fjall::KeyspaceCreateOptions::default)
        .expect("open restored seq keyspace")
        .get("seq")
        .expect("read restored seq")
        .expect("seq present in restore")
        .to_vec()
}
