//! Whole-instance backup: coherent snapshot of knowledge, sessions, runtime state, config, and workspace data.
//!
//! WHY(#4856): the legacy `FjallBackup` only copied `knowledge.fjall`. The
//! `aletheia backup` command and the daemon's scheduled backup task now produce
//! a backup *set* that includes `sessions.db`, auth/task-state stores,
//! configuration, and workspace data needed for run replay/review. A JSON
//! manifest records every covered store, its restore target, snapshot time,
//! byte/file counts, content digest, and verification status.
//!
//! SECURITY(#5043): the manifest never records the host's absolute source
//! paths -- it is a portability/restore artifact that gets copied off the
//! originating host, and restore/verify key off relative `backup_path`/
//! `restore_path` alone.

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

/// Scan persisted backup state and publish it for freshness alerting. (#6445)
///
/// Called at daemon startup and after every backup attempt — including skipped
/// and failed ones — because the question the alert asks is "is there a recent
/// backup on disk", which has an answer regardless of what this run did.
///
/// A backup directory that cannot be listed publishes `None`, the same as an
/// empty one. WHY: at a disaster-recovery boundary an unreadable backup store
/// is not evidence of a good backup, so it must alert rather than hold the last
/// known-good value.
pub fn publish_backup_state(
    config: &InstanceBackupConfig,
    recorder: &dyn super::BackupMetricsRecorder,
) {
    let last_success_unixtime = InstanceBackup::new(config.clone())
        .latest_backup_time()
        .unwrap_or_else(|e| {
            tracing::warn!(
                backup_dir = %config.backup_dir.display(),
                error = %e,
                "could not read backup directory for freshness metric; reporting as no backup"
            );
            None
        })
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|since_epoch| i64::try_from(since_epoch.as_secs()).ok());

    recorder.record_backup_state(
        last_success_unixtime,
        config.enabled,
        config.interval_hours.saturating_mul(3600),
    );
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests;
