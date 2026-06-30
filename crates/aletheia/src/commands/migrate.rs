//! `aletheia migrate`: cross-machine instance migration.
//!
//! Copies an entire Aletheia instance tree to a new location, normalizing
//! absolute paths in the configuration so they are portable.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

// ── CLI ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Args)]
pub(crate) struct MigrateArgs {
    /// Source instance directory
    pub source: PathBuf,
    /// Destination instance directory
    pub dest: PathBuf,
    /// Show what would be copied without making changes
    #[arg(long)]
    pub dry_run: bool,
    /// Follow symbolic links encountered in the source tree.
    ///
    /// By default `migrate` pre-walks the source and refuses if any symlink
    /// is found, because they frequently introduce cycles
    /// (`data/loop -> ../data`) or escape the source root. Refusing up-front
    /// avoids the prior failure mode where a cycle would only blow up
    /// mid-copy, leaving a partially populated destination behind. Pass
    /// this flag if you understand the risks and want the legacy behavior.
    #[arg(long)]
    pub follow_symlinks: bool,
}

// ── Dispatch ───────────────────────────────────────────────────────────────

pub(crate) fn run(args: &MigrateArgs) -> Result<()> {
    let source = std::fs::canonicalize(&args.source)
        .whatever_context("failed to canonicalize source path")?;

    if !source.is_dir() {
        whatever!("source path is not a directory: {}", source.display());
    }

    let source_config_toml = source.join("config").join("aletheia.toml");
    let source_config_json = source.join("config").join("aletheia.json");
    let has_config = source_config_toml.exists() || source_config_json.exists();
    let has_data = source.join("data").is_dir();

    if !has_config {
        whatever!(
            "source does not appear to be a valid aletheia instance: \
             config/aletheia.toml (or .json) not found in {}",
            source.display()
        );
    }
    if !has_data {
        whatever!(
            "source does not appear to be a valid aletheia instance: \
             data/ directory not found in {}",
            source.display()
        );
    }

    let dest = absolute_path(&args.dest)?;

    if source == dest {
        whatever!("source and destination cannot be the same directory");
    }
    if dest.starts_with(&source) {
        whatever!("destination cannot be inside the source directory");
    }

    if dest.exists() {
        let entries: Vec<_> = std::fs::read_dir(&dest)
            .whatever_context("failed to read destination directory")?
            .collect();
        if !entries.is_empty() {
            whatever!("destination directory is not empty: {}", dest.display());
        }
    }

    if !args.follow_symlinks {
        reject_if_symlinks(&source, &source)?;
    }

    let mut manifest = MigrateManifest::default();

    if args.dry_run {
        collect_dry_run(&source, &dest, &mut manifest)?;
        normalize_config_dry_run(&source, &dest, &mut manifest)?;
    } else {
        copy_tree(&source, &dest, &mut manifest)?;
        normalize_config(&source, &dest, &mut manifest)?;
    }

    verify_migrated_stores(&source, &dest, args.dry_run, &mut manifest)?;

    print_manifest(&manifest, args.dry_run);

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct MigrateManifest {
    files_copied: u64,
    bytes_copied: u64,
    paths_rewritten: Vec<String>,
    store_verifications: Vec<StoreVerificationEntry>,
}

#[derive(Debug)]
struct StoreVerificationEntry {
    name: &'static str,
    status: StoreVerificationStatus,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreVerificationStatus {
    Pass,
    Skipped,
}

impl StoreVerificationStatus {
    fn cli_label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Skipped => "SKIPPED",
        }
    }

    fn manifest_label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Skipped => "skipped",
        }
    }
}

const KNOWLEDGE_STORE: &str = "knowledge.fjall";
const SESSION_STORE: &str = "sessions.db";

const SESSION_PARTITIONS: &[&str] = &[
    "sessions",
    "messages",
    "usage",
    "tool_audit",
    "distillations",
    "notes",
    "blackboard",
    "counters",
];

fn verify_migrated_stores(
    source: &Path,
    dest: &Path,
    dry_run: bool,
    manifest: &mut MigrateManifest,
) -> Result<()> {
    verify_knowledge_store(source, dest, dry_run, manifest)?;
    verify_session_store(source, dest, dry_run, manifest)
}

