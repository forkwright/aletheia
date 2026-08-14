use std::collections::HashMap;
use std::path::PathBuf;

use super::default_symlink_policy;

/// Configuration for whole-instance backups.
#[derive(Debug, Clone)]
pub struct InstanceBackupConfig {
    /// Whether periodic whole-instance backups are enabled.
    pub enabled: bool,
    /// Path to the instance root directory.
    pub instance_root: PathBuf,
    /// Directory where timestamped backup sets are stored.
    pub backup_dir: PathBuf,
    /// Hours between automatic backups.
    pub interval_hours: u64,
    /// Maximum number of backup snapshots to retain.
    pub retention_count: usize,
    /// Additional agent workspaces to include: `(logical name, source path)`. (#5139)
    ///
    /// Each path is classified at backup time: paths inside `instance_root` are
    /// copied; absolute paths outside the root are recorded as omissions with a
    /// warning rather than silently dropped.
    pub additional_workspaces: Vec<(String, PathBuf)>,
}

impl Default for InstanceBackupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            instance_root: PathBuf::from("instance"),
            backup_dir: PathBuf::from("instance/data/backups/instance"),
            interval_hours: 24,
            retention_count: 7,
            additional_workspaces: Vec::new(),
        }
    }
}

/// Records an agent workspace that was not copied into a backup set. (#5139)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceOmission {
    /// Logical workspace name.
    pub name: String,
    /// Source path that was omitted.
    pub source_path: PathBuf,
    /// Why the workspace was omitted (e.g. `absolute-outside-root`).
    pub reason: String,
}

/// A single store entry recorded in the backup manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreEntry {
    /// Logical store name, e.g. `knowledge.fjall` or `sessions.db`.
    pub name: String,
    /// Absolute source path the store was copied from.
    ///
    /// SECURITY(#5043): never serialized into the manifest. A backup set is a
    /// portability/restore artifact that gets copied off the originating
    /// host; `backup_path` and the injected `restore_path` evidence (see
    /// `manifest::inject_entry_evidence`) are the relative paths restore and
    /// verify actually key off, so recording the workstation's absolute
    /// source path here would leak usernames/mount layout/private directory
    /// names into every exported manifest for no restore-side benefit.
    #[serde(skip_serializing, default)]
    pub source_path: PathBuf,
    /// Relative backup path inside the backup set.
    pub backup_path: PathBuf,
    /// ISO 8601 timestamp captured immediately after this entry's own copy
    /// finished.
    ///
    /// WHY(#6442): each entry records its OWN capture time rather than a
    /// value shared with every other entry, so a consumer can see the real
    /// wall-clock spread between stores in a non-quiesced backup instead of
    /// a single stamp that falsely implies simultaneity.
    pub snapshot_time: String,
    /// Total bytes copied for this store.
    pub byte_count: u64,
    /// Verification status: `ok` or `excluded`.
    pub status: String,
    /// Agent that produced this store, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Workspace source classification, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_source_class: Option<String>,
    /// Reason this store was excluded from the backup, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion_reason: Option<String>,
    /// SHA-256 digest of the backed-up file or directory tree, if recorded.
    ///
    /// Current-format manifests require this field for every included entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Whole-instance backup manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupManifest {
    /// Manifest format version.
    pub version: String,
    /// ISO 8601 timestamp when the backup set was created.
    pub created_at: String,
    /// Absolute path to the instance root that was backed up.
    ///
    /// SECURITY(#5043): never serialized -- see `StoreEntry::source_path`.
    #[serde(skip_serializing, default)]
    pub source_root: PathBuf,
    /// Required stores (must be present and verifiable for a valid set).
    pub stores: Vec<StoreEntry>,
    /// Optional data directories copied when present.
    pub optional_stores: Vec<StoreEntry>,
    /// Agent workspaces that were not copied into this set. (#5139)
    #[serde(default)]
    pub workspace_omissions: Vec<WorkspaceOmission>,
    /// Total bytes copied across all stores.
    pub total_bytes: u64,
    /// ISO 8601 timestamp recorded once, when this backup run began.
    ///
    /// WHY(#4950): a coarse start marker for the whole run.
    /// WHY(#6442): this alone does NOT prove the stores were captured
    /// together — it is stamped before any copying starts. Each
    /// [`StoreEntry::snapshot_time`] carries the real per-entry capture time,
    /// which is what actually lets a consumer detect cross-store skew.
    #[serde(default)]
    pub snapshot_epoch: String,
    /// Snapshot protocol version. Bumped when the staging/verify/publish protocol changes.
    #[serde(default)]
    pub snapshot_protocol_version: String,
    /// Whether a caller-supplied quiesce mechanism actually ran before this
    /// backup's stores were copied. `false` means the backup is a live,
    /// non-atomic export that may contain cross-store write-point skew —
    /// treat it as a crash-consistent, best-effort export, never as a
    /// guaranteed point-in-time recovery set.
    ///
    /// WHY(#6442): this is a fact DERIVED from
    /// [`InstanceBackup::create_backup_with_quiesce`]'s hook having actually
    /// executed and reported a mechanism, never a bare declaration.
    /// `instance_backup` holds no live store handles of its own — it copies
    /// filesystem paths, in-process or out-of-process alike — and so cannot
    /// quiesce writers by itself; [`InstanceBackup::create_backup`] supplies
    /// a no-op hook and is therefore always `false`. A caller that owns the
    /// live handles (writer pause, WAL flush, checkpoint) supplies the real
    /// hook.
    #[serde(default)]
    pub quiesced: bool,
    /// Per-fjall-store generation (seqno) captured from the staged copy.
    ///
    /// WHY(#4950): generation IDs are evidence of the store state at snapshot
    /// time and help detect restore mismatches.
    #[serde(default)]
    pub store_generations: HashMap<String, u64>,
    /// Symbolic-link traversal policy used when copying source paths.
    #[serde(default = "default_symlink_policy")]
    pub symlink_policy: String,
    /// Number of credential decryption-key sidecars excluded from this
    /// backup set. (#5353)
    ///
    /// WHY: a backup set is a portability artifact that routinely leaves the
    /// host (NAS, cloud, operator handoff). Symbolon stores each
    /// credential's decryption key as a bare sidecar file (`<name>.key`)
    /// next to the ciphertext it decrypts, so copying both would make the
    /// backup as sensitive as the plaintext tokens it protects. `0` means
    /// either no credentials were configured or none needed exclusion.
    #[serde(default)]
    pub credential_keys_excluded: u32,
}

