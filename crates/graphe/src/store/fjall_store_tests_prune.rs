#![expect(clippy::expect_used, reason = "test assertions")]

use crate::test_fixtures::test_store;

#[test]
fn prune_session_archives_removes_stale_files() {
    use std::time::{Duration, SystemTime};

    let store = test_store();
    let archive_dir = store
        .path()
        .parent()
        .expect("store path has parent")
        .join("archive")
        .join("sessions");
    let stale = archive_dir.join("stale.json");
    let recent = archive_dir.join("recent.json");

    // WHY: a fixed epoch far in the past is guaranteed to be older than
    // any 7-day TTL, while `SystemTime::now()` is guaranteed to be newer.
    let past = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_archive_file(&stale, past);
    write_archive_file(&recent, SystemTime::now());

    let removed = store.prune_session_archives(7).expect("prune");
    assert_eq!(removed, 1, "one stale archive should be removed");
    assert!(!stale.exists(), "stale archive should be deleted");
    assert!(recent.exists(), "recent archive should remain");
}

#[test]
fn prune_session_archives_leaves_recent_files() {
    use std::time::SystemTime;

    let store = test_store();
    let archive_dir = store
        .path()
        .parent()
        .expect("store path has parent")
        .join("archive")
        .join("sessions");
    let recent = archive_dir.join("recent.json");

    write_archive_file(&recent, SystemTime::now());

    let removed = store.prune_session_archives(7).expect("prune");
    assert_eq!(removed, 0, "recent archives must not be pruned");
    assert!(recent.exists(), "recent archive should remain");
}

#[test]
fn prune_session_archives_no_dir_returns_zero() {
    let store = test_store();
    let removed = store.prune_session_archives(7).expect("prune");
    assert_eq!(removed, 0, "missing archive dir yields zero removals");
}

#[test]
fn prune_session_archives_skips_non_json() {
    use std::time::{Duration, SystemTime};

    let store = test_store();
    let archive_dir = store
        .path()
        .parent()
        .expect("store path has parent")
        .join("archive")
        .join("sessions");
    let old_json = archive_dir.join("old.json");
    let old_txt = archive_dir.join("old.txt");

    let past = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_archive_file(&old_json, past);
    write_archive_file(&old_txt, past);

    let removed = store.prune_session_archives(7).expect("prune");
    assert_eq!(removed, 1, "only json files should be pruned");
    assert!(!old_json.exists(), "stale json archive should be deleted");
    assert!(old_txt.exists(), "non-json files should be ignored");
}

fn write_archive_file(path: &std::path::Path, modified: std::time::SystemTime) {
    let parent = path.parent().expect("archive file has parent");
    std::fs::create_dir_all(parent).expect("create archive dir");
    let file = std::fs::File::create(path).expect("create archive file");
    let times = std::fs::FileTimes::new().set_modified(modified);
    file.set_times(times).expect("set archive file mtime");
}
