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
//! - never opens `source` via fjall at all, only a filesystem `cp -r`;
//! - runs strictly BEFORE `open_fjall`, in the caller's startup path (e.g.
//!   `aletheia::runtime::setup::open_knowledge_stores`), not inside
//!   `knowledge_store::migration`.

use std::path::{Path, PathBuf};

/// Path component name refused at any depth. The psyche partition holds
/// private identity-continuity content that must never be duplicated
/// outside its owning directory tree — not even into a crash-recovery
/// snapshot with a looser retention/access story
/// (`~/aletheia/instance/data/knowledge.fjall/shared/psyche/`).
const REFUSED_COMPONENT: &str = "psyche";

/// Recursively copy `source` into `dest`, refusing any path component
/// literally named `psyche` at any depth — not merely a top-level directory
/// of that name. Symlinks are not followed (a fjall data directory should
/// not contain any; refusing them avoids ever copying outside `source`).
///
/// # Errors
/// Returns an error if a filesystem operation fails partway through. On
/// error the destination may hold a partial copy; callers must not treat a
/// partial copy as a usable snapshot (see [`pre_migration_snapshot`], which
/// gates on a subsequent open-and-read, not on this function alone).
pub fn copy_excluding_psyche(source: &Path, dest: &Path) -> std::io::Result<()> {
    if source
        .file_name()
        .is_some_and(|name| name == REFUSED_COMPONENT)
    {
        return Ok(());
    }

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
        // Symlink or other special file: skip rather than follow/copy it —
        // fjall data directories don't legitimately contain these.
        Ok(())
    }
}

/// Take a filesystem-level copy of a fjall knowledge-store directory and
/// verify it is genuinely restorable before returning: opens the COPY
/// (never `source`) with zero background workers and performs a real
/// full-scan read, not merely a successful `open()`.
///
/// If `source` does not exist yet (first boot, store not yet created),
/// this is a no-op — there is nothing to protect.
///
/// # Errors
/// Returns an error if the copy cannot be made, or if the copy cannot be
/// opened and read back. "The copy call returned `Ok`" is never, on its
/// own, treated as "backed up" — that is exactly the claim the fleet's
/// `preserve` doctrine exists to refuse.
#[cfg(feature = "storage-fjall")]
pub fn pre_migration_snapshot(source: &Path, snapshot_dir: &Path) -> crate::error::Result<PathBuf> {
    if !source.exists() {
        return Ok(snapshot_dir.to_path_buf());
    }

    // WHY: best-effort — a stale snapshot directory left by a prior crash
    // (or a prior run that never reached the verify step) must not silently
    // merge with this run's copy and pass verification on leftover data.
    let _ = std::fs::remove_dir_all(snapshot_dir);

    copy_excluding_psyche(source, snapshot_dir).map_err(|e| {
        crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot copy from {} to {} failed: {e}",
                source.display(),
                snapshot_dir.display()
            ),
        }
        .build()
    })?;

    verify_restorable(snapshot_dir)?;
    Ok(snapshot_dir.to_path_buf())
}

#[cfg(feature = "storage-fjall")]
fn verify_restorable(snapshot_dir: &Path) -> crate::error::Result<()> {
    use fjall::Readable as _;

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

    let keyspace = db
        .keyspace("data", fjall::KeyspaceCreateOptions::default)
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
    db.read_tx().len(&keyspace).map_err(|e| {
        crate::error::MigrationIntegritySnafu {
            message: format!(
                "pre-migration snapshot at {} opened but a full read failed — not verified-restorable: {e}",
                snapshot_dir.display()
            ),
        }
        .build()
    })?;

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
            "top-level psyche must never be copied"
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
            "psyche must be refused at any depth, not only top-level"
        );
        assert!(dst.path().join("shared/other/data.bin").exists());
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
    #[test]
    fn snapshot_of_real_fjall_store_is_verified_restorable() {
        let base = tempfile::tempdir().expect("tempdir");
        let source = base.path().join("source");
        let snapshot_dir = base.path().join("snapshot");

        {
            let db = fjall::SingleWriterTxDatabase::builder(&source)
                .open()
                .expect("open source fjall db");
            let keyspace = db
                .keyspace("data", fjall::KeyspaceCreateOptions::default)
                .expect("open data keyspace");
            let mut tx = db.write_tx();
            tx.insert(&keyspace, "k1", "v1");
            tx.commit().expect("commit seed row");
            db.persist(fjall::PersistMode::SyncAll)
                .expect("flush before copying");
        }

        let result = pre_migration_snapshot(&source, &snapshot_dir).expect("snapshot + verify");
        assert_eq!(result, snapshot_dir);
        assert!(
            snapshot_dir.join("data").exists()
                || std::fs::read_dir(&snapshot_dir)
                    .expect("list snapshot dir")
                    .next()
                    .is_some()
        );
    }
}
