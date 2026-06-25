#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on a known-length collection"
)]

use std::io::Write as _;

use super::*;

fn training_dir(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    tmp.path().join("training")
}

fn write_shard(path: &std::path::Path, lines: &[&str]) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("open shard");
    for line in lines {
        file.write_all(line.as_bytes()).expect("write line");
        file.write_all(b"\n").expect("write newline");
    }
}

fn default_config() -> TrainingConfig {
    TrainingConfig {
        enabled: true,
        path: "training".to_owned(),
        max_shard_bytes: 50 * 1024 * 1024,
        pii_filter_enabled: false,
        author_classifier_enabled: false,
        author_classifier_threshold: 0.85,
    }
}

#[test]
fn reconcile_stale_manifest_counts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = training_dir(&tmp);
    std::fs::create_dir_all(&dir).expect("mkdir");

    let shard_path = dir.join("training-20260101-0001.jsonl");
    write_shard(
        &shard_path,
        &[
            r#"{"schema_version":5,"session_id":"s1","nous_id":"n","user_message":"a","assistant_response":"b","model":"m","tokens":1,"timestamp":"1970-01-01T00:00:00Z"}"#,
            r#"{"schema_version":5,"session_id":"s2","nous_id":"n","user_message":"c","assistant_response":"d","model":"m","tokens":1,"timestamp":"1970-01-01T00:00:00Z"}"#,
            r#"{"schema_version":5,"session_id":"s3","nous_id":"n","user_message":"e","assistant_response":"f","model":"m","tokens":1,"timestamp":"1970-01-01T00:00:00Z"}"#,
        ],
    );

    // Stale manifest under-counts the shard.
    let stale_manifest = TrainingManifest {
        shards: vec![ShardEntry {
            file_name: "training-20260101-0001.jsonl".to_owned(),
            record_count: 1,
            size_bytes: 0,
        }],
        total_records: 1,
        schema_version_min: 5,
        schema_version_max: 5,
    };
    let manifest_path = dir.join("training-manifest.json");
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&manifest_path)
            .expect("open manifest");
        f.write_all(
            serde_json::to_string_pretty(&stale_manifest)
                .expect("serialize")
                .as_bytes(),
        )
        .expect("write manifest");
    }

    let capture = TrainingCapture::new(tmp.path(), &default_config()).expect("init");
    assert_eq!(capture.manifest().total_records, 3);
    assert_eq!(capture.manifest().shards[0].record_count, 3);
}

#[test]
fn reconcile_orphan_rotated_shard() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = training_dir(&tmp);
    std::fs::create_dir_all(&dir).expect("mkdir");

    // A rotated shard exists without any manifest.
    let shard_path = dir.join("training-20260101-0002.jsonl");
    write_shard(
        &shard_path,
        &[
            r#"{"schema_version":5,"session_id":"s1","nous_id":"n","user_message":"a","assistant_response":"b","model":"m","tokens":1,"timestamp":"1970-01-01T00:00:00Z"}"#,
            r#"{"schema_version":5,"session_id":"s2","nous_id":"n","user_message":"c","assistant_response":"d","model":"m","tokens":1,"timestamp":"1970-01-01T00:00:00Z"}"#,
        ],
    );

    let capture = TrainingCapture::new(tmp.path(), &default_config()).expect("init");
    assert!(
        capture
            .manifest()
            .shards
            .iter()
            .any(|s| s.file_name == "training-20260101-0002.jsonl"),
        "orphan shard must be adopted into manifest"
    );
    assert_eq!(capture.manifest().total_records, 2);
}

#[test]
fn corrupt_manifest_surfaces_as_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = training_dir(&tmp);
    std::fs::create_dir_all(&dir).expect("mkdir");

    let manifest_path = dir.join("training-manifest.json");
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&manifest_path)
            .expect("open manifest");
        f.write_all(b"this is not valid json")
            .expect("write garbage");
    }

    let result = TrainingCapture::new(tmp.path(), &default_config());
    assert!(
        matches!(result, Err(TrainingCaptureError::CorruptManifest { .. })),
        "corrupt manifest must fail loudly, not silently reset"
    );
}

#[test]
fn reconcile_after_record_append_without_manifest_persist() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = training_dir(&tmp);
    std::fs::create_dir_all(&dir).expect("mkdir");

    // Simulate a crash: a record was appended to the shard, but the
    // manifest was not updated afterwards.
    let shard_path = dir.join("training-20260101-0001.jsonl");
    write_shard(
        &shard_path,
        &[
            r#"{"schema_version":5,"session_id":"s1","nous_id":"n","user_message":"a","assistant_response":"b","model":"m","tokens":1,"timestamp":"1970-01-01T00:00:00Z"}"#,
        ],
    );
    let stale_manifest = TrainingManifest {
        shards: vec![ShardEntry {
            file_name: "training-20260101-0001.jsonl".to_owned(),
            record_count: 0,
            size_bytes: 0,
        }],
        total_records: 0,
        schema_version_min: 5,
        schema_version_max: 5,
    };
    let manifest_path = dir.join("training-manifest.json");
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&manifest_path)
            .expect("open manifest");
        f.write_all(
            serde_json::to_string_pretty(&stale_manifest)
                .expect("serialize")
                .as_bytes(),
        )
        .expect("write manifest");
    }

    let capture = TrainingCapture::new(tmp.path(), &default_config()).expect("init");
    assert_eq!(capture.manifest().total_records, 1);
}