/// Outcome of a whole-instance backup run.
#[derive(Debug, Clone, Default)]
pub struct InstanceBackupReport {
    /// Path to the created backup set directory.
    pub backup_path: Option<PathBuf>,
    /// Total bytes copied.
    pub bytes_copied: u64,
    /// Number of files copied.
    pub files_copied: u32,
    /// Number of old backups pruned.
    pub backups_pruned: u32,
}

/// Result of verifying a whole-instance backup set.
#[derive(Debug, Clone, Default)]
pub struct InstanceVerifyResult {
    /// Loaded manifest.
    pub manifest: Option<BackupManifest>,
    /// Per-store verification outcomes: name, key count (or error message).
    pub store_results: Vec<(String, std::result::Result<usize, String>)>,
    /// Total keys iterated across all fjall stores.
    pub total_keys: usize,
    /// First error encountered, if any.
    pub first_error: Option<String>,
    /// Per-fjall-store generation (seqno) captured during verification.
    pub store_generations: HashMap<String, u64>,
}

/// Result of verifying one live fjall store or a tree of fjall cohorts.
#[derive(Debug, Clone, Default)]
pub struct StoreVerifyReport {
    /// Total keys iterated across the store or cohort tree.
    pub total_keys: usize,
    /// Per-fjall-store generation (seqno) captured during verification.
    pub store_generations: Vec<(String, u64)>,
}

/// Options for restoring a whole-instance backup set.
#[derive(Debug, Clone)]
pub struct InstanceRestoreOptions {
    /// Path to the backup set directory containing `manifest.json`.
    pub backup_path: PathBuf,
    /// Bypass live-service preflight checks.
    ///
    /// This is unsafe because the running server can write into stores while
    /// restore is replacing them.
    pub force_live: bool,
    /// Optional selectors limiting restore to matching manifest entry names,
    /// backup paths, or source-relative target paths.
    pub include: Vec<String>,
    /// Optional selectors removing matching manifest entry names, backup paths,
    /// or source-relative target paths from the restore set.
    pub exclude: Vec<String>,
}

/// Outcome of a whole-instance restore run.
#[derive(Debug, Clone, Default)]
pub struct InstanceRestoreReport {
    /// Path to the restored backup set.
    pub backup_path: PathBuf,
    /// Manifest entries copied back into the instance.
    pub entries_restored: usize,
    /// Manifest entries skipped because they were excluded or duplicate targets.
    pub entries_skipped: usize,
    /// Existing live entries moved aside and later discarded after success.
    pub live_entries_replaced: usize,
    /// Bytes copied into the restore staging directory.
    pub bytes_copied: u64,
}

impl InstanceBackupReport {
    /// Returns `true` if this run created and published a backup set.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.backup_path.is_some()
    }

    /// Returns `true` if retention pruning removed at least one old backup.
    #[must_use]
    pub fn pruned_old_backups(&self) -> bool {
        self.backups_pruned > 0
    }
}

