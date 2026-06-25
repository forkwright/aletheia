use std::os::unix::fs::PermissionsExt;

use super::*;

fn write_fixture(path: impl AsRef<Path>, content: &str) {
    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture: synchronous write in non-async test context"
    )]
    fs::write(path.as_ref(), content).expect("write fixture");
    let mut perms = fs::metadata(path.as_ref())
        .expect("read fixture metadata")
        .permissions();
    perms.set_mode(0o644);
    fs::set_permissions(path.as_ref(), perms).expect("set fixture permissions");
}

/// Create a real fjall store with a small amount of data for backup tests.
fn make_fjall_store(path: &Path) {
    fs::create_dir_all(path).expect("create store dir");
    let db = fjall::SingleWriterTxDatabase::builder(path)
        .worker_threads_unchecked(0)
        .open()
        .expect("open test fjall store");
    let partition = db
        .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
        .expect("create test partition");
    for i in 0..5u8 {
        partition
            .insert(format!("key-{i}"), vec![i])
            .expect("insert test key");
    }
    db.persist(fjall::PersistMode::SyncAll)
        .expect("persist test store");
    drop(db);
}

#[test]
fn create_backup_stages_verifies_and_publishes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = tmp.path().join("knowledge.fjall");
    let backup_dir = tmp.path().join("backups");
    make_fjall_store(&source);

    let config = FjallBackupConfig {
        enabled: true,
        source_dir: source,
        backup_dir: backup_dir.clone(),
        interval_hours: 24,
        retention_count: 7,
    };

    let manager = FjallBackup::new(config);
    let report = manager.create_backup().expect("backup succeeds");

    let backup_path = report.backup_path.expect("backup path set");
    assert!(backup_path.exists());
    assert!(backup_path.join(COMPLETE_MARKER).is_file());
    assert_eq!(backup_path.parent(), Some(backup_dir.as_path()));
    assert!(report.files_copied > 0);
    assert!(report.bytes_copied > 0);

    // No staging directories should remain after a successful publish.
    let staging_left = fs::read_dir(&backup_dir)
        .expect("read backup dir")
        .flatten()
        .any(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with(STAGING_DIR_PREFIX)
        });
    assert!(!staging_left, "staging directory leaked after publish");

    // The published backup must be a valid fjall store.
    let verify = FjallBackup::verify_store(&backup_path).expect("verify published backup");
    assert!(verify.is_valid());
    assert_eq!(verify.total_keys, 5);

    let backups = manager.list_backups().expect("list succeeds");
    assert_eq!(backups.len(), 1);
}

#[test]
fn prune_respects_retention_count() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = tmp.path().join("knowledge.fjall");
    let backup_dir = tmp.path().join("backups");
    make_fjall_store(&source);

    let config = FjallBackupConfig {
        enabled: true,
        source_dir: source,
        backup_dir: backup_dir.clone(),
        interval_hours: 24,
        retention_count: 2,
    };

    let manager = FjallBackup::new(config);

    // Create 4 backups; BACKUP_SEQ guarantees distinct directory names.
    for _ in 0..4 {
        manager.create_backup().expect("backup succeeds");
    }

    let backups = manager.list_backups().expect("list succeeds");
    assert_eq!(backups.len(), 2, "should keep only 2 backups");
}

#[test]
fn nonexistent_source_returns_empty_report() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = FjallBackupConfig {
        source_dir: tmp.path().join("nonexistent"),
        backup_dir: tmp.path().join("backups"),
        ..FjallBackupConfig::default()
    };

    let manager = FjallBackup::new(config);
    let report = manager.create_backup().expect("should not error");
    assert!(!report.succeeded());
    assert_eq!(report.files_copied, 0);
}

#[test]
fn list_empty_backup_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = FjallBackupConfig {
        backup_dir: tmp.path().join("nonexistent-backups"),
        ..FjallBackupConfig::default()
    };

    let manager = FjallBackup::new(config);
    let backups = manager.list_backups().expect("list succeeds");
    assert!(backups.is_empty());
}

#[test]
fn list_backups_skips_incomplete_staging() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let backup_dir = tmp.path().join("backups");
    fs::create_dir_all(&backup_dir).unwrap();

    // Create a fake staging directory that mimics a crashed backup.
    let staging = backup_dir.join(format!("{STAGING_DIR_PREFIX}crashed"));
    fs::create_dir_all(&staging).unwrap();
    write_fixture(staging.join("partial.sst"), "partial data");

    let config = FjallBackupConfig {
        backup_dir,
        ..FjallBackupConfig::default()
    };
    let manager = FjallBackup::new(config);
    let backups = manager.list_backups().expect("list succeeds");
    assert!(backups.is_empty(), "staging directory must not be listed");
}

