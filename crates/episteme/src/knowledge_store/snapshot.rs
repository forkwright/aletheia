//! Pre-migration verified snapshot (aletheia#5779, plan §8.5).
//!
//! Placement is load-bearing. Migrations run after
//! [`super::KnowledgeStore::open_fjall`] already holds the fjall lock
//! (`knowledge_store/mod.rs`), so a snapshot function living inside the
//! migration path cannot re-open the source with a clean, verification-only
//! handle — it would either hit a lock failure, or worse, an implementer
//! working around the lock by opening the *live* keyspace a second way,
//! which is the auto-recovery hazard that has already cost the fleet ~600
//! records once (`fjall::Keyspace::open()` auto-recovery deletes segments
//! absent from the levels manifest). This module therefore:
//!
//! - runs strictly BEFORE the caller's long-lived `Db::open_fjall` handle is
//!   established (`KnowledgeStore::open_fjall`, `knowledge_store/mod.rs`),
//!   not inside `knowledge_store::migration`;
//! - takes the actual snapshot via a filesystem `cp -r`, never by opening
//!   `source` through fjall for that purpose;
//! - is, however, permitted brief, sequential, opened-then-immediately-dropped
//!   fjall reads of `source` for cheap point queries (the caller's
//!   pending-migration gate; this module's own pre-copy record count) —
//!   never a long-lived handle held concurrently with anything else. This is
//!   the same open-then-drop-then-reopen pattern this codebase already
//!   exercises deliberately (`agent_io.rs` reopen-durability tests) — the
//!   hazard being avoided is a *second* handle held *while* the real one is
//!   live, not a clean, non-overlapping open that finishes before the real
//!   one starts.
//!
//! **Verified, not copied-and-assumed.** A copy that returned `Ok` is not
//! "backed up" — that is exactly the claim the fleet's `preserve` doctrine
//! exists to refuse. [`pre_migration_snapshot`] takes a record count from
//! `source` before copying, and [`verify_restorable`] opens the copy with
//! zero background workers, asserts fjall's own on-disk marker is present
//! (never letting fjall's create-or-recover silently manufacture an empty
//! keyspace), and requires the restored count to equal the count taken
//! before the copy.
//!
//! **Write-new, verify, then replace.** The previous verified snapshot is
//! never deleted before its replacement exists: the copy lands in a
//! `<dir>.new` sibling, gets verified there, and only then replaces
//! `<dir>` — so a failed copy or a failed verify leaves the last known-good
//! snapshot untouched.

use std::path::{Path, PathBuf};

/// Path component name refused at any depth *below* a snapshot's root. The
/// psyche partition holds private identity-continuity content that must
/// never be duplicated outside its owning directory tree — not even into a
/// crash-recovery snapshot with a looser retention/access story
/// (`~/aletheia/instance/data/knowledge.fjall/shared/psyche/`).
///
/// This does **not** refuse a snapshot whose *root* is itself named
/// `psyche` — the psyche cohort's own store root is snapshotted like any
/// other cohort (aletheia#5779 F1: the local-only rule governs routing to
/// cloud models, not on-box filesystem copies; a root-level refusal here
/// made the psyche cohort's own pre-migration snapshot a silent no-op that
/// still reported success). A *nested* `psyche` directory found while
/// snapshotting a **different** cohort's tree is a distinct case — most
/// plausibly legacy content dragged along by
/// [`super::KnowledgeStore::migrate_to_cohort_layout`] before the cohort
/// split existed — and copying it would duplicate psyche-classified content
/// outside its owning tree under a foreign cohort's snapshot, which is
/// exactly what this per-descendant check still prevents.
const REFUSED_COMPONENT: &str = "psyche";

/// Name of fjall's single on-disk keyspace, matching
/// `krites/src/storage/fjall_backend.rs:69`.
#[cfg(feature = "storage-fjall")]
const DATA_KEYSPACE: &str = "data";