fn verify_knowledge_store(
    source: &Path,
    dest: &Path,
    dry_run: bool,
    manifest: &mut MigrateManifest,
) -> Result<()> {
    let source_path = source.join("data").join(KNOWLEDGE_STORE);
    let dest_path = dest.join("data").join(KNOWLEDGE_STORE);

    if dry_run {
        record_store_verification(
            manifest,
            KNOWLEDGE_STORE,
            StoreVerificationStatus::Skipped,
            "dry run",
        );
        print_store_verification(
            "Knowledge store",
            StoreVerificationStatus::Skipped,
            "dry run",
        );
        return Ok(());
    }

    if !source_path.exists() {
        record_store_verification(
            manifest,
            KNOWLEDGE_STORE,
            StoreVerificationStatus::Skipped,
            "absent in source",
        );
        print_store_verification(
            "Knowledge store",
            StoreVerificationStatus::Skipped,
            "no knowledge.fjall found in source",
        );
        return Ok(());
    }

    if !dest_path.exists() {
        whatever!(
            "destination knowledge store missing after migration: {}",
            dest_path.display()
        );
    }

    let result = match oikonomos::maintenance::InstanceBackup::verify_fjall_store_tree(
        KNOWLEDGE_STORE,
        &dest_path,
    ) {
        Ok(Some(result)) => result,
        Ok(None) => whatever!(
            "destination knowledge store verification failed: not a fjall store or fjall cohort tree: {}",
            dest_path.display()
        ),
        Err(err) => whatever!("destination knowledge store verification failed: {err}"),
    };

    let detail = format!("{} keys", result.total_keys);
    record_store_verification(
        manifest,
        KNOWLEDGE_STORE,
        StoreVerificationStatus::Pass,
        detail.clone(),
    );
    print_store_verification("Knowledge store", StoreVerificationStatus::Pass, &detail);
    Ok(())
}

fn verify_session_store(
    source: &Path,
    dest: &Path,
    dry_run: bool,
    manifest: &mut MigrateManifest,
) -> Result<()> {
    let source_path = source.join("data").join(SESSION_STORE);
    let dest_path = dest.join("data").join(SESSION_STORE);

    if dry_run {
        record_store_verification(
            manifest,
            SESSION_STORE,
            StoreVerificationStatus::Skipped,
            "dry run",
        );
        print_store_verification("Session store", StoreVerificationStatus::Skipped, "dry run");
        return Ok(());
    }

    if !source_path.exists() {
        record_store_verification(
            manifest,
            SESSION_STORE,
            StoreVerificationStatus::Skipped,
            "absent in source",
        );
        print_store_verification(
            "Session store",
            StoreVerificationStatus::Skipped,
            "no sessions.db found in source",
        );
        return Ok(());
    }

    let source_metadata = std::fs::metadata(&source_path)
        .whatever_context("failed to read source session store metadata")?;
    if source_metadata.is_file() {
        verify_legacy_session_file_was_copied(&dest_path)?;
        record_store_verification(
            manifest,
            SESSION_STORE,
            StoreVerificationStatus::Skipped,
            "legacy file-shaped sessions.db; current fjall verification not applicable",
        );
        print_store_verification(
            "Session store",
            StoreVerificationStatus::Skipped,
            "legacy file-shaped sessions.db; current fjall verification not applicable",
        );
        return Ok(());
    }

    if !source_metadata.is_dir() {
        whatever!(
            "source session store has unsupported shape: {}",
            source_path.display()
        );
    }
    if !dest_path.exists() {
        whatever!(
            "destination session store missing after migration: {}",
            dest_path.display()
        );
    }
    if !dest_path.is_dir() {
        whatever!(
            "destination session store is not a current fjall directory: {}",
            dest_path.display()
        );
    }

    let result = match oikonomos::maintenance::FjallBackup::verify_store(&dest_path) {
        Ok(result) => result,
        Err(err) => whatever!("destination session store verification failed: {err}"),
    };
    if let Some(err) = result.first_error {
        whatever!("destination session store verification failed: {err}");
    }
    verify_session_partitions(&result)?;

    let detail = format!("{} keys", result.total_keys);
    record_store_verification(
        manifest,
        SESSION_STORE,
        StoreVerificationStatus::Pass,
        detail.clone(),
    );
    print_store_verification("Session store", StoreVerificationStatus::Pass, &detail);
    Ok(())
}

