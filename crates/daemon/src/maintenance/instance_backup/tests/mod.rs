use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error;

use super::restore::{publish_restore_plan, rollback_restore};
use super::*;

mod creation;
mod restore;
mod snapshot;
mod verify;

fn make_fjall_store(path: &Path) {
    fs::create_dir_all(path).unwrap();
    let db = fjall::SingleWriterTxDatabase::builder(path).open().unwrap();
    let _ = db
        .keyspace("test", fjall::KeyspaceCreateOptions::default)
        .unwrap();
    drop(db);
}

fn assert_fjall_marker(root: &Path, rel: &[&str]) {
    let mut path = root.to_path_buf();
    for segment in rel {
        path.push(segment);
    }
    assert!(path.join("version").is_file(), "missing {}", path.display());
}

fn assert_optional_store(manifest: &BackupManifest, name: &str) {
    assert!(
        manifest
            .optional_stores
            .iter()
            .any(|entry| entry.name == name && entry.status == "ok"),
        "manifest should include runtime store {name}"
    );
}

fn assert_generations(verify: &InstanceVerifyResult, names: &[&str]) {
    for name in names {
        assert!(
            verify.store_generations.contains_key(*name),
            "missing generation for {name}: {:?}",
            verify.store_generations
        );
    }
}

fn create_basic_instance_backup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();
    write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();
    write_text_file(
        &instance_root.join("nous").join("syn").join("SOUL.md"),
        "soul",
    )
    .unwrap();

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
    let report = manager.create_backup().expect("backup succeeds");
    let backup_path = report.backup_path.expect("backup path set");

    (tmp, backup_path)
}

fn first_verify_error(backup_path: &Path) -> String {
    InstanceBackup::verify_backup(backup_path)
        .unwrap()
        .first_error
        .expect("verification should fail")
}

fn mutate_manifest_json<F>(backup_path: &Path, mutate: F)
where
    F: FnOnce(&mut serde_json::Value),
{
    let manifest_path = backup_path.join("manifest.json");
    let mut manifest_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    mutate(&mut manifest_json);
    write_text_file(
        &manifest_path,
        &serde_json::to_string_pretty(&manifest_json).unwrap(),
    )
    .unwrap();
}

fn assert_manifest_mutation_rejected<F>(mutate: F, expected: &str)
where
    F: FnOnce(&mut serde_json::Value),
{
    let (_tmp, backup_path) = create_basic_instance_backup();
    mutate_manifest_json(&backup_path, mutate);
    let err = first_verify_error(&backup_path);
    assert!(
        err.contains(expected),
        "error should contain {expected:?}: {err}"
    );
}

fn make_fjall_store_with_data(path: &Path, key: &str) {
    fs::create_dir_all(path).unwrap();
    let db = fjall::SingleWriterTxDatabase::builder(path)
        .worker_threads_unchecked(0)
        .open()
        .unwrap();
    let partition = db
        .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
        .unwrap();
    partition.insert(key, b"value").unwrap();
    db.persist(fjall::PersistMode::SyncAll).unwrap();
    drop(db);
}
