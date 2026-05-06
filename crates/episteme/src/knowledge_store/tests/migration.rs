#![expect(clippy::expect_used, reason = "test setup failures should panic")]

use std::io::Write;

use super::super::KnowledgeStore;

#[test]
fn open_fjall_copies_legacy_root_into_shared_cohort() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy_root = dir.path().join("knowledge.fjall");
    let legacy_partition = legacy_root.join("relations");
    std::fs::create_dir_all(&legacy_partition).expect("create legacy partition");
    let mut marker = std::fs::File::create(legacy_partition.join("marker")).expect("create marker");
    marker.write_all(b"legacy").expect("write marker");

    let shared = legacy_root.join("shared");
    KnowledgeStore::migrate_to_cohort_layout(&shared).expect("migrate shared cohort");

    let migrated_marker = shared.join("relations").join("marker");
    assert_eq!(
        std::fs::metadata(&migrated_marker)
            .expect("stat migrated marker")
            .len(),
        6
    );
}