fn verify_legacy_session_file_was_copied(dest_path: &Path) -> Result<()> {
    if !dest_path.exists() {
        whatever!(
            "destination session store missing after migration: {}",
            dest_path.display()
        );
    }
    if !dest_path.is_file() {
        whatever!(
            "destination legacy session store is not a file: {}",
            dest_path.display()
        );
    }
    Ok(())
}

fn verify_session_partitions(result: &oikonomos::maintenance::FjallVerifyResult) -> Result<()> {
    let actual: BTreeSet<&str> = result
        .partition_counts
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let missing = SESSION_PARTITIONS
        .iter()
        .copied()
        .filter(|partition| !actual.contains(partition))
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        whatever!(
            "destination session store verification failed: missing current fjall partition(s): {}",
            missing.join(", ")
        );
    }
    Ok(())
}

fn record_store_verification(
    manifest: &mut MigrateManifest,
    name: &'static str,
    status: StoreVerificationStatus,
    detail: impl Into<String>,
) {
    manifest.store_verifications.push(StoreVerificationEntry {
        name,
        status,
        detail: detail.into(),
    });
}

fn print_store_verification(label: &str, status: StoreVerificationStatus, detail: &str) {
    println!("{} verification: {} ({detail})", label, status.cli_label());
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let raw = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let cwd = std::env::current_dir().whatever_context("failed to get current directory")?;
        cwd.join(path)
    };
    // WHY: canonicalize so macOS `/var/...` and `/private/var/...` compare
    // equal during the same-directory and containment checks. If the path
    // itself doesn't exist yet (e.g. `--dest`), walk up to the nearest
    // existing ancestor, canonicalize that, then re-append the trailing
    // segments so the resulting PathBuf is still a canonical form.
    Ok(canonicalize_partial(&raw))
}

fn canonicalize_partial(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut ancestor = path;
    while let Some(parent) = ancestor.parent() {
        if let Ok(canonical) = parent.canonicalize() {
            let mut result = canonical;
            if let Some(last) = ancestor.file_name() {
                tail.push(last);
            }
            for seg in tail.iter().rev() {
                result.push(seg);
            }
            return result;
        }
        if let Some(last) = ancestor.file_name() {
            tail.push(last);
        }
        ancestor = parent;
    }
    path.to_path_buf()
}

// WHY: Pre-walk the source tree using `symlink_metadata` so we never follow a
// symbolic link. This catches cycle-prone setups (`data/loop -> ../data`) and
// escape-the-root setups before `copy_tree` writes anything to disk. Refusing
// up-front is strictly safer than the prior behavior, which only blew up
// mid-copy (ELOOP) after leaving a partial destination behind.
fn reject_if_symlinks(src: &Path, source_root: &Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(src).whatever_context("failed to read source metadata")?;
    if metadata.file_type().is_symlink() {
        whatever!(
            "refusing to follow symbolic link in source tree: {}\n\
             (relative to source root {}; pass --follow-symlinks to override)",
            src.display(),
            source_root.display(),
        );
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(src).whatever_context("failed to read directory")? {
        let entry = entry.whatever_context("failed to read directory entry")?;
        reject_if_symlinks(&entry.path(), source_root)?;
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path, manifest: &mut MigrateManifest) -> Result<()> {
    let metadata = std::fs::metadata(src).whatever_context("failed to read metadata")?;

    if metadata.is_dir() {
        std::fs::create_dir_all(dst).whatever_context("failed to create directory")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(metadata.permissions().mode());
            std::fs::set_permissions(dst, permissions)
                .whatever_context("failed to set directory permissions")?;
        }

        for entry in std::fs::read_dir(src).whatever_context("failed to read directory")? {
            let entry = entry.whatever_context("failed to read directory entry")?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()), manifest)?;
        }
    } else {
        let bytes = std::fs::copy(src, dst).whatever_context("failed to copy file")?;
        manifest.files_copied += 1;
        manifest.bytes_copied += bytes;
    }

    Ok(())
}