/// fjall's own version-marker filename. Not re-exported by the `fjall`
/// crate (`fjall::file::VERSION_MARKER` lives in a private `mod file;`,
/// `fjall-3.1.6/src/lib.rs:111`), so it is named here directly —
/// `fjall-3.1.6/src/file.rs:12`. Its absence is exactly what makes
/// `Database::open` (`== create_or_recover`,
/// `fjall-3.1.6/src/db.rs:403-414`) silently **create** a fresh empty
/// keyspace instead of opening an existing one; checking for it ourselves
/// before ever calling into fjall is what makes this module fail closed
/// instead of "verifying" an empty directory (aletheia#5779 F1/F2, proven by
/// compiling and running this exact sequence against fjall 3.1.6).
#[cfg(feature = "storage-fjall")]
const FJALL_VERSION_MARKER: &str = "version";

/// Recursively copy `source` into `dest`, refusing any path component
/// literally named `psyche` **below** `source` itself — not the root. See
/// [`REFUSED_COMPONENT`] for why the root is deliberately exempt. Symlinks
/// (and any other non-regular entry) are refused outright rather than
/// silently skipped: a store whose partition is symlinked to another volume
/// must not produce a snapshot that looks complete but is truncated.
///
/// # Errors
/// Returns an error if a filesystem operation fails partway through, or if
/// a non-regular filesystem entry (symlink, device node, ...) is
/// encountered anywhere in the tree. On error the destination may hold a
/// partial copy; callers must not treat a partial copy as a usable snapshot
/// (see [`pre_migration_snapshot`], which gates on a subsequent open-and-read
/// against a pre-copy record count, not on this function alone).
pub fn copy_excluding_psyche(source: &Path, dest: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let Some(name) = entry.path().file_name().map(std::ffi::OsStr::to_os_string) else {
                continue;
            };
            if name == REFUSED_COMPONENT {
                continue;
            }
            copy_excluding_psyche(&entry.path(), &dest.join(&name))?;
        }
        Ok(())
    } else if metadata.is_file() {
        std::fs::copy(source, dest).map(|_bytes| ())
    } else {
        // WARNING: do not turn this back into a silent skip. A symlinked
        // partition dir (e.g. mounted from another volume) would otherwise
        // yield a snapshot that verifies clean while missing everything
        // reachable only through the link (aletheia#5779 F2).
        Err(std::io::Error::other(format!(
            "refusing to copy non-regular filesystem entry at {}: a pre-migration snapshot must not silently truncate",
            source.display()
        )))
    }
}

/// Sibling path used as the staging target for a new copy — never the live
/// snapshot directory itself. See module docs: write-new, verify, then
/// replace.
#[cfg(feature = "storage-fjall")]
fn staging_sibling(snapshot_dir: &Path) -> PathBuf {
    let mut name = snapshot_dir
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".new");
    snapshot_dir.with_file_name(name)
}

