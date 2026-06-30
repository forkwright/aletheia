use super::*;

#[test]
fn create_backup_copies_required_stores_and_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();
    fs::create_dir_all(instance_root.join("nous").join("syn")).unwrap();
    write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();
    write_text_file(
        &instance_root.join("nous").join("syn").join("SOUL.md"),
        "soul",
    )
    .unwrap();

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
    let report = manager.create_backup().expect("backup succeeds");

    let files_copied = report.files_copied;
    let backup_path = report.backup_path.expect("backup path set");
    assert!(backup_path.join("manifest.json").is_file());
    assert!(
        backup_path
            .join("stores")
            .join("knowledge.fjall")
            .join("version")
            .is_file()
    );
    assert!(
        backup_path
            .join("stores")
            .join("sessions.db")
            .join("version")
            .is_file()
    );
    assert!(backup_path.join("config").join("aletheia.toml").is_file());
    assert!(
        backup_path
            .join("workspace")
            .join("nous")
            .join("syn")
            .join("SOUL.md")
            .is_file()
    );

    let manifest: BackupManifest =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest.version, MANIFEST_VERSION);
    assert_eq!(manifest.symlink_policy, SYMLINK_POLICY);
    assert_eq!(manifest.stores.len(), 3); // knowledge, sessions, config
    assert!(manifest.stores.iter().any(|s| s.name == "knowledge.fjall"));
    assert!(manifest.stores.iter().any(|s| s.name == "sessions.db"));
    assert!(manifest.stores.iter().any(|s| s.name == "config"));
    assert!(manifest.optional_stores.iter().any(|s| s.name == "nous"));

    let manifest_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest_json
            .get(MANIFEST_TOTAL_FILES_FIELD)
            .and_then(serde_json::Value::as_u64),
        Some(u64::from(files_copied))
    );
    let sessions = manifest_json
        .get("stores")
        .and_then(serde_json::Value::as_array)
        .and_then(|stores| {
            stores.iter().find(|entry| {
                entry.get("name").and_then(serde_json::Value::as_str) == Some("sessions.db")
            })
        })
        .expect("sessions entry");
    assert!(
        sessions
            .get(MANIFEST_FILE_COUNT_FIELD)
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|count| count > 0),
        "sessions entry should record file_count"
    );
    assert_eq!(
        sessions
            .get(MANIFEST_RESTORE_PATH_FIELD)
            .and_then(serde_json::Value::as_str),
        Some("data/sessions.db")
    );
}

