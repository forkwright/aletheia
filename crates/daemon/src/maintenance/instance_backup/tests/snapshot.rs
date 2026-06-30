use super::*;

/// #4950 regression: backup creation must stage, verify, and atomically
/// publish the set; the manifest must record snapshot metadata and store
/// generation IDs.
#[test]
fn create_backup_publishes_verified_snapshot_with_metadata() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();
    write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

    make_fjall_store_with_data(&instance_root.join("data").join("knowledge.fjall"), "k1");
    make_fjall_store_with_data(&instance_root.join("data").join("sessions.db"), "s1");

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
    let report = manager.create_backup().expect("backup succeeds");
    let backup_path = report.backup_path.expect("backup path set");

    // The published path must live directly under backup_dir, not in a
    // hidden staging directory.
    assert_eq!(
        backup_path.parent(),
        Some(backup_dir.as_path()),
        "backup was not atomically published into backup_dir"
    );
    assert!(
        !backup_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(STAGING_DIR_PREFIX),
        "backup path is still a staging directory"
    );

    // No staging directories should remain visible after publish.
    let leftover_staging: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(STAGING_DIR_PREFIX)
        })
        .collect();
    assert!(
        leftover_staging.is_empty(),
        "staging directories leaked after publish: {leftover_staging:?}"
    );

    let manifest: BackupManifest =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    assert!(
        !manifest.snapshot_epoch.is_empty(),
        "snapshot_epoch must be recorded"
    );
    assert_eq!(
        manifest.snapshot_protocol_version,
        SNAPSHOT_PROTOCOL_VERSION
    );
    assert!(
        !manifest.quiesced,
        "live snapshot must be recorded as not quiesced"
    );
    assert!(
        manifest.store_generations.contains_key("knowledge.fjall"),
        "knowledge generation must be captured"
    );
    assert!(
        manifest.store_generations.contains_key("sessions.db"),
        "sessions generation must be captured"
    );
    let manifest_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    let sessions_checkpoint = manifest_json
        .get("stores")
        .and_then(serde_json::Value::as_array)
        .and_then(|stores| {
            stores.iter().find(|entry| {
                entry.get("name").and_then(serde_json::Value::as_str) == Some("sessions.db")
            })
        })
        .and_then(|entry| entry.get(MANIFEST_CHECKPOINT_GENERATIONS_FIELD))
        .and_then(serde_json::Value::as_object)
        .and_then(|generations| generations.get("sessions.db"));
    assert!(
        sessions_checkpoint.is_some(),
        "sessions entry must carry checkpoint generation evidence"
    );

    let result = InstanceBackup::verify_backup(&backup_path).unwrap();
    assert!(result.first_error.is_none());
    assert_eq!(result.total_keys, 2);
}

/// #4950 regression: in-progress staging directories must never be listed
/// as valid backups.
#[test]
fn list_backups_skips_staging_directories() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let backup_dir = tmp.path().join("backups");
    fs::create_dir_all(&backup_dir).unwrap();

    let manifest = BackupManifest {
        version: String::from(MANIFEST_VERSION),
        created_at: jiff::Zoned::now().to_string(),
        source_root: backup_dir.clone(),
        stores: Vec::new(),
        optional_stores: Vec::new(),
        workspace_omissions: Vec::new(),
        total_bytes: 0,
        snapshot_epoch: jiff::Zoned::now().to_string(),
        snapshot_protocol_version: String::from(SNAPSHOT_PROTOCOL_VERSION),
        quiesced: false,
        store_generations: HashMap::new(),
        symlink_policy: String::from(SYMLINK_POLICY),
    };
    let manifest_json = serde_json::to_string(&manifest).unwrap();

    // Create a valid backup set.
    let valid = backup_dir.join("20260101-000000.000");
    fs::create_dir_all(&valid).unwrap();
    write_text_file(&valid.join("manifest.json"), &manifest_json).unwrap();

    // Create a fake staging directory with a manifest (simulating an
    // interrupted backup).
    let staging = backup_dir.join(format!("{STAGING_DIR_PREFIX}fake"));
    fs::create_dir_all(&staging).unwrap();
    write_text_file(&staging.join("manifest.json"), &manifest_json).unwrap();

    let manager = InstanceBackup::new(InstanceBackupConfig {
        backup_dir,
        ..InstanceBackupConfig::default()
    });
    let backups = manager.list_backups().expect("list succeeds");
    assert_eq!(backups.len(), 1);
    assert_eq!(backups.first().unwrap().path, valid);
}
