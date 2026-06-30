//! Whole-instance backup: coherent snapshot of knowledge, sessions, runtime state, config, and workspace data.
//!
//! WHY(#4856): the legacy `FjallBackup` only copied `knowledge.fjall`. The
//! `aletheia backup` command and the daemon's scheduled backup task now produce
//! a backup *set* that includes `sessions.db`, auth/task-state stores,
//! configuration, and workspace data needed for run replay/review. A JSON
//! manifest records every covered store, its source path, restore target,
//! snapshot time, byte/file counts, content digest, and verification status.

mod build;
mod constants;
mod create;
mod filesystem;
mod manifest;
mod restore;
mod types;
mod verify;

pub use types::{
    BackupManifest, InstanceBackup, InstanceBackupConfig, InstanceBackupReport,
    InstanceRestoreOptions, InstanceRestoreReport, InstanceVerifyResult, StoreEntry,
    StoreVerifyReport, WorkspaceOmission,
};

pub(crate) use constants::*;
pub(crate) use filesystem::*;
pub(crate) use manifest::*;
pub(crate) use types::{
    BackupBuild, EntryManifestMetadata, ManifestEvidence, ManifestSection, OptionalStoreRecord,
    RestorePlan, RestorePlanEntry, RollbackEntry,
};
pub(crate) use verify::*;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests;
