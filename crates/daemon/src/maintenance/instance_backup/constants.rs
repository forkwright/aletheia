/// Manifest format version.
pub(crate) const MANIFEST_VERSION: &str = "aletheia-instance-backup-v1";

/// Snapshot protocol version.
///
/// WHY(#4950): bumped when the stage/verify/atomic-publish protocol changes.
pub(crate) const SNAPSHOT_PROTOCOL_VERSION: &str = "aletheia-instance-backup-v1-snapshot-1";

/// Policy used for all backup source traversal.
pub(crate) const SYMLINK_POLICY: &str = "reject";

/// Symlink name backup traversal excludes -- skipped and counted, never
/// dereferenced -- instead of refused like every other symlink, under any
/// backup source root. (#7246)
///
/// WHY: `.planning` is the ecosystem-wide convention for a symlink an
/// instance places under its own root (or under a workspace within it)
/// pointing at the operator's private planning repository. That repository
/// is a separate git repo whose backup is its own version control
/// (no-backup-copies canon); following the symlink and copying the tree it
/// names would duplicate a foreign repository -- `.git` included -- inside
/// every instance backup, and make the whole backup's success depend on
/// that foreign tree's contents. It would also reopen exactly the escape
/// `reject_symlinks_in_backup_source`'s traversal-escape guard exists to
/// stop: a `.planning` pointed at an ancestor or at itself would recurse
/// forever, and a `.planning` containing its own symlink would smuggle
/// unexpected content in. So the name is excluded outright: resolved only
/// via its own `symlink_metadata`, never dereferenced, so nothing about
/// its target -- including where it points or what it contains -- can
/// affect the backup. Any other symlink is still refused unconditionally.
pub(crate) const EXCLUDED_BACKUP_SYMLINK_NAME: &str = ".planning";

pub(crate) const STATUS_OK: &str = "ok";
pub(crate) const STATUS_EXCLUDED: &str = "excluded";

pub(crate) const MANIFEST_TOTAL_FILES_FIELD: &str = "total_files";
pub(crate) const MANIFEST_FILE_COUNT_FIELD: &str = "file_count";
pub(crate) const MANIFEST_RESTORE_PATH_FIELD: &str = "restore_path";
pub(crate) const MANIFEST_CHECKPOINT_GENERATIONS_FIELD: &str = "checkpoint_generations";
/// WHY(#6442): names the mechanism that made `quiesced: true` true. Injected
/// as raw manifest evidence (never a `BackupManifest` struct field) so
/// out-of-crate callers that construct `BackupManifest` literals directly
/// are unaffected by its addition.
pub(crate) const MANIFEST_QUIESCE_MECHANISM_FIELD: &str = "quiesce_mechanism";
/// WHY(#6442): the honest, derived answer to "how non-atomic was this
/// backup" — see [`super::BackupBuild::observed_snapshot_skew_seconds`].
pub(crate) const MANIFEST_OBSERVED_SKEW_SECONDS_FIELD: &str = "observed_snapshot_skew_seconds";
/// WHY(#5353): count of credential decryption-key sidecars excluded from
/// this backup set. Injected as raw manifest evidence (never a
/// `BackupManifest` struct field), same reasoning as
/// `MANIFEST_QUIESCE_MECHANISM_FIELD` above.
pub(crate) const MANIFEST_CREDENTIAL_KEYS_EXCLUDED_FIELD: &str = "credential_keys_excluded";
/// WHY(#7246): count of `.planning` symlinks excluded from this backup set,
/// never dereferenced. Injected as raw manifest evidence, same reasoning as
/// `MANIFEST_CREDENTIAL_KEYS_EXCLUDED_FIELD` above.
pub(crate) const MANIFEST_PLANNING_SYMLINKS_EXCLUDED_FIELD: &str = "planning_symlinks_excluded";

/// Prefix for hidden staging directories inside `backup_dir`.
///
/// WHY(#4950): `list_backups` skips these so an in-progress backup is never
/// listed as a valid backup set.
pub(crate) const STAGING_DIR_PREFIX: &str = ".aletheia-backup-staging.";

/// Prefix for hidden restore staging directories inside the instance root.
pub(crate) const RESTORE_STAGING_DIR_PREFIX: &str = ".aletheia-restore-staging.";

/// Prefix for hidden restore rollback directories inside the instance root.
pub(crate) const RESTORE_ROLLBACK_DIR_PREFIX: &str = ".aletheia-restore-rollback.";