/// Take a filesystem-level copy of a fjall knowledge-store directory and
/// verify it is genuinely restorable before promoting it into place: opens
/// the COPY (never `source`) with zero background workers and performs a
/// real full-scan read whose record count must equal a count taken from
/// `source` before the copy ran.
///
/// If `source` does not exist yet (first boot, store not yet created),
/// this is a no-op — there is nothing to protect. Callers are expected to
/// gate the (expensive) call itself on whether a schema migration might
/// actually run this boot — see
/// [`super::KnowledgeStore::open_fjall`] — so this function does not
/// re-derive that decision; it always does the full protective work when
/// asked to.
///
/// # Errors
/// Returns an error if the pre-copy count cannot be taken, if the copy
/// cannot be made, or if the copy cannot be opened and read back with a
/// record count matching the pre-copy count. "The copy call returned `Ok`"
/// is never, on its own, treated as "backed up" — that is exactly the claim
/// the fleet's `preserve` doctrine exists to refuse. On any failure the
/// prior verified snapshot at `snapshot_dir` (if one exists) is left
/// untouched.
#[cfg(feature = "storage-fjall")]
pub fn pre_migration_snapshot(source: &Path, snapshot_dir: &Path) -> crate::error::Result<PathBuf> {
    if !source.exists() {
        return Ok(snapshot_dir.to_path_buf());
    }

    let source_count = count_data_keyspace_rows(source)?;

    let staging_dir = staging_sibling(snapshot_dir);
    // WHY: clear a stale `.new` left by a prior crashed attempt — never the
    // live `snapshot_dir`. A leftover partial copy from an earlier crash
    // must not silently merge with this run's copy and pass verification on
    // mixed-generation data.
    let _ = std::fs::remove_dir_all(&staging_dir);

    copy_excluding_psyche(source, &staging_dir).map_err(|e| {
        crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot copy from {} to {} failed: {e}",
                source.display(),
                staging_dir.display()
            ),
        }
        .build()
    })?;

    verify_restorable(&staging_dir, source_count)?;

    // WHY: POSIX `rename(2)` cannot atomically replace a non-empty
    // directory (EEXIST/ENOTEMPTY) — an unconditional rename onto a live
    // `snapshot_dir` from a prior successful run would simply fail. Removing
    // the old snapshot first is safe here specifically because
    // `staging_dir` above has already been fully verified restorable: a
    // crash in the narrow window between this removal and the rename below
    // leaves `staging_dir` in place, still fully valid, and the "clear a
    // stale `.new`" step on the next attempt re-copies rather than
    // promotes it — one wasted pass, never data loss.
    let _ = std::fs::remove_dir_all(snapshot_dir);
    std::fs::rename(&staging_dir, snapshot_dir).map_err(|e| {
        crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot verified at {} but could not be promoted to {}: {e}",
                staging_dir.display(),
                snapshot_dir.display()
            ),
        }
        .build()
    })?;

    Ok(snapshot_dir.to_path_buf())
}

/// Count records in `dir`'s `"data"` fjall keyspace via a brief,
/// sequential, opened-then-immediately-dropped handle — never held
/// concurrently with anything else. Returns `0` if `dir` is not (yet) a
/// real fjall store, which is a legitimate state (e.g. a parent directory
/// created but never opened).
#[cfg(feature = "storage-fjall")]
fn count_data_keyspace_rows(dir: &Path) -> crate::error::Result<u64> {
    use fjall::Readable as _;

    if !dir.join(FJALL_VERSION_MARKER).is_file() {
        return Ok(0);
    }

    let db = fjall::SingleWriterTxDatabase::builder(dir)
        .worker_threads_unchecked(0)
        .open()
        .map_err(|e| {
            crate::error::MigrationIntegritySnafu {
                message: format!(
                    "pre-migration snapshot: failed to open source {} to take its pre-copy record count: {e}",
                    dir.display()
                ),
            }
            .build()
        })?;

    if !db.keyspace_exists(DATA_KEYSPACE) {
        return Ok(0);
    }
    let keyspace = db
        .keyspace(DATA_KEYSPACE, fjall::KeyspaceCreateOptions::default)
        .map_err(|e| {
            crate::error::MigrationIntegritySnafu {
                message: format!(
                    "pre-migration snapshot: source {} 'data' keyspace did not open for counting: {e}",
                    dir.display()
                ),
            }
            .build()
        })?;
    let count = db.read_tx().len(&keyspace).map_err(|e| {
        crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot: failed to count records in source {}: {e}",
                dir.display()
            ),
        }
        .build()
    })?;
    Ok(u64::try_from(count).unwrap_or(u64::MAX))
}

