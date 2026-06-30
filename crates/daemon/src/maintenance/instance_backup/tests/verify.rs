use super::*;

#[test]
fn verify_backup_passes_for_complete_set() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();
    write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    make_fjall_store(&instance_root.join("data").join("sessions.db"));

    let backup_dir = tmp.path().join("backups");
    let config = InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir: backup_dir.clone(),
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    };

    let manager = InstanceBackup::new(config);
    let report = manager.create_backup().unwrap();
    let backup_path = report.backup_path.unwrap();

    let result = InstanceBackup::verify_backup(&backup_path).unwrap();
    assert!(result.first_error.is_none());
    assert_eq!(result.store_results.len(), 3);
    assert!(result.store_results.iter().all(|(_, r)| r.is_ok()));
    assert!(
        result
            .store_results
            .iter()
            .any(|(name, result)| name == "config" && result.is_ok())
    );
}

#[test]
fn verify_backup_rejects_missing_sessions_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let backup_path = tmp.path().join("bad-backup");
    fs::create_dir_all(&backup_path).unwrap();

    // Create a manifest that claims only knowledge.fjall was backed up.
    let manifest = BackupManifest {
        version: String::from(MANIFEST_VERSION),
        created_at: jiff::Zoned::now().to_string(),
        source_root: tmp.path().join("instance"),
        stores: vec![StoreEntry {
            name: String::from("knowledge.fjall"),
            source_path: tmp
                .path()
                .join("instance")
                .join("data")
                .join("knowledge.fjall"),
            backup_path: PathBuf::from("stores/knowledge.fjall"),
            snapshot_time: jiff::Zoned::now().to_string(),
            byte_count: 0,
            status: String::from("ok"),
            agent_id: None,
            workspace_source_class: None,
            exclusion_reason: None,
            sha256: None,
        }],
        optional_stores: Vec::new(),
        workspace_omissions: Vec::new(),
        total_bytes: 0,
        snapshot_epoch: jiff::Zoned::now().to_string(),
        snapshot_protocol_version: String::from(SNAPSHOT_PROTOCOL_VERSION),
        quiesced: false,
        store_generations: HashMap::new(),
        symlink_policy: String::from(SYMLINK_POLICY),
    };
    write_text_file(
        &backup_path.join("manifest.json"),
        &serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    // Create the knowledge store so that the failure is the missing sessions
    // entry, not a missing directory.
    make_fjall_store(&backup_path.join("stores").join("knowledge.fjall"));

    let result = InstanceBackup::verify_backup(&backup_path).unwrap();
    assert!(result.first_error.is_some());
    let err = result.first_error.unwrap();
    assert!(
        err.contains("sessions.db"),
        "error should mention sessions.db: {err}"
    );
}

#[test]
fn verify_backup_rejects_missing_included_runtime_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();

    make_fjall_store(
        &instance_root
            .join("data")
            .join("knowledge.fjall")
            .join("shared"),
    );
    make_fjall_store(&instance_root.join("data").join("sessions.db"));
    make_fjall_store(&instance_root.join("data").join("auth.fjall"));

    let backup_dir = tmp.path().join("backups");
    let manager = InstanceBackup::new(InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir,
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    });
    let report = manager.create_backup().expect("backup succeeds");
    let backup_path = report.backup_path.expect("backup path set");

    fs::remove_dir_all(backup_path.join("stores").join("auth.fjall")).unwrap();

    let result = InstanceBackup::verify_backup(&backup_path).unwrap();
    let err = result
        .first_error
        .expect("missing included auth store should fail verification");
    assert!(
        err.contains("auth.fjall"),
        "error should mention missing auth store: {err}"
    );
}

