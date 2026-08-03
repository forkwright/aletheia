//! Backup freshness derived from persisted state rather than attempt counters. (#6445)

use std::sync::Mutex;

use super::*;

/// Captures what the daemon publishes, so each case asserts on the exact
/// triple an exporter would serve.
#[derive(Debug, Default)]
struct CapturingRecorder {
    state: Mutex<Vec<(Option<i64>, bool, u64)>>,
}

impl CapturingRecorder {
    fn last(&self) -> (Option<i64>, bool, u64) {
        *self
            .state
            .lock()
            .unwrap()
            .last()
            .expect("record_backup_state was never called")
    }
}

impl crate::maintenance::BackupMetricsRecorder for CapturingRecorder {
    fn record_backup_duration(&self, _duration_secs: f64, _success: bool) {}

    fn record_backup_state(
        &self,
        last_success_unixtime: Option<i64>,
        enabled: bool,
        interval_secs: u64,
    ) {
        self.state
            .lock()
            .unwrap()
            .push((last_success_unixtime, enabled, interval_secs));
    }
}

fn seeded_instance(root: &Path) {
    fs::create_dir_all(root.join("data")).unwrap();
    fs::create_dir_all(root.join("config")).unwrap();
    write_text_file(&root.join("config").join("aletheia.toml"), "test").unwrap();
    make_fjall_store(&root.join("data").join("knowledge.fjall"));
    make_fjall_store(&root.join("data").join("sessions.db"));
}

fn config_for(tmp: &Path, interval_hours: u64) -> InstanceBackupConfig {
    let instance_root = tmp.join("instance");
    seeded_instance(&instance_root);
    InstanceBackupConfig {
        enabled: true,
        instance_root,
        backup_dir: tmp.join("backups"),
        interval_hours,
        retention_count: 7,
        additional_workspaces: Vec::new(),
    }
}

/// Rewrite a backup set's `created_at` so age can be asserted without waiting.
/// The manifest stays structurally valid — only the timestamp moves.
fn set_created_at(backup_path: &Path, timestamp: &str) {
    let manifest_path = backup_path.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    value
        .as_object_mut()
        .expect("manifest is a JSON object")
        .insert(
            "created_at".to_owned(),
            serde_json::Value::String(timestamp.to_owned()),
        );
    write_text_file(&manifest_path, &serde_json::to_string(&value).unwrap()).unwrap();
}

fn publish(config: &InstanceBackupConfig) -> (Option<i64>, bool, u64) {
    let recorder = CapturingRecorder::default();
    super::super::publish_backup_state(config, &recorder);
    recorder.last()
}

#[test]
fn first_boot_with_no_backup_publishes_a_defined_absence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_for(tmp.path(), 24);

    // NOTE: the backup directory does not exist yet. The old counter-based
    // rule exported nothing here; the contract now is an explicit "none".
    let (last, enabled, interval) = publish(&config);
    assert_eq!(
        last, None,
        "no backup set must publish None, not a stale value"
    );
    assert!(enabled);
    assert_eq!(interval, 24 * 3600);
}

#[test]
fn a_published_backup_is_reported_at_its_manifest_time() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_for(tmp.path(), 24);
    let backup_path = InstanceBackup::new(config.clone())
        .create_backup()
        .expect("backup succeeds")
        .backup_path
        .expect("backup path set");
    set_created_at(&backup_path, "2026-07-01T00:00:00Z[UTC]");

    // WHY: published by a fresh scan, with no backup having run in this
    // process — exactly the post-restart case the counter could not answer.
    let (last, _, _) = publish(&config);
    assert_eq!(last, Some(1_782_864_000));
}

#[test]
fn a_stale_backup_reports_its_real_age_rather_than_absence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_for(tmp.path(), 24);
    let backup_path = InstanceBackup::new(config.clone())
        .create_backup()
        .expect("backup succeeds")
        .backup_path
        .expect("backup path set");
    set_created_at(&backup_path, "2025-01-01T00:00:00Z[UTC]");

    // WHY: a long-stale backup must publish its true timestamp, not None. The
    // distinction matters to alerting — None means "no recovery point exists"
    // (BackupMissing), an old timestamp means "one exists but is overdue"
    // (BackupStale). Collapsing them would lose that.
    let (last, _, _) = publish(&config);
    assert_eq!(last, Some(1_735_689_600));
}

#[test]
fn the_newest_backup_wins_when_an_older_set_is_present() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_for(tmp.path(), 24);
    let manager = InstanceBackup::new(config.clone());

    let older = manager
        .create_backup()
        .expect("first backup")
        .backup_path
        .expect("path");
    set_created_at(&older, "2026-01-01T00:00:00Z[UTC]");
    let newer = manager
        .create_backup()
        .expect("second backup")
        .backup_path
        .expect("path");
    set_created_at(&newer, "2026-07-01T00:00:00Z[UTC]");

    let (last, _, _) = publish(&config);
    assert_eq!(
        last,
        Some(1_782_864_000),
        "freshness must track the newest set, not the first or last written"
    );
}

#[test]
fn a_malformed_manifest_never_reads_as_a_fresh_backup() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_for(tmp.path(), 24);
    let backup_path = InstanceBackup::new(config.clone())
        .create_backup()
        .expect("backup succeeds")
        .backup_path
        .expect("backup path set");

    // WHY: a truncated manifest is the crash-during-write case. Counting it as
    // a backup would suppress the alert precisely when recovery is in doubt.
    write_text_file(&backup_path.join("manifest.json"), "{\"created_at\":").unwrap();

    let (last, _, _) = publish(&config);
    assert_eq!(last, None);
}

#[test]
fn an_in_progress_staging_directory_is_not_a_backup() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_for(tmp.path(), 24);
    let staging = config
        .backup_dir
        .join(format!("{STAGING_DIR_PREFIX}inflight"));
    fs::create_dir_all(&staging).unwrap();
    write_text_file(
        &staging.join("manifest.json"),
        &serde_json::json!({"created_at": "2026-07-01T00:00:00Z[UTC]"}).to_string(),
    )
    .unwrap();

    let (last, _, _) = publish(&config);
    assert_eq!(
        last, None,
        "a set is only a backup once it is renamed into place"
    );
}

#[test]
fn a_disabled_backup_config_publishes_enabled_false() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut config = config_for(tmp.path(), 24);
    config.enabled = false;

    let (_, enabled, _) = publish(&config);
    assert!(
        !enabled,
        "an instance that opted out must be able to suppress the alert"
    );
}

#[test]
fn a_non_default_cadence_is_published_verbatim() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_for(tmp.path(), 6);

    // The rule multiplies this gauge rather than assuming 48h, so a shorter
    // cadence must reach the exporter unrounded.
    let (_, _, interval) = publish(&config);
    assert_eq!(interval, 6 * 3600);
}
