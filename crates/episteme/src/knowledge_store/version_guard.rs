//! Pre-open schema-version guard for fjall-backed knowledge stores
//! (aletheia#6838, correction 2).
//!
//! [`KnowledgeStore::open_fjall`](super::KnowledgeStore::open_fjall) already
//! refuses a version-mismatched store -- but only via
//! [`KnowledgeStore::init_schema`](super::KnowledgeStore::init_schema)'s
//! in-band check against the `schema_version` Datalog relation, which can
//! only run *after* `crate::engine::Db::open_fjall` (`fjall::Keyspace::open()`
//! under the hood) has already opened the keyspace. Opening is exactly where
//! fjall's own auto-recovery runs, as a side effect of the open call itself,
//! not of any query that follows it (`knowledge_store::snapshot`'s module
//! docs: this has already cost the fleet ~600 records once, via that same
//! auto-recovery deleting segments absent from the levels manifest). A
//! version check that runs only after that open can refuse to *use* a store
//! that may already have been silently altered -- it cannot prevent the
//! alteration.
//!
//! This module adds a second, out-of-band check that runs strictly before
//! any fjall API touches the directory: a plain-file manifest
//! (`schema_manifest.json`), read and written with bare [`std::fs`] calls,
//! never through fjall. Mirrors the pattern already shipped for graphe's
//! session store (`graphe::store::fjall_store`, issue #5031) -- same idea,
//! reapplied to the knowledge store the fleet's `~600` records incident and
//! aletheia#6838 both concern.
//!
//! **Policy** (this is the compatibility contract the guard enforces, and
//! the only one it enforces):
//!
//! - **Refuse newer.** A manifest stamped *above* `SCHEMA_VERSION` means an
//!   older binary is opening a store a newer one already migrated -- the
//!   one case fjall's own auto-recovery can silently act on before any
//!   in-band check would even run. This is the only case this guard
//!   refuses.
//! - **Migrate older.** A manifest stamped *below* `SCHEMA_VERSION` is the
//!   ordinary upgrade path -- a newer binary opening a store a prior
//!   release left behind. The guard is silent (`Ok(())`) and defers to the
//!   existing in-band machinery
//!   ([`KnowledgeStore::protect_pre_migration`](super::KnowledgeStore::protect_pre_migration)
//!   then
//!   [`KnowledgeStore::init_schema`](super::KnowledgeStore::init_schema)'s
//!   `apply_pending_migrations`), the same as it always has. Refusing here
//!   instead would turn *every* schema bump into a refusal for *every*
//!   existing instance on its very next start -- exactly the regression
//!   this guard must not introduce.
//! - **Backfill missing.** A store this code has never opened -- brand
//!   new, or one predating this guard -- has no manifest yet and nothing
//!   to check against, so it too falls through to the in-band path
//!   unchanged.  [`stamp_after_open`] then writes the manifest for the
//!   *next* open, once [`KnowledgeStore::init_schema`](super::KnowledgeStore::init_schema)
//!   has confirmed the store actually reached `SCHEMA_VERSION`.
//!
//! So this guard only ever adds a *faster*, pre-open refusal on top of a
//! store it has itself already stamped at a version newer than the
//! opening binary expects; it never introduces a new refusal for a store
//! at or below the expected version, so it cannot regress an existing
//! instance's next start.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;

/// Filename for the pre-open schema-version manifest: a sibling of fjall's
/// own `keyspaces/` directory inside a knowledge-store cohort.
///
/// WHY a plain file: the entire point is to decide whether this binary may
/// open the store *before* any fjall keyspace machinery -- which performs
/// its own internal directory recovery as a side effect of opening -- ever
/// runs against it.
const SCHEMA_MANIFEST_FILE: &str = "schema_manifest.json";

#[derive(Debug, Serialize, Deserialize)]
struct SchemaManifest {
    schema_version: i64,
}

/// Check `path`'s pre-open manifest (if any) against `expected` before the
/// caller opens the fjall keyspace at `path`.
///
/// Implements this module's refuse-newer / migrate-older / backfill-missing
/// policy (see module docs). Returns `Ok(())` when there is nothing to
/// refuse: no manifest on disk (a fresh store, or one predating this
/// guard), the stamped version already matches `expected`, or the stamped
/// version is *below* `expected` -- the ordinary upgrade path, left to the
/// existing in-band migration machinery. Returns
/// [`crate::error::Error::SchemaVersion`] -- the same typed refusal
/// `KnowledgeStore::init_schema`'s existing in-band check already uses --
/// only when the stamped version is *above* `expected` (an older binary
/// opening a store a newer one already migrated), without ever touching
/// the fjall directory.
///
/// # Errors
///
/// Returns an error if the manifest exists and is unreadable or corrupt, or
/// if its stamped schema version is greater than `expected`.
#[expect(
    clippy::disallowed_methods,
    reason = "synchronous pre-open gate: it must run before any fjall API touches the \
              directory, in every sync caller of open_fjall (including non-tokio bin \
              targets), so it cannot depend on a runtime being present"
)]
pub(super) fn check_before_open(path: &Path, expected: i64) -> crate::error::Result<()> {
    let manifest_path = path.join(SCHEMA_MANIFEST_FILE);
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(crate::error::EngineInitSnafu {
                message: format!(
                    "failed to read schema manifest at {}: {e}",
                    manifest_path.display()
                ),
            }
            .build());
        }
    };

    let manifest: SchemaManifest =
        serde_json::from_slice(&bytes).context(crate::error::StoredJsonSnafu)?;

    // Refuse newer, migrate older: only a manifest stamped *above*
    // `expected` is the destructive case (an older binary re-opening a
    // store a newer one already migrated). A manifest at or below
    // `expected` -- an ordinary upgrade in progress, or nothing to do --
    // falls through to the existing in-band migration machinery.
    if manifest.schema_version <= expected {
        return Ok(());
    }

    Err(crate::error::SchemaVersionSnafu {
        expected,
        found: manifest.schema_version,
    }
    .build())
}

/// Stamp `path`'s pre-open manifest with `version` after a successful open.
///
/// Callers must only invoke this once
/// [`KnowledgeStore::init_schema`](super::KnowledgeStore::init_schema) has
/// confirmed the store is actually at `version` -- never speculatively --
/// so the manifest is always a true record of a version this code has
/// itself verified, not merely attempted.
///
/// # Errors
///
/// Returns an error if the manifest file cannot be serialized or written.
#[expect(
    clippy::disallowed_methods,
    reason = "synchronous post-open stamp: init_schema has already confirmed the store is at \
              `version` by the time this runs, in every sync caller of open_fjall (including \
              non-tokio bin targets), so it cannot depend on a runtime being present"
)]
pub(super) fn stamp_after_open(path: &Path, version: i64) -> crate::error::Result<()> {
    let manifest_path = path.join(SCHEMA_MANIFEST_FILE);
    let data = serde_json::to_vec_pretty(&SchemaManifest {
        schema_version: version,
    })
    .context(crate::error::StoredJsonSnafu)?;
    fs::write(&manifest_path, data).map_err(|e| {
        crate::error::EngineInitSnafu {
            message: format!(
                "failed to write schema manifest at {}: {e}",
                manifest_path.display()
            ),
        }
        .build()
    })
}