fn collect_dry_run(src: &Path, dst: &Path, manifest: &mut MigrateManifest) -> Result<()> {
    let metadata = std::fs::metadata(src).whatever_context("failed to read metadata")?;

    if metadata.is_dir() {
        for entry in std::fs::read_dir(src).whatever_context("failed to read directory")? {
            let entry = entry.whatever_context("failed to read directory entry")?;
            collect_dry_run(&entry.path(), &dst.join(entry.file_name()), manifest)?;
        }
    } else {
        manifest.files_copied += 1;
        manifest.bytes_copied += metadata.len();
    }

    Ok(())
}

fn normalize_config(source: &Path, dest: &Path, manifest: &mut MigrateManifest) -> Result<()> {
    let dest_config_toml = dest.join("config").join("aletheia.toml");
    let dest_config_json = dest.join("config").join("aletheia.json");

    if dest_config_toml.exists() {
        let contents = std::fs::read_to_string(&dest_config_toml)
            .whatever_context("failed to read destination config")?;
        let mut doc: toml_edit::DocumentMut = contents
            .parse()
            .whatever_context("failed to parse config as TOML")?;

        {
            let root = doc.as_table_mut();
            for (_, item) in root.iter_mut() {
                normalize_toml_item(item, source, dest, &mut manifest.paths_rewritten);
            }
        }

        if !manifest.paths_rewritten.is_empty() {
            #[expect(
                clippy::disallowed_methods,
                reason = "migrate is CLI-invoked and requires synchronous filesystem access"
            )]
            std::fs::write(&dest_config_toml, doc.to_string())
                .whatever_context("failed to write normalized config")?;
        }
    } else if dest_config_json.exists() {
        let contents = std::fs::read_to_string(&dest_config_json)
            .whatever_context("failed to read destination config")?;
        let mut value: serde_json::Value =
            serde_json::from_str(&contents).whatever_context("failed to parse config as JSON")?;

        normalize_json_value(&mut value, source, dest, &mut manifest.paths_rewritten);

        if !manifest.paths_rewritten.is_empty() {
            let out = serde_json::to_string_pretty(&value)
                .whatever_context("failed to serialize normalized config")?;
            #[expect(
                clippy::disallowed_methods,
                reason = "migrate is CLI-invoked and requires synchronous filesystem access"
            )]
            std::fs::write(&dest_config_json, out)
                .whatever_context("failed to write normalized config")?;
        }
    }

    Ok(())
}

fn normalize_config_dry_run(
    source: &Path,
    dest: &Path,
    manifest: &mut MigrateManifest,
) -> Result<()> {
    let source_config_toml = source.join("config").join("aletheia.toml");
    let source_config_json = source.join("config").join("aletheia.json");

    if source_config_toml.exists() {
        let contents = std::fs::read_to_string(&source_config_toml)
            .whatever_context("failed to read source config")?;
        let mut doc: toml_edit::DocumentMut = contents
            .parse()
            .whatever_context("failed to parse config as TOML")?;

        {
            let root = doc.as_table_mut();
            for (_, item) in root.iter_mut() {
                normalize_toml_item(item, source, dest, &mut manifest.paths_rewritten);
            }
        }
    } else if source_config_json.exists() {
        let contents = std::fs::read_to_string(&source_config_json)
            .whatever_context("failed to read source config")?;
        let mut value: serde_json::Value =
            serde_json::from_str(&contents).whatever_context("failed to parse config as JSON")?;

        normalize_json_value(&mut value, source, dest, &mut manifest.paths_rewritten);
    }

    Ok(())
}

fn normalize_toml_item(
    item: &mut toml_edit::Item,
    source_root: &Path,
    dest_root: &Path,
    rewritten: &mut Vec<String>,
) {
    match item {
        toml_edit::Item::Value(val) => {
            normalize_toml_value(val, source_root, dest_root, rewritten);
        }
        toml_edit::Item::Table(table) => {
            for (_, v) in table.iter_mut() {
                normalize_toml_item(v, source_root, dest_root, rewritten);
            }
        }
        toml_edit::Item::ArrayOfTables(aot) => {
            for table in aot.iter_mut() {
                for (_, v) in table.iter_mut() {
                    normalize_toml_item(v, source_root, dest_root, rewritten);
                }
            }
        }
        toml_edit::Item::None => {}
    }
}