#[test]
fn verify_backup_rejects_mutated_truncated_deleted_and_swapped_entries_4953() {
    {
        let (_tmp, backup_path) = create_basic_instance_backup();
        write_text_file(&backup_path.join("config").join("aletheia.toml"), "tast").unwrap();

        let err = first_verify_error(&backup_path);
        assert!(
            err.contains("config") && err.contains("sha256 mismatch"),
            "same-size mutation should fail by hash: {err}"
        );
    }

    {
        let (_tmp, backup_path) = create_basic_instance_backup();
        write_text_file(&backup_path.join("config").join("aletheia.toml"), "t").unwrap();

        let err = first_verify_error(&backup_path);
        assert!(
            err.contains("config") && err.contains("byte_count mismatch"),
            "truncated entry should fail by byte count: {err}"
        );
    }

    {
        let (_tmp, backup_path) = create_basic_instance_backup();
        fs::remove_file(backup_path.join("config").join("aletheia.toml")).unwrap();

        let err = first_verify_error(&backup_path);
        assert!(
            err.contains("config") && err.contains("byte_count mismatch"),
            "deleted entry should fail by manifest integrity: {err}"
        );
    }

    {
        let (_tmp, backup_path) = create_basic_instance_backup();
        fs::remove_dir_all(backup_path.join("stores").join("sessions.db")).unwrap();
        write_text_file(
            &backup_path.join("stores").join("sessions.db"),
            "not-a-fjall-store",
        )
        .unwrap();

        let err = first_verify_error(&backup_path);
        assert!(
            err.contains("sessions.db") && err.contains("current fjall"),
            "swapped required store should fail by store format: {err}"
        );
    }
}

#[test]
#[expect(
    clippy::indexing_slicing,
    reason = "WHY(#4953): test mutates known fixture manifest JSON fields"
)]
fn verify_backup_rejects_unsafe_manifest_entries_4953() {
    assert_manifest_mutation_rejected(
        |manifest| {
            manifest["version"] = serde_json::Value::String(String::from("old"));
        },
        "unsupported backup manifest version",
    );
    assert_manifest_mutation_rejected(
        |manifest| {
            manifest["stores"][0]["backup_path"] =
                serde_json::Value::String(String::from("/tmp/evil"));
        },
        "clean relative",
    );
    assert_manifest_mutation_rejected(
        |manifest| {
            manifest["stores"][0]["backup_path"] =
                serde_json::Value::String(String::from("stores/../evil"));
        },
        "clean relative",
    );
    assert_manifest_mutation_rejected(
        |manifest| {
            manifest["optional_stores"][0]["name"] =
                serde_json::Value::String(String::from("knowledge.fjall"));
        },
        "duplicate manifest logical name",
    );
    assert_manifest_mutation_rejected(
        |manifest| {
            manifest["optional_stores"][0]["backup_path"] =
                serde_json::Value::String(String::from("stores/knowledge.fjall"));
        },
        "duplicate manifest backup path",
    );
    assert_manifest_mutation_rejected(
        |manifest| {
            manifest["stores"][0]["status"] = serde_json::Value::String(String::from("bogus"));
        },
        "invalid manifest status",
    );
    assert_manifest_mutation_rejected(
        |manifest| {
            manifest["stores"][0][MANIFEST_RESTORE_PATH_FIELD] =
                serde_json::Value::String(String::from("../data/knowledge.fjall"));
        },
        "restore path",
    );
}

#[test]
fn prune_respects_retention_count() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();
    write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    make_fjall_store(&instance_root.join("data").join("sessions.db"));

    let backup_dir = tmp.path().join("backups");
    let config = InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir: backup_dir.clone(),
        interval_hours: 24,
        retention_count: 2,
        additional_workspaces: Vec::new(),
    };

    let manager = InstanceBackup::new(config);

    // Create 4 backups; BACKUP_SEQ guarantees distinct directory names.
    for _ in 0..4 {
        manager.create_backup().expect("backup succeeds");
    }

    let backups = manager.list_backups().expect("list succeeds");
    assert_eq!(backups.len(), 2, "should keep only 2 backups");
}

#[test]
fn create_backup_rejects_file_shaped_sessions_store_4953() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    write_text_file(
        &instance_root.join("data").join("sessions.db"),
        "legacy-session-store",
    )
    .unwrap();

    let backup_dir = tmp.path().join("backups");
    let config = InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir,
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    };

    let manager = InstanceBackup::new(config);
    let err = manager
        .create_backup()
        .expect_err("file-shaped sessions store should be rejected");
    assert!(
        err.to_string().contains("session store must be"),
        "unexpected error: {err}"
    );
}

#[test]
fn verify_backup_rejects_file_shaped_required_store_4953() {
    let (_tmp, backup_path) = create_basic_instance_backup();
    fs::remove_dir_all(backup_path.join("stores").join("sessions.db")).unwrap();
    write_text_file(
        &backup_path.join("stores").join("sessions.db"),
        "not-a-fjall-store",
    )
    .unwrap();

    let result = InstanceBackup::verify_backup(&backup_path).unwrap();
    let err = result
        .first_error
        .expect("file-shaped required store should fail verification");
    assert!(
        err.contains("sessions.db") && err.contains("current fjall"),
        "unexpected error: {err}"
    );
}