#[test]
fn prune_only_completed_backups() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = tmp.path().join("knowledge.fjall");
    let backup_dir = tmp.path().join("backups");
    make_fjall_store(&source);

    let config = FjallBackupConfig {
        enabled: true,
        source_dir: source,
        backup_dir: backup_dir.clone(),
        interval_hours: 24,
        retention_count: 1,
    };
    let manager = FjallBackup::new(config);

    // First backup: 1 total, retention=1 → internal prune is a no-op.
    manager.create_backup().expect("backup 1 succeeds");
    // Second backup: 2 total, retention=1 → internal prune removes backup 1.
    let report2 = manager.create_backup().expect("backup 2 succeeds");
    assert_eq!(
        report2.backups_pruned, 1,
        "second create should have internally pruned the first"
    );

    // Inject an incomplete staging directory after both backups are complete.
    let staging = backup_dir.join(format!("{STAGING_DIR_PREFIX}incomplete"));
    fs::create_dir_all(&staging).unwrap();
    write_fixture(staging.join("partial.sst"), "partial data");

    // Exactly one completed backup remains; staging is not counted as completed.
    let listed = manager.list_backups().expect("list succeeds");
    assert_eq!(listed.len(), 1, "only one completed backup should remain");

    // The incomplete staging directory must not be pruned.
    assert!(staging.exists(), "incomplete staging must not be pruned");
}

#[test]
fn create_backup_cleans_stale_staging_and_restores() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = tmp.path().join("knowledge.fjall");
    let backup_dir = tmp.path().join("backups");
    make_fjall_store(&source);

    // Simulate an interrupted previous backup run.
    let stale = backup_dir.join(format!("{STAGING_DIR_PREFIX}interrupted"));
    fs::create_dir_all(&stale).unwrap();
    write_fixture(stale.join("partial.sst"), "partial data");

    let config = FjallBackupConfig {
        enabled: true,
        source_dir: source,
        backup_dir: backup_dir.clone(),
        interval_hours: 24,
        retention_count: 7,
    };
    let manager = FjallBackup::new(config);

    let report = manager.create_backup().expect("backup succeeds");
    let backup_path = report.backup_path.expect("backup path set");

    // The stale staging directory from the interrupted run must be gone.
    assert!(!stale.exists(), "stale staging directory should be cleaned");

    // The new backup must be restorable.
    let verify = FjallBackup::verify_store(&backup_path).expect("verify restored backup");
    assert!(verify.is_valid());
    assert_eq!(verify.total_keys, 5);
}

#[test]
fn create_backup_with_quiesce_calls_hook() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = tmp.path().join("knowledge.fjall");
    let backup_dir = tmp.path().join("backups");
    make_fjall_store(&source);

    let config = FjallBackupConfig {
        enabled: true,
        source_dir: source,
        backup_dir,
        interval_hours: 24,
        retention_count: 7,
    };
    let manager = FjallBackup::new(config);

    let mut quiesce_called = false;
    let report = manager
        .create_backup_with_quiesce(|| {
            quiesce_called = true;
            Ok(())
        })
        .expect("backup succeeds");

    assert!(quiesce_called, "quiesce hook must be invoked");
    assert!(report.backup_path.is_some());
}

#[test]
fn default_config_values() {
    let config = FjallBackupConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.interval_hours, 24);
    assert_eq!(config.retention_count, 7);
}

/// #5754 regression: verify must not destructively recover the canonical
/// backup directory. A hot-copy may contain orphan segment files; opening
/// the backup in-place would delete them. Verify should copy first and only
/// open the disposable copy.
#[test]
fn verify_store_does_not_mutate_backup_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = tmp.path().join("knowledge.fjall");
    let backup = tmp.path().join("backup").join("knowledge.fjall");

    // Create a live store with one partition and a few keys.
    {
        fs::create_dir_all(&source).unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(&source)
            .worker_threads_unchecked(0)
            .open()
            .unwrap();
        let partition = db
            .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        for i in 0..5u8 {
            partition.insert(format!("key-{i}"), vec![i]).unwrap();
        }
        db.persist(fjall::PersistMode::SyncAll).unwrap();
        drop(db);
    }

    // Simulate a hot-copy backup.
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    copy_dir_recursive(&source, &backup).unwrap();

    // Inject a fake orphan segment file into the backup. A real hot-copy
    // could contain compaction-in-progress segments not yet referenced by
    // the manifest; the name here is chosen to look like an SST.
    let orphan_rel = PathBuf::from("orphan-0001.sst");
    let orphan = backup.join(&orphan_rel);
    write_fixture(&orphan, "orphan segment data");

    let files_before: std::collections::HashSet<PathBuf> =
        walk_paths(&backup).into_iter().collect();
    assert!(files_before.contains(&orphan_rel));

    // Verify the backup. This must open only a temp copy, leaving the
    // canonical backup untouched.
    let result = FjallBackup::verify_store(&backup).expect("verify succeeds");
    assert!(result.is_valid());
    assert_eq!(result.total_keys, 5);

    let files_after: std::collections::HashSet<PathBuf> = walk_paths(&backup).into_iter().collect();
    assert!(
        files_after.contains(&orphan_rel),
        "verify deleted the orphan segment from the canonical backup"
    );
    assert_eq!(
        files_before, files_after,
        "verify mutated the canonical backup directory"
    );

    // Simulate restore by opening the backup directly. The orphan is
    // expected to be discarded by recovery, but all real records must
    // survive.
    let restored = fjall::SingleWriterTxDatabase::builder(&backup)
        .worker_threads_unchecked(0)
        .open()
        .unwrap();
    let partition = restored
        .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
        .unwrap();
    let snap = restored.read_tx();
    let count = snap.range::<&str, _>(&partition, ..).count();
    assert_eq!(count, 5, "restore lost real records");
}

fn walk_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                paths.extend(walk_paths(&path));
            } else {
                paths.push(path.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    paths
}