fn normalize_toml_value(
    val: &mut toml_edit::Value,
    source_root: &Path,
    dest_root: &Path,
    rewritten: &mut Vec<String>,
) {
    match val {
        toml_edit::Value::String(s) => {
            let current = s.value().clone();
            if let Some(new) = maybe_rewrite_path(&current, source_root, dest_root) {
                rewritten.push(format!("{current} -> {new}"));
                *val = toml_edit::Value::from(new);
            }
        }
        toml_edit::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_toml_value(v, source_root, dest_root, rewritten);
            }
        }
        toml_edit::Value::InlineTable(table) => {
            for (_, v) in table.iter_mut() {
                normalize_toml_value(v, source_root, dest_root, rewritten);
            }
        }
        // Scalars (bool, int, float, datetime): no path strings to rewrite.
        toml_edit::Value::Boolean(_)
        | toml_edit::Value::Integer(_)
        | toml_edit::Value::Float(_)
        | toml_edit::Value::Datetime(_) => {}
    }
}

fn normalize_json_value(
    val: &mut serde_json::Value,
    source_root: &Path,
    dest_root: &Path,
    rewritten: &mut Vec<String>,
) {
    match val {
        serde_json::Value::String(s) => {
            if let Some(new) = maybe_rewrite_path(s, source_root, dest_root) {
                rewritten.push(format!("{s} -> {new}"));
                *s = new;
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_json_value(v, source_root, dest_root, rewritten);
            }
        }
        serde_json::Value::Object(obj) => {
            for (_, v) in obj.iter_mut() {
                normalize_json_value(v, source_root, dest_root, rewritten);
            }
        }
        // Null, Bool, Number: no path strings to rewrite.
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn maybe_rewrite_path(path_str: &str, source_root: &Path, _dest_root: &Path) -> Option<String> {
    let path = Path::new(path_str);
    if !path.is_absolute() {
        return None;
    }
    // WHY: `source_root` is already canonical (via `absolute_path`), but paths
    // inside a config file may still be in the non-canonical form the user
    // authored (`/var/folders/...` on macOS, before the `/private` prefix is
    // resolved). The target may not exist either (e.g. a `packs` directory
    // declared but never created), so we cannot rely on `path.canonicalize()`.
    // Build every plausible form of both sides and try each pairing.
    let forms = |p: &Path| -> Vec<PathBuf> {
        let mut v = vec![p.to_path_buf()];
        if let Ok(canonical) = p.canonicalize()
            && canonical != *p
        {
            v.push(canonical);
        }
        if let Ok(stripped) = p.strip_prefix("/private") {
            let unprefixed = Path::new("/").join(stripped);
            if !v.contains(&unprefixed) {
                v.push(unprefixed);
            }
        }
        v
    };

    let source_forms = forms(source_root);
    let path_forms = forms(path);

    for src in &source_forms {
        for p in &path_forms {
            if p.starts_with(src)
                && let Ok(rel) = p.strip_prefix(src)
            {
                return Some(rel.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn print_manifest(manifest: &MigrateManifest, dry_run: bool) {
    if dry_run {
        println!("Dry-run manifest (no changes made):");
    } else {
        println!("Migration manifest:");
    }
    println!("  Files copied: {}", manifest.files_copied);
    println!("  Bytes copied: {}", manifest.bytes_copied);
    if manifest.paths_rewritten.is_empty() {
        println!("  Paths rewritten: none");
    } else {
        println!("  Paths rewritten:");
        for path in &manifest.paths_rewritten {
            println!("    {path}");
        }
    }
    if manifest.store_verifications.is_empty() {
        println!("  Store verification: none");
    } else {
        println!("  Store verification:");
        for entry in &manifest.store_verifications {
            println!(
                "    {}: {} ({})",
                entry.name,
                entry.status.manifest_label(),
                entry.detail
            );
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test setup requires synchronous filesystem access"
)]
mod tests {
    use super::*;

    fn create_minimal_instance(root: &Path) {
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("config/aletheia.toml"), "port = 1\n").unwrap();
    }

    fn make_fjall_store(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(path)
            .worker_threads_unchecked(0)
            .open()
            .unwrap();
        let partition = db
            .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        partition.insert("key", b"value").unwrap();
        db.persist(fjall::PersistMode::SyncAll).unwrap();
        drop(db);
    }

    fn make_current_session_store(path: &Path) {
        let store = mneme::store::SessionStore::open(path).unwrap();
        store.ensure_durable().unwrap();
        drop(store);
    }

    fn make_corrupt_current_session_store(path: &Path) {
        make_current_session_store(path);
        let db = fjall::SingleWriterTxDatabase::builder(path)
            .worker_threads_unchecked(0)
            .open()
            .unwrap();
        let sessions = db
            .keyspace("sessions", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        sessions.insert("bad-session", b"not-json").unwrap();
        db.persist(fjall::PersistMode::SyncAll).unwrap();
        drop(db);
    }

    fn store_entry<'a>(manifest: &'a MigrateManifest, name: &str) -> &'a StoreVerificationEntry {
        manifest
            .store_verifications
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
    }

    #[test]
    fn maybe_rewrite_path_rewrites_source_absolute() {
        let source = Path::new("/srv/instance");
        let dest = Path::new("/new/instance");
        assert_eq!(
            maybe_rewrite_path("/srv/instance/nous/main", source, dest),
            Some("nous/main".to_string())
        );
    }

    #[test]
    fn maybe_rewrite_path_leaves_relative() {
        let source = Path::new("/srv/instance");
        let dest = Path::new("/new/instance");
        assert!(maybe_rewrite_path("nous/main", source, dest).is_none());
    }

    #[test]
    fn maybe_rewrite_path_leaves_unrelated_absolute() {
        let source = Path::new("/srv/instance");
        let dest = Path::new("/new/instance");
        assert!(maybe_rewrite_path("/home/user/.claude/creds.json", source, dest).is_none());
    }

    #[test]
    fn maybe_rewrite_path_rewrites_nested() {
        let source = Path::new("/srv/instance");
        let dest = Path::new("/new/instance");
        assert_eq!(
            maybe_rewrite_path("/srv/instance/packs/engineering", source, dest),
            Some("packs/engineering".to_string())
        );
    }

    #[test]
    fn validate_source_requires_config() {
        let tmp = tempfile::tempdir().unwrap();
        let args = MigrateArgs {
            source: tmp.path().to_path_buf(),
            dest: PathBuf::from("/tmp/nonexistent-dest-migrate-xyz"),
            dry_run: false,
            follow_symlinks: false,
        };
        let result = run(&args);
        assert!(result.is_err(), "should fail without config and data");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("config/aletheia.toml (or .json) not found"),
            "expected config error: {msg}"
        );
    }

    #[test]
    fn validate_dest_must_be_empty() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("config")).unwrap();
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        std::fs::write(tmp.path().join("config/aletheia.toml"), "").unwrap();

        let dest = tempfile::tempdir().unwrap();
        std::fs::write(dest.path().join("existing.txt"), "hi").unwrap();

        let args = MigrateArgs {
            source: tmp.path().to_path_buf(),
            dest: dest.path().to_path_buf(),
            dry_run: false,
            follow_symlinks: false,
        };
        let result = run(&args);
        assert!(result.is_err(), "should fail when dest not empty");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not empty"), "expected empty error: {msg}");
    }

    #[test]
    fn dry_run_counts_files_without_copying() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::write(src.path().join("config/aletheia.toml"), "port = 1\n").unwrap();
        std::fs::write(src.path().join("data/file.txt"), "hello").unwrap();

        let dest = tempfile::tempdir().unwrap();

        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().to_path_buf(),
            dry_run: true,
            follow_symlinks: false,
        };
        run(&args).unwrap();

        let dest_entries: Vec<_> = std::fs::read_dir(dest.path()).unwrap().collect();
        assert!(dest_entries.is_empty(), "dry run should not copy files");
    }

    #[test]
    fn migrate_succeeds_with_knowledge_store_and_absent_sessions() {
        let src = tempfile::tempdir().unwrap();
        create_minimal_instance(src.path());
        make_fjall_store(&src.path().join("data/knowledge.fjall/shared"));

        let dest = tempfile::tempdir().unwrap();
        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: false,
        };

        run(&args).unwrap();

        assert!(
            dest.path()
                .join("migrated/data/knowledge.fjall/shared/version")
                .is_file()
        );
        assert!(!dest.path().join("migrated/data/sessions.db").exists());
    }

    #[test]
    fn migrate_verifies_current_fjall_sessions_store() {
        let src = tempfile::tempdir().unwrap();
        create_minimal_instance(src.path());
        make_current_session_store(&src.path().join("data/sessions.db"));

        let dest = tempfile::tempdir().unwrap();
        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: false,
        };

        run(&args).unwrap();

        assert!(
            dest.path()
                .join("migrated/data/sessions.db/version")
                .is_file()
        );
    }

    #[test]
    fn migrated_store_verification_rejects_missing_current_sessions_dest() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::create_dir_all(dest.path().join("data")).unwrap();
        make_current_session_store(&src.path().join("data/sessions.db"));

        let mut manifest = MigrateManifest::default();
        let result = verify_migrated_stores(src.path(), dest.path(), false, &mut manifest);

        assert!(result.is_err(), "missing destination sessions.db must fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("destination session store missing"),
            "expected missing sessions error: {msg}"
        );
    }

    #[test]
    fn migrate_rejects_corrupt_current_sessions_store() {
        let src = tempfile::tempdir().unwrap();
        create_minimal_instance(src.path());
        make_corrupt_current_session_store(&src.path().join("data/sessions.db"));

        let dest = tempfile::tempdir().unwrap();
        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: false,
        };

        let result = run(&args);

        assert!(
            result.is_err(),
            "corrupt destination session store must fail"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("destination session store verification failed"),
            "expected verification failure: {msg}"
        );
        assert!(
            msg.contains("bad-session"),
            "expected corrupt key in error: {msg}"
        );
    }

    #[test]
    fn migrated_store_verification_skips_legacy_file_shaped_sessions() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::create_dir_all(dest.path().join("data")).unwrap();
        std::fs::write(src.path().join("data/sessions.db"), "legacy sqlite").unwrap();
        std::fs::write(dest.path().join("data/sessions.db"), "legacy sqlite").unwrap();

        let mut manifest = MigrateManifest::default();

        verify_migrated_stores(src.path(), dest.path(), false, &mut manifest).unwrap();

        let sessions = store_entry(&manifest, SESSION_STORE);
        assert_eq!(sessions.status, StoreVerificationStatus::Skipped);
        assert!(
            sessions.detail.contains("legacy file-shaped"),
            "expected legacy skip detail: {}",
            sessions.detail
        );
    }

    #[test]
    fn copy_preserves_directory_structure() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data/sub")).unwrap();
        std::fs::write(src.path().join("config/aletheia.toml"), "port = 1\n").unwrap();
        std::fs::write(src.path().join("data/sub/file.txt"), "hello").unwrap();

        let dest = tempfile::tempdir().unwrap();

        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: false,
        };
        run(&args).unwrap();

        assert!(dest.path().join("migrated/config/aletheia.toml").exists());
        assert!(dest.path().join("migrated/data/sub/file.txt").exists());
    }

    #[test]
    fn normalize_rewrites_absolute_paths_in_toml() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::create_dir_all(src.path().join("nous/main")).unwrap();

        let toml = format!(
            "workspace = \"{}\"\npacks = [\"{}\"]\n",
            src.path().join("nous/main").to_string_lossy(),
            src.path().join("packs/custom").to_string_lossy(),
        );
        std::fs::write(src.path().join("config/aletheia.toml"), toml).unwrap();

        let dest = tempfile::tempdir().unwrap();

        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: false,
        };
        run(&args).unwrap();

        let rewritten =
            std::fs::read_to_string(dest.path().join("migrated/config/aletheia.toml")).unwrap();
        assert!(
            rewritten.contains("workspace = \"nous/main\""),
            "expected relative workspace in: {rewritten}"
        );
        assert!(
            rewritten.contains("packs = [\"packs/custom\"]"),
            "expected relative packs in: {rewritten}"
        );
    }

    #[test]
    fn normalize_rewrites_absolute_paths_in_json() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::create_dir_all(src.path().join("nous/main")).unwrap();

        let json = format!(
            r#"{{"workspace":"{}"}}"#,
            src.path()
                .join("nous/main")
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\""),
        );
        std::fs::write(src.path().join("config/aletheia.json"), json).unwrap();

        let dest = tempfile::tempdir().unwrap();

        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: false,
        };
        run(&args).unwrap();

        let rewritten =
            std::fs::read_to_string(dest.path().join("migrated/config/aletheia.json")).unwrap();
        assert!(
            rewritten.contains(r#""workspace": "nous/main""#),
            "expected relative workspace in: {rewritten}"
        );
    }

    #[test]
    fn source_dest_same_fails() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("config")).unwrap();
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        std::fs::write(tmp.path().join("config/aletheia.toml"), "").unwrap();

        let args = MigrateArgs {
            source: tmp.path().to_path_buf(),
            dest: tmp.path().to_path_buf(),
            dry_run: false,
            follow_symlinks: false,
        };
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("same directory"));
    }

    #[test]
    fn dest_inside_source_fails() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("config")).unwrap();
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        std::fs::write(tmp.path().join("config/aletheia.toml"), "").unwrap();

        let args = MigrateArgs {
            source: tmp.path().to_path_buf(),
            dest: tmp.path().join("sub/dest"),
            dry_run: false,
            follow_symlinks: false,
        };
        let result = run(&args);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("inside the source")
        );
    }

    // WHY: The exact bug from #4233 — a `data/loop -> ../data` cycle used to
    // ELOOP partway through a copy, leaving ~40 nested directories on disk.
    // The pre-walk must reject up-front so the destination stays untouched.
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_cycle_in_source_by_default() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::write(src.path().join("config/aletheia.toml"), "").unwrap();
        std::os::unix::fs::symlink("../data", src.path().join("data/loop")).unwrap();

        let dest = tempfile::tempdir().unwrap();
        let dest_target = dest.path().join("migrated");

        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest_target.clone(),
            dry_run: false,
            follow_symlinks: false,
        };
        let result = run(&args);
        assert!(result.is_err(), "should reject symlink by default");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("symbolic link") && msg.contains("--follow-symlinks"),
            "expected symlink/override message: {msg}"
        );
        assert!(
            !dest_target.exists(),
            "destination must not be created when pre-walk rejects",
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_file_in_source_by_default() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::write(src.path().join("config/aletheia.toml"), "").unwrap();
        std::fs::write(src.path().join("data/real.txt"), "hi").unwrap();
        std::os::unix::fs::symlink("./real.txt", src.path().join("data/link.txt")).unwrap();

        let dest = tempfile::tempdir().unwrap();
        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: false,
        };
        let result = run(&args);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("link.txt"),
            "error should name the offending symlink path",
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_during_dry_run_too() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::write(src.path().join("config/aletheia.toml"), "").unwrap();
        std::os::unix::fs::symlink("../data", src.path().join("data/loop")).unwrap();

        let dest = tempfile::tempdir().unwrap();
        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: true,
            follow_symlinks: false,
        };
        let result = run(&args);
        assert!(result.is_err(), "dry-run should also refuse to follow");
    }

    // WHY: The opt-in escape hatch must still work — the test uses a
    // non-cycling file-target symlink so we exercise --follow-symlinks
    // without triggering the cycle bug. After the migrate, the symlink
    // target's contents are present at the link's name in the destination
    // (legacy `std::fs::copy` semantics — it copies the target).
    #[cfg(unix)]
    #[test]
    fn follow_symlinks_flag_allows_non_cycle_symlinks() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("config")).unwrap();
        std::fs::create_dir_all(src.path().join("data")).unwrap();
        std::fs::write(src.path().join("config/aletheia.toml"), "").unwrap();
        std::fs::write(src.path().join("data/real.txt"), "hello").unwrap();
        std::os::unix::fs::symlink("./real.txt", src.path().join("data/link.txt")).unwrap();

        let dest = tempfile::tempdir().unwrap();
        let args = MigrateArgs {
            source: src.path().to_path_buf(),
            dest: dest.path().join("migrated"),
            dry_run: false,
            follow_symlinks: true,
        };
        run(&args).unwrap();

        let copied = std::fs::read_to_string(dest.path().join("migrated/data/link.txt")).unwrap();
        assert_eq!(copied, "hello");
    }
}
