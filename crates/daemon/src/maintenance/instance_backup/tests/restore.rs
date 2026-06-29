use super::*;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "WHY(#4951): one behavioral fixture proves restore covers required stores plus optional archive/audit/log entries"
)]
fn restore_backup_restores_all_manifest_entries_by_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    fs::create_dir_all(instance_root.join("data")).unwrap();
    fs::create_dir_all(instance_root.join("config")).unwrap();

    make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
    make_fjall_store(&instance_root.join("data").join("sessions.db"));
    write_text_file(
        &instance_root.join("config").join("aletheia.toml"),
        "original",
    )
    .unwrap();
    write_text_file(
        &instance_root.join("nous").join("alice").join("SOUL.md"),
        "soul",
    )
    .unwrap();
    write_text_file(&instance_root.join("shared").join("NOTE.md"), "shared").unwrap();
    write_text_file(&instance_root.join("theke").join("Page.md"), "theke").unwrap();
    write_text_file(
        &instance_root
            .join("data")
            .join("archive")
            .join("sessions")
            .join("alice.json"),
        "archive",
    )
    .unwrap();
    write_text_file(
        &instance_root
            .join("data")
            .join("prosoche-audits")
            .join("audit.json"),
        "prosoche",
    )
    .unwrap();
    write_text_file(
        &instance_root
            .join("data")
            .join("prompt-audit")
            .join("data.log"),
        "prompt-data",
    )
    .unwrap();
    write_text_file(
        &instance_root
            .join("logs")
            .join("prompt-audit")
            .join("llm.log"),
        "prompt-log",
    )
    .unwrap();

    let backup_dir = tmp.path().join("backups");
    let manager = InstanceBackup::new(InstanceBackupConfig {
        enabled: true,
        instance_root: instance_root.clone(),
        backup_dir,
        interval_hours: 24,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    });
    let report = manager.create_backup().expect("backup succeeds");
    let backup_path = report.backup_path.expect("backup path set");

    fs::remove_dir_all(instance_root.join("config")).unwrap();
    fs::remove_dir_all(instance_root.join("nous")).unwrap();
    fs::remove_dir_all(instance_root.join("shared")).unwrap();
    fs::remove_dir_all(instance_root.join("theke")).unwrap();
    fs::remove_dir_all(instance_root.join("data").join("archive")).unwrap();
    fs::remove_dir_all(instance_root.join("data").join("prosoche-audits")).unwrap();
    fs::remove_dir_all(instance_root.join("data").join("prompt-audit")).unwrap();
    fs::remove_dir_all(instance_root.join("logs")).unwrap();
    write_text_file(
        &instance_root.join("config").join("aletheia.toml"),
        "mutated",
    )
    .unwrap();
    write_text_file(
        &instance_root.join("nous").join("alice").join("SOUL.md"),
        "mutated",
    )
    .unwrap();
    write_text_file(
        &instance_root
            .join("data")
            .join("archive")
            .join("sessions")
            .join("alice.json"),
        "mutated",
    )
    .unwrap();

    let restore = manager
        .restore_backup(&InstanceRestoreOptions {
            backup_path: backup_path.clone(),
            force_live: true,
            include: Vec::new(),
            exclude: Vec::new(),
        })
        .expect("restore succeeds");

    assert!(restore.entries_restored >= 9, "restore report: {restore:?}");
    assert!(
        restore.live_entries_replaced >= 3,
        "restore should replace mutated live entries: {restore:?}"
    );
    assert_eq!(
        fs::read_to_string(instance_root.join("config").join("aletheia.toml")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(instance_root.join("nous").join("alice").join("SOUL.md")).unwrap(),
        "soul"
    );
    assert_eq!(
        fs::read_to_string(instance_root.join("shared").join("NOTE.md")).unwrap(),
        "shared"
    );
    assert_eq!(
        fs::read_to_string(instance_root.join("theke").join("Page.md")).unwrap(),
        "theke"
    );
    assert_eq!(
        fs::read_to_string(
            instance_root
                .join("data")
                .join("archive")
                .join("sessions")
                .join("alice.json")
        )
        .unwrap(),
        "archive"
    );
    assert_eq!(
        fs::read_to_string(
            instance_root
                .join("data")
                .join("prosoche-audits")
                .join("audit.json")
        )
        .unwrap(),
        "prosoche"
    );
    assert_eq!(
        fs::read_to_string(
            instance_root
                .join("data")
                .join("prompt-audit")
                .join("data.log")
        )
        .unwrap(),
        "prompt-data"
    );
    assert_eq!(
        fs::read_to_string(
            instance_root
                .join("logs")
                .join("prompt-audit")
                .join("llm.log")
        )
        .unwrap(),
        "prompt-log"
    );
    assert_fjall_marker(&instance_root, &["data", "knowledge.fjall"]);
    assert_fjall_marker(&instance_root, &["data", "sessions.db"]);
}

#[test]
fn rollback_restore_restores_moved_live_entry_after_publish_failure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");
    let rollback_root = tmp.path().join("rollback");
    fs::create_dir_all(&live_root).unwrap();
    fs::create_dir_all(&staging_root).unwrap();
    fs::create_dir_all(&rollback_root).unwrap();
    write_text_file(&live_root.join("one.txt"), "live-one").unwrap();
    write_text_file(&live_root.join("two.txt"), "live-two").unwrap();
    write_text_file(&staging_root.join("one.txt"), "restored-one").unwrap();

    let entries = vec![
        RestorePlanEntry {
            name: String::from("one"),
            backup_path: PathBuf::from("one.txt"),
            backup_source: staging_root.join("one.txt"),
            target_rel: PathBuf::from("one.txt"),
            target_path: live_root.join("one.txt"),
            byte_count: 12,
            file_count: 1,
            sha256: None,
        },
        RestorePlanEntry {
            name: String::from("two"),
            backup_path: PathBuf::from("two.txt"),
            backup_source: staging_root.join("two.txt"),
            target_rel: PathBuf::from("two.txt"),
            target_path: live_root.join("two.txt"),
            byte_count: 8,
            file_count: 1,
            sha256: None,
        },
    ];
    let mut rollback_entries = Vec::new();

    let publish_error = publish_restore_plan(
        &entries,
        &staging_root,
        &rollback_root,
        &mut rollback_entries,
    )
    .expect_err("missing staged entry should fail publish");
    assert!(
        publish_error
            .to_string()
            .contains("staged restore entry missing"),
        "unexpected publish error: {publish_error}"
    );

    rollback_restore(&rollback_entries).expect("rollback succeeds");

    assert_eq!(
        fs::read_to_string(live_root.join("one.txt")).unwrap(),
        "live-one"
    );
    assert_eq!(
        fs::read_to_string(live_root.join("two.txt")).unwrap(),
        "live-two"
    );
}