#[test]
fn create_backup_copies_runtime_stores_and_knowledge_cohorts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();
    write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

    make_fjall_store(
        &instance_root
            .join("data")
            .join("knowledge.fjall")
            .join("shared"),
    );
    make_fjall_store(
        &instance_root
            .join("data")
            .join("knowledge.fjall")
            .join("identity"),
    );
    make_fjall_store(&instance_root.join("data").join("sessions.db"));
    make_fjall_store(&instance_root.join("data").join("auth.fjall"));
    make_fjall_store(
        &instance_root
            .join("data")
            .join("daemon-task-state")
            .join("system"),
    );
    make_fjall_store(
        &instance_root
            .join("data")
            .join("daemon-task-state")
            .join("alice"),
    );
    make_fjall_store(&instance_root.join("data").join("cron-locks.fjall"));

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

    assert_fjall_marker(&backup_path, &["stores", "knowledge.fjall", "shared"]);
    assert_fjall_marker(&backup_path, &["stores", "knowledge.fjall", "identity"]);
    assert_fjall_marker(&backup_path, &["stores", "auth.fjall"]);
    assert_fjall_marker(&backup_path, &["stores", "daemon-task-state", "system"]);
    assert_fjall_marker(&backup_path, &["stores", "cron-locks.fjall"]);

    let manifest: BackupManifest =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();
    for name in ["auth.fjall", "daemon-task-state", "cron-locks.fjall"] {
        assert_optional_store(&manifest, name);
    }

    let verify = InstanceBackup::verify_backup(&backup_path).unwrap();
    assert!(
        verify.first_error.is_none(),
        "complete runtime backup should verify: {:?}",
        verify.first_error
    );
    assert_generations(
        &verify,
        &[
            "knowledge.fjall/shared",
            "knowledge.fjall/identity",
            "sessions.db",
            "auth.fjall",
            "daemon-task-state/system",
            "daemon-task-state/alice",
            "cron-locks.fjall",
        ],
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "single fixture covers #5139 workspace classes and duplicate handling"
)]
fn create_backup_records_configured_agent_workspaces() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();

    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    make_fjall_store(&instance_root.join("data").join("sessions.db"));

    let relative_source = instance_root.join("workspaces").join("relative");
    let inside_source = instance_root.join("workspaces").join("inside");
    let outside_source = tmp.path().join("outside");
    let duplicate_source = instance_root.join("workspaces").join("duplicate");
    for path in [
        &relative_source,
        &inside_source,
        &outside_source,
        &duplicate_source,
    ] {
        fs::create_dir_all(path).unwrap();
        write_text_file(&path.join("NOTE.md"), "workspace").unwrap();
    }

    let config_toml = format!(
        r#"
[[agents.list]]
id = "alice"
workspace = "workspaces/relative"

[[agents.list]]
id = "bob"
workspace = "{}"

[[agents.list]]
id = "carol"
workspace = "{}"

[[agents.list]]
id = "dana"
workspace = "workspaces/missing"

[[agents.list]]
id = "erin"
workspace = "{}"

[[agents.list]]
id = "frank"
workspace = "{}"
"#,
        inside_source.display(),
        outside_source.display(),
        duplicate_source.display(),
        duplicate_source.display()
    );
    write_text_file(
        &instance_root.join("config").join("aletheia.toml"),
        &config_toml,
    )
    .unwrap();

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
    let manifest: BackupManifest =
        serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
            .unwrap();

    let entry_for = |agent_id: &str| {
        manifest
            .optional_stores
            .iter()
            .find(|entry| entry.agent_id.as_deref() == Some(agent_id))
            .unwrap_or_else(|| panic!("missing workspace entry for {agent_id}"))
    };

    let alice = entry_for("alice");
    assert_eq!(alice.status, "ok");
    assert_eq!(alice.workspace_source_class.as_deref(), Some("in-root"));
    assert_eq!(
        alice.backup_path,
        PathBuf::from("workspace").join("configured").join("alice")
    );
    assert!(
        backup_path
            .join(&alice.backup_path)
            .join("NOTE.md")
            .is_file()
    );

    let bob = entry_for("bob");
    assert_eq!(bob.status, "ok");
    assert_eq!(
        bob.workspace_source_class.as_deref(),
        Some("absolute-inside-root")
    );
    assert_eq!(
        bob.backup_path,
        PathBuf::from("workspace").join("configured").join("bob")
    );
    assert!(backup_path.join(&bob.backup_path).join("NOTE.md").is_file());

    let carol = entry_for("carol");
    assert_eq!(carol.status, "excluded");
    assert_eq!(
        carol.workspace_source_class.as_deref(),
        Some("absolute-outside-root")
    );
    assert!(
        carol
            .exclusion_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("outside"))
    );

    let dana = entry_for("dana");
    assert_eq!(dana.status, "excluded");
    assert_eq!(dana.workspace_source_class.as_deref(), Some("in-root"));
    assert!(
        dana.exclusion_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("missing"))
    );

    let erin = entry_for("erin");
    let frank = entry_for("frank");
    assert_eq!(erin.status, "ok");
    assert_eq!(frank.status, "ok");
    assert_ne!(&frank.backup_path, &erin.backup_path);
    assert!(
        backup_path
            .join(&erin.backup_path)
            .join("NOTE.md")
            .is_file()
    );
    assert!(
        backup_path
            .join(&frank.backup_path)
            .join("NOTE.md")
            .is_file()
    );

    // WHY(#4950): excluded entries are intentional policy omissions, not
    // verification failures, so the published backup set must verify cleanly.
    let verify = InstanceBackup::verify_backup(&backup_path).unwrap();
    assert!(
        verify.first_error.is_none(),
        "excluded entries should not fail verification: {:?}",
        verify.first_error
    );
}

