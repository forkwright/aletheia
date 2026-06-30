/// Manifest format version.
pub(crate) const MANIFEST_VERSION: &str = "aletheia-instance-backup-v1";

/// Snapshot protocol version.
///
/// WHY(#4950): bumped when the stage/verify/atomic-publish protocol changes.
pub(crate) const SNAPSHOT_PROTOCOL_VERSION: &str = "aletheia-instance-backup-v1-snapshot-1";

/// Policy used for all backup source traversal.
pub(crate) const SYMLINK_POLICY: &str = "reject";

pub(crate) const STATUS_OK: &str = "ok";
pub(crate) const STATUS_EXCLUDED: &str = "excluded";

pub(crate) const MANIFEST_TOTAL_FILES_FIELD: &str = "total_files";
pub(crate) const MANIFEST_FILE_COUNT_FIELD: &str = "file_count";
pub(crate) const MANIFEST_RESTORE_PATH_FIELD: &str = "restore_path";
pub(crate) const MANIFEST_CHECKPOINT_GENERATIONS_FIELD: &str = "checkpoint_generations";

/// Prefix for hidden staging directories inside `backup_dir`.
///
/// WHY(#4950): `list_backups` skips these so an in-progress backup is never
/// listed as a valid backup set.
pub(crate) const STAGING_DIR_PREFIX: &str = ".aletheia-backup-staging.";

/// Prefix for hidden restore staging directories inside the instance root.
pub(crate) const RESTORE_STAGING_DIR_PREFIX: &str = ".aletheia-restore-staging.";

/// Prefix for hidden restore rollback directories inside the instance root.
pub(crate) const RESTORE_ROLLBACK_DIR_PREFIX: &str = ".aletheia-restore-rollback.";