#[cfg(feature = "storage-fjall")]
fn verify_restorable(snapshot_dir: &Path, expected_count: u64) -> crate::error::Result<()> {
    use fjall::Readable as _;

    // F2 (proven empirically): refuse to even call into fjall until its own
    // marker file is confirmed present — otherwise `Database::open` treats
    // a missing/never-copied snapshot as "create a fresh empty one" and
    // every check below "passes" against that empty store.
    if !snapshot_dir.join(FJALL_VERSION_MARKER).is_file() {
        return Err(crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot at {} carries no fjall version marker — it was never actually written; refusing to open it (fjall's create-or-recover would silently manufacture an empty one)",
                snapshot_dir.display()
            ),
        }
        .build());
    }

    let db = fjall::SingleWriterTxDatabase::builder(snapshot_dir)
        // WHY: doc-hidden but public escape hatch around the panicking
        // `worker_threads(n)` (n=0 asserts `n > 0`). This handle exists only
        // to verify readability and is dropped immediately after — it must
        // not spin up background flush/compaction threads against a
        // disposable copy. Re-check this call if the fjall pin changes
        // (Cargo.toml WHY note).
        .worker_threads_unchecked(0)
        .open()
        .map_err(|e| {
            crate::error::MigrationIntegritySnafu {
                message: format!(
                    "pre-migration snapshot at {} did not open — not verified-restorable: {e}",
                    snapshot_dir.display()
                ),
            }
            .build()
        })?;

    // F2 (proven empirically): `Database::keyspace(name, create_options)`
    // CREATES the keyspace when absent — calling it directly on an empty
    // copy would silently manufacture the very keyspace being verified.
    // `keyspace_exists` never creates anything.
    if !db.keyspace_exists(DATA_KEYSPACE) {
        return Err(crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot at {} opened but its 'data' keyspace does not exist — not verified-restorable",
                snapshot_dir.display()
            ),
        }
        .build());
    }

    let keyspace = db
        .keyspace(DATA_KEYSPACE, fjall::KeyspaceCreateOptions::default)
        .map_err(|e| {
            crate::error::MigrationIntegritySnafu {
                message: format!(
                    "pre-migration snapshot at {} opened but its 'data' keyspace did not — not verified-restorable: {e}",
                    snapshot_dir.display()
                ),
            }
            .build()
        })?;

    // WHY: a genuine full-scan read (`Readable::len`), not the O(1)
    // `approximate_len()` — "verified restorable" means the copy can
    // actually be read end to end, not merely that its metadata opened.
    let restored_count = db.read_tx().len(&keyspace).map_err(|e| {
        crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot at {} opened but a full read failed — not verified-restorable: {e}",
                snapshot_dir.display()
            ),
        }
        .build()
    })?;
    let restored_count = u64::try_from(restored_count).unwrap_or(u64::MAX);

    if restored_count != expected_count {
        return Err(crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot at {} restored {restored_count} record(s) but the source held {expected_count} before the copy — not verified-restorable",
                snapshot_dir.display()
            ),
        }
        .build());
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test setup and assertions")]
mod tests {
    use super::*;

    // WHY: `std::fs::write`/`std::fs::read`/`std::fs::File::open` are
    // disallowed crate-wide (clippy.toml) — `File::create` + `write_all` and
    // `read_to_string` aren't, and every fixture here is plain text.
    fn write(path: &Path, content: &str) {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        let mut file = std::fs::File::create(path).expect("create file");
        file.write_all(content.as_bytes()).expect("write file");
    }