#[cfg(unix)]
fn assert_backup_symlink_rejected<T>(
    result: error::Result<T>,
    expected_relative_path: &str,
    expected_source_root: &Path,
) {
    let msg = match result {
        Ok(_) => panic!("symlink traversal should be rejected"),
        Err(err) => err.to_string(),
    };
    assert!(
        msg.contains("symbolic link"),
        "error should identify symlink policy: {msg}"
    );
    assert!(
        msg.contains(expected_relative_path),
        "error should include relative path {expected_relative_path:?}: {msg}"
    );
    assert!(
        msg.contains("source root") && msg.contains(&expected_source_root.display().to_string()),
        "error should include source root {}: {msg}",
        expected_source_root.display()
    );
}

#[cfg(unix)]
#[test]
fn copy_path_rejects_symlink_to_outside_instance_4952() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source_root = tmp.path().join("instance");
    let workspace = source_root.join("nous").join("alice");
    fs::create_dir_all(&workspace).unwrap();
    write_text_file(&workspace.join("NOTE.md"), "safe").unwrap();

    let outside_dir = tmp.path().join("outside");
    fs::create_dir_all(&outside_dir).unwrap();
    write_text_file(&outside_dir.join("secret.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(outside_dir.join("secret.txt"), workspace.join("leak.txt")).unwrap();

    let dst = tmp.path().join("backup-copy");
    assert_backup_symlink_rejected(
        copy_path(&source_root, &dst),
        "nous/alice/leak.txt",
        &source_root,
    );
    assert!(
        !dst.exists(),
        "pre-walk rejection must not leave a partial destination"
    );
}

#[cfg(unix)]
#[test]
fn copy_path_rejects_symlink_loop_4952() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source_root = tmp.path().join("source");
    fs::create_dir_all(&source_root).unwrap();
    write_text_file(&source_root.join("real.txt"), "safe").unwrap();
    std::os::unix::fs::symlink(".", source_root.join("loop")).unwrap();

    let dst = tmp.path().join("backup-copy");
    assert_backup_symlink_rejected(copy_path(&source_root, &dst), "loop", &source_root);
    assert!(
        !dst.exists(),
        "pre-walk rejection must not leave a partial destination"
    );
}

#[cfg(unix)]
#[test]
fn copy_path_rejects_symlink_to_file_4952() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source_root = tmp.path().join("source");
    fs::create_dir_all(&source_root).unwrap();
    write_text_file(&source_root.join("real.txt"), "safe").unwrap();
    std::os::unix::fs::symlink("real.txt", source_root.join("link.txt")).unwrap();

    let dst = tmp.path().join("backup-copy");
    assert_backup_symlink_rejected(copy_path(&source_root, &dst), "link.txt", &source_root);
    assert!(
        !dst.exists(),
        "pre-walk rejection must not leave a partial destination"
    );
}

#[cfg(unix)]
#[test]
fn copy_path_rejects_internal_directory_symlink_4952() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source_root = tmp.path().join("source");
    let target = source_root.join("target");
    fs::create_dir_all(&target).unwrap();
    write_text_file(&target.join("NOTE.md"), "safe").unwrap();
    std::os::unix::fs::symlink("target", source_root.join("target-link")).unwrap();

    let dst = tmp.path().join("backup-copy");
    assert_backup_symlink_rejected(copy_path(&source_root, &dst), "target-link", &source_root);
    assert!(
        !dst.exists(),
        "pre-walk rejection must not leave a partial destination"
    );
}
