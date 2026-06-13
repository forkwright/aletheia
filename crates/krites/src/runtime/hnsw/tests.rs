//! Durability tests for HNSW persistence infrastructure.
//!
//! These tests exercise [`atomic_save`] and [`mmap_storage`] together to show
//! that persisted index state and vector data survive close/reopen cycles and
//! that corrupted or interrupted writes are detected rather than silently
//! returned as valid data.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use super::atomic_save::{atomic_save_state, load_state};
use super::mmap_storage::MmapVectorStorage;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct FakeIndexState {
    next_id: u64,
    dim: usize,
}

#[test]
fn persisted_hnsw_state_survives_restart() {
    let dir = tempfile::tempdir()
        .unwrap_or_else(|_| unreachable!("INVARIANT: temp dir creation should not fail in tests"));
    let state_path = dir.path().join("state.msgpack");
    let vectors_path = dir.path().join("vectors.bin");

    let state = FakeIndexState { next_id: 7, dim: 3 };
    atomic_save_state(&state_path, &state)
        .unwrap_or_else(|_| unreachable!("INVARIANT: atomic save of index state should not fail"));

    {
        let mut storage = MmapVectorStorage::open(&vectors_path, 3)
            .unwrap_or_else(|_| unreachable!("INVARIANT: valid path and dim=3 should not fail"));
        storage.push(&[1.0f32, 2.0, 3.0]).unwrap_or_else(|_| {
            unreachable!("INVARIANT: push with correct dimension should not fail")
        });
        storage.push(&[4.0f32, 5.0, 6.0]).unwrap_or_else(|_| {
            unreachable!("INVARIANT: push with correct dimension should not fail")
        });
        storage
            .flush()
            .unwrap_or_else(|_| unreachable!("INVARIANT: flush should not fail on valid storage"));
    }

    let loaded: Option<FakeIndexState> = load_state(&state_path)
        .unwrap_or_else(|_| unreachable!("INVARIANT: load of existing state should not fail"));
    assert_eq!(loaded, Some(state), "index state survives restart");

    let storage = MmapVectorStorage::open(&vectors_path, 3).unwrap_or_else(|_| {
        unreachable!("INVARIANT: valid path and dim=3 should not fail on reopen")
    });
    assert_eq!(storage.len(), 2, "persisted vector count");
    assert_eq!(
        storage.get(0),
        Some(&[1.0f32, 2.0, 3.0][..]),
        "persisted vector 0 roundtrip"
    );
    assert_eq!(
        storage.get(1),
        Some(&[4.0f32, 5.0, 6.0][..]),
        "persisted vector 1 roundtrip"
    );
}

#[test]
fn interrupted_atomic_write_preserves_prior_state() {
    let dir = tempfile::tempdir()
        .unwrap_or_else(|_| unreachable!("INVARIANT: temp dir creation should not fail in tests"));
    let target = dir.path().join("state.msgpack");

    let initial = FakeIndexState { next_id: 1, dim: 2 };
    atomic_save_state(&target, &initial)
        .unwrap_or_else(|_| unreachable!("INVARIANT: initial atomic save should not fail"));

    // Simulate an interrupted write by leaving a partial temp file.
    let mut temp_os = target.as_os_str().to_owned();
    temp_os.push(".tmp");
    let temp = PathBuf::from(temp_os);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp)
        .unwrap_or_else(|_| unreachable!("INVARIANT: open for temp write must succeed"));
    file.write_all(&[0x93, 0x01])
        .unwrap_or_else(|_| unreachable!("INVARIANT: temp write must succeed"));

    let loaded: Option<FakeIndexState> = load_state(&target)
        .unwrap_or_else(|_| unreachable!("INVARIANT: load of existing state should not fail"));
    assert_eq!(
        loaded,
        Some(initial),
        "prior state survives interrupted write"
    );

    let next = FakeIndexState { next_id: 2, dim: 2 };
    atomic_save_state(&target, &next)
        .unwrap_or_else(|_| unreachable!("INVARIANT: recovery atomic save should not fail"));
    let loaded: Option<FakeIndexState> = load_state(&target)
        .unwrap_or_else(|_| unreachable!("INVARIANT: load of recovered state should not fail"));
    assert_eq!(loaded, Some(next), "recovery write replaces state");
    assert!(!temp.exists(), "temp file cleaned up after recovery");
}

#[test]
fn truncated_vector_storage_is_detected() {
    let dir = tempfile::tempdir()
        .unwrap_or_else(|_| unreachable!("INVARIANT: temp dir creation should not fail in tests"));
    let path = dir.path().join("vectors.bin");

    {
        let mut storage = MmapVectorStorage::open(&path, 2)
            .unwrap_or_else(|_| unreachable!("INVARIANT: valid path and dim=2 should not fail"));
        storage.push(&[1.0f32, 2.0]).unwrap_or_else(|_| {
            unreachable!("INVARIANT: push with correct dimension should not fail")
        });
        storage
            .flush()
            .unwrap_or_else(|_| unreachable!("INVARIANT: flush should not fail on valid storage"));
    }

    // Corrupt the file by truncating it to a size that is not a multiple of
    // the vector stride.
    let file = OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap_or_else(|_| unreachable!("INVARIANT: open for truncate must succeed"));
    file.set_len(1)
        .unwrap_or_else(|_| unreachable!("INVARIANT: truncate must succeed"));

    let result = MmapVectorStorage::open(&path, 2);
    assert!(result.is_err(), "corrupted file size must be rejected");
}