    #[test]
    fn copy_preserves_ordinary_files_and_structure() {
        let src = tempfile::tempdir().expect("src tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        write(&src.path().join("a.txt"), "hello");
        write(&src.path().join("nested/b.txt"), "world");

        copy_excluding_psyche(src.path(), dst.path()).expect("copy");

        assert_eq!(
            std::fs::read_to_string(dst.path().join("a.txt")).expect("read copied a.txt"),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(dst.path().join("nested/b.txt"))
                .expect("read copied nested/b.txt"),
            "world"
        );
    }

    #[test]
    fn copy_refuses_top_level_psyche() {
        let src = tempfile::tempdir().expect("src tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        write(&src.path().join("psyche/secret.txt"), "private");
        write(&src.path().join("public.txt"), "fine");

        copy_excluding_psyche(src.path(), dst.path()).expect("copy");

        assert!(
            !dst.path().join("psyche").exists(),
            "a psyche child must never be copied"
        );
        assert!(dst.path().join("public.txt").exists());
    }

    #[test]
    fn copy_refuses_nested_psyche_at_any_depth() {
        let src = tempfile::tempdir().expect("src tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        // Mirrors the live layout named in plan §8.5:
        // knowledge.fjall/shared/psyche/{0.jnl,keyspaces,lock,version}
        write(&src.path().join("shared/psyche/0.jnl"), "journal");
        write(&src.path().join("shared/psyche/lock"), "lock");
        write(&src.path().join("shared/other/data.bin"), "ok");

        copy_excluding_psyche(src.path(), dst.path()).expect("copy");

        assert!(
            !dst.path().join("shared/psyche").exists(),
            "psyche must be refused at any depth below the root, not only top-level"
        );
        assert!(dst.path().join("shared/other/data.bin").exists());
    }

    #[test]
    fn copy_of_a_root_literally_named_psyche_is_not_refused() {
        // aletheia#5779 F1: the psyche cohort's OWN store root is a `source`
        // argument literally named "psyche" — treat it exactly like any
        // other cohort's root, not a refused component.
        let src_parent = tempfile::tempdir().expect("src parent tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        let psyche_root = src_parent.path().join("psyche");
        write(&psyche_root.join("0.jnl"), "journal");
        write(&psyche_root.join("data.bin"), "cohort data");

        copy_excluding_psyche(&psyche_root, dst.path()).expect("copy");

        assert!(
            dst.path().join("0.jnl").exists(),
            "a snapshot root literally named 'psyche' must be copied like any other cohort"
        );
        assert!(dst.path().join("data.bin").exists());
    }

    #[test]
    fn copy_refuses_symlink_entries() {
        let src = tempfile::tempdir().expect("src tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        write(&src.path().join("real.bin"), "real data");
        std::os::unix::fs::symlink(src.path().join("real.bin"), src.path().join("linked.bin"))
            .expect("create symlink fixture");

        let err = copy_excluding_psyche(src.path(), dst.path())
            .expect_err("a symlinked entry must be refused, not silently skipped");
        assert!(
            err.to_string().contains("non-regular"),
            "expected a non-regular-entry refusal, got: {err}"
        );
    }

    #[cfg(feature = "storage-fjall")]
    #[test]
    fn snapshot_of_missing_source_is_a_noop() {
        let base = tempfile::tempdir().expect("tempdir");
        let source = base.path().join("does-not-exist");
        let snapshot_dir = base.path().join("snapshot");

        let result = pre_migration_snapshot(&source, &snapshot_dir).expect("no-op for first boot");
        assert_eq!(result, snapshot_dir);
        assert!(!snapshot_dir.exists());
    }

    #[cfg(feature = "storage-fjall")]
    fn seed_fjall_store(path: &Path, rows: &[(&str, &str)]) {
        let db = fjall::SingleWriterTxDatabase::builder(path)
            .open()
            .expect("open source fjall db");
        let keyspace = db
            .keyspace("data", fjall::KeyspaceCreateOptions::default)
            .expect("open data keyspace");
        let mut tx = db.write_tx();
        for (k, v) in rows {
            tx.insert(&keyspace, *k, *v);
        }
        tx.commit().expect("commit seed rows");
        db.persist(fjall::PersistMode::SyncAll)
            .expect("flush before copying");
    }

    #[cfg(feature = "storage-fjall")]
    #[test]
    fn snapshot_of_real_fjall_store_is_verified_restorable_and_reads_back_seeded_data() {
        use fjall::Readable as _;

        let base = tempfile::tempdir().expect("tempdir");
        let source = base.path().join("source");
        let snapshot_dir = base.path().join("snapshot");
        seed_fjall_store(&source, &[("k1", "v1"), ("k2", "v2")]);

        let result = pre_migration_snapshot(&source, &snapshot_dir).expect("snapshot + verify");
        assert_eq!(result, snapshot_dir);

        // F2: read the snapshot back for real, not merely assert the
        // directory is non-empty.
        let db = fjall::SingleWriterTxDatabase::builder(&snapshot_dir)
            .worker_threads_unchecked(0)
            .open()
            .expect("open snapshot for readback");
        let keyspace = db
            .keyspace("data", fjall::KeyspaceCreateOptions::default)
            .expect("open snapshot data keyspace");
        let read_tx = db.read_tx();
        assert_eq!(
            read_tx.get(&keyspace, "k1").expect("read k1").as_deref(),
            Some(b"v1".as_slice())
        );
        assert_eq!(
            read_tx.get(&keyspace, "k2").expect("read k2").as_deref(),
            Some(b"v2".as_slice())
        );
    }

    #[cfg(feature = "storage-fjall")]
    #[test]
    fn pre_migration_snapshot_replaces_a_prior_verified_snapshot_atomically() {
        use fjall::Readable as _;

        let base = tempfile::tempdir().expect("tempdir");
        let source = base.path().join("source");
        let snapshot_dir = base.path().join("snapshot");
        seed_fjall_store(&source, &[("k1", "v1")]);
        pre_migration_snapshot(&source, &snapshot_dir).expect("first snapshot");

        // Source changes between two boots; a stale `.new` sibling from an
        // earlier (imagined) crashed attempt must not corrupt the retry.
        std::fs::create_dir_all(staging_sibling(&snapshot_dir).join("garbage"))
            .expect("simulate a stale .new leftover");
        seed_fjall_store(&source, &[("k1", "v1"), ("k2", "v2")]);
        pre_migration_snapshot(&source, &snapshot_dir).expect("second snapshot");

        let db = fjall::SingleWriterTxDatabase::builder(&snapshot_dir)
            .worker_threads_unchecked(0)
            .open()
            .expect("open replaced snapshot");
        let keyspace = db
            .keyspace("data", fjall::KeyspaceCreateOptions::default)
            .expect("open replaced data keyspace");
        assert_eq!(
            db.read_tx()
                .len(&keyspace)
                .expect("count replaced snapshot"),
            2,
            "the replaced snapshot must reflect the new source, not merge with the old"
        );
        assert!(
            !staging_sibling(&snapshot_dir).exists(),
            "the .new staging dir must not survive a successful promotion"
        );
    }

    #[cfg(feature = "storage-fjall")]
    #[test]
    fn verify_restorable_fails_closed_when_the_fjall_version_marker_is_absent() {
        let base = tempfile::tempdir().expect("tempdir");
        let empty_dir = base.path().join("not-really-a-store");
        std::fs::create_dir_all(&empty_dir).expect("create empty dir");

        let err = verify_restorable(&empty_dir, 0)
            .expect_err("a directory with no fjall version marker must never verify");
        assert!(
            err.to_string().contains("version marker"),
            "expected a version-marker refusal, got: {err}"
        );
        assert!(
            !empty_dir.join(FJALL_VERSION_MARKER).exists(),
            "verification must not have created a fjall store as a side effect"
        );
    }

    #[cfg(feature = "storage-fjall")]
    #[test]
    fn verify_restorable_fails_closed_on_a_record_count_mismatch() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("store");
        seed_fjall_store(&dir, &[("k1", "v1")]);

        let err = verify_restorable(&dir, 999)
            .expect_err("a restored count that disagrees with the source count must fail");
        assert!(
            err.to_string().contains("restored 1 record"),
            "expected a count-mismatch message, got: {err}"
        );
    }
}