impl InstanceVerifyResult {
    /// Returns `true` when verification completed without recording any error.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.first_error.is_none() && self.store_results.iter().all(|(_, result)| result.is_ok())
    }

    /// Returns the manifest when verification loaded one successfully.
    #[must_use]
    pub fn manifest(&self) -> Option<&BackupManifest> {
        self.manifest.as_ref()
    }
}

impl StoreVerifyReport {
    /// Returns `true` when at least one key or generation was observed.
    #[must_use]
    pub fn observed_store_state(&self) -> bool {
        self.total_keys > 0 || !self.store_generations.is_empty()
    }
}

impl InstanceRestoreOptions {
    /// Build restore options that restore every included manifest entry with live-service checks enabled.
    #[must_use]
    pub fn all_entries(backup_path: PathBuf) -> Self {
        Self {
            backup_path,
            force_live: false,
            include: Vec::new(),
            exclude: Vec::new(),
        }
    }

    /// Returns `true` when no include or exclude selector narrows the restore plan.
    #[must_use]
    pub fn restores_all_entries(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }
}

impl InstanceRestoreReport {
    /// Returns `true` if restore replaced at least one existing live entry.
    #[must_use]
    pub fn replaced_live_entries(&self) -> bool {
        self.live_entries_replaced > 0
    }
}

/// Manages whole-instance backup sets.
// kanon:ignore RUST/pub-visibility — consumed by the aletheia binary crate for backup and maintenance commands
pub struct InstanceBackup {
    pub(crate) config: InstanceBackupConfig,
}

#[derive(Clone)]
pub(crate) struct BackupBuild {
    pub(crate) source_root: PathBuf,
    pub(crate) stores: Vec<StoreEntry>,
    pub(crate) optional_stores: Vec<StoreEntry>,
    pub(crate) store_metadata: Vec<EntryManifestMetadata>,
    pub(crate) optional_store_metadata: Vec<EntryManifestMetadata>,
    pub(crate) workspace_omissions: Vec<WorkspaceOmission>,
    pub(crate) total_bytes: u64,
    pub(crate) total_files: u64,
    /// Count of credential decryption-key sidecars excluded from this
    /// backup set. See [`BackupManifest::credential_keys_excluded`]. (#5353)
    pub(crate) credential_keys_excluded: u32,
    pub(crate) snapshot_time: String,
    /// Monotonic instant sampled around the first entry actually copied.
    ///
    /// WHY(#6442): paired with `last_entry_copied_at` to compute an honest,
    /// wall-clock-jump-immune measure of how non-atomic a backup run was.
    /// Never serialized — the manifest carries only the derived seconds
    /// value, injected as evidence alongside `total_files`.
    pub(crate) first_entry_copied_at: Option<std::time::Instant>,
    /// Monotonic instant sampled around the last entry actually copied.
    pub(crate) last_entry_copied_at: Option<std::time::Instant>,
}

#[derive(Debug, Clone)]
pub(crate) struct RestorePlanEntry {
    pub(crate) name: String,
    pub(crate) backup_path: PathBuf,
    pub(crate) backup_source: PathBuf,
    pub(crate) target_rel: PathBuf,
    pub(crate) target_path: PathBuf,
    pub(crate) byte_count: u64,
    pub(crate) file_count: u64,
    pub(crate) sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RestorePlan {
    pub(crate) entries: Vec<RestorePlanEntry>,
    pub(crate) entries_skipped: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct RollbackEntry {
    pub(crate) target_path: PathBuf,
    pub(crate) rollback_path: Option<PathBuf>,
    pub(crate) published: bool,
}

/// Arguments for recording an optional store entry without copying.
pub(crate) struct OptionalStoreRecord {
    pub(crate) name: String,
    pub(crate) source_path: PathBuf,
    pub(crate) backup_path: PathBuf,
    pub(crate) restore_path: Option<PathBuf>,
    pub(crate) status: String,
    pub(crate) agent_id: Option<String>,
    pub(crate) workspace_source_class: Option<String>,
    pub(crate) exclusion_reason: Option<String>,
    pub(crate) byte_count: u64,
    pub(crate) file_count: u64,
    pub(crate) sha256: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EntryManifestMetadata {
    pub(crate) file_count: Option<u64>,
    pub(crate) restore_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ManifestEvidence {
    pub(crate) total_files: Option<u64>,
    pub(crate) stores: Vec<EntryManifestMetadata>,
    pub(crate) optional_stores: Vec<EntryManifestMetadata>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ManifestSection {
    Stores,
    OptionalStores,
}

impl ManifestEvidence {
    pub(crate) fn entry(
        &self,
        section: ManifestSection,
        index: usize,
    ) -> Option<&EntryManifestMetadata> {
        match section {
            ManifestSection::Stores => self.stores.get(index),
            ManifestSection::OptionalStores => self.optional_stores.get(index),
        }
    }
}
