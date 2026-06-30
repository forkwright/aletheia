//! Operator credential management over the instance credential directory.

use std::path::{Path, PathBuf};

use koina::secret::SecretString;
use snafu::IntoError;

use crate::error::{self, Result};
use crate::types::{ManagedCredential, ManagedCredentialRole, ManagedCredentialStatus};

use super::CredentialFile;
use super::file_ops::CredentialFileLock;

const BACKUP_SUFFIX: &str = ".backup";
const JSON_EXT: &str = "json";
const ROTATE_JOURNAL_SUFFIX: &str = ".rotate.journal";
const MIN_CREDENTIAL_SECRET_CHARS: usize = 9;
const REDACTED_SECRET_PLACEHOLDER: &str = "...????";

pub(crate) fn list(root: &Path) -> Result<Vec<ManagedCredential>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    recover_all_rotations(root)?;

    let mut credentials = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|source| io_error(root, source))? {
        let entry = entry.map_err(|source| io_error(root, source))?;
        let path = entry.path();
        if !is_json_file(&path) {
            continue;
        }
        let Some((provider, role)) = parse_path_role(&path) else {
            continue;
        };
        if let Some(credential) = metadata_from_path(root, &provider, role, None)? {
            credentials.push(credential);
        }
    }

    credentials.sort_by(|a, b| {
        a.provider
            .cmp(&b.provider)
            .then_with(|| a.role.as_str().cmp(b.role.as_str()))
    });
    Ok(credentials)
}

pub(crate) fn add(
    root: &Path,
    provider: &str,
    key: &SecretString,
    role: ManagedCredentialRole,
) -> Result<ManagedCredential> {
    validate_provider(provider)?;
    validate_credential_secret(key)?;
    recover_provider_rotation(root, provider)?;
    let path = credential_path(root, provider, role)?;

    // WHY: use `create_new` so the existence check and file creation happen
    // atomically, closing the TOCTOU window between `path.exists()` and `save()`.
    let create_result = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
        }
        #[cfg(not(unix))]
        {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
        }
    };

    if let Err(source) = create_result {
        if source.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(error::DuplicateSnafu {
                entity: "credential".to_owned(),
                id: credential_id(provider, role),
            }
            .build());
        }
        return Err(io_error(&path, source));
    }

    let credential = CredentialFile {
        token: key.clone(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };

    if let Err(source) = credential.save(&path) {
        // kanon:ignore RUST/no-silent-result-swallow — best-effort cleanup of the
        // placeholder created above so a failed add does not leave a partial file.
        let _ = std::fs::remove_file(&path);
        return Err(io_error(&path, source));
    }

    metadata_from_path(root, provider, role, None)?.ok_or_else(|| {
        error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: credential_id(provider, role),
        }
        .build()
    })
}

pub(crate) fn validate(root: &Path, id: &str) -> Result<ManagedCredential> {
    let (provider, role) = parse_id(id)?;
    recover_provider_rotation(root, &provider)?;
    let validated_at = jiff::Timestamp::now().to_string();
    metadata_from_path(root, &provider, role, Some(validated_at))?.ok_or_else(|| {
        error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: id.to_owned(),
        }
        .build()
    })
}

pub(crate) fn rotate(root: &Path, provider: &str) -> Result<Vec<ManagedCredential>> {
    validate_provider(provider)?;
    let primary_path = credential_path(root, provider, ManagedCredentialRole::Primary)?;
    let backup_path = credential_path(root, provider, ManagedCredentialRole::Backup)?;

    // WHY: provider-wide exclusive lock serializes mutation and recovery for
    // both primary and backup credentials.
    let lock_path = provider_lock_path(root, provider);
    let _lock = CredentialFileLock::exclusive_at(&lock_path)
        .map_err(|source| io_error(&lock_path, source))?;
    recover_provider_rotation_locked(root, provider)?;

    let primary_file = CredentialFile::load(&primary_path).ok_or_else(|| {
        error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: credential_id(provider, ManagedCredentialRole::Primary),
        }
        .build()
    })?;
    let backup_file = CredentialFile::load(&backup_path).ok_or_else(|| {
        error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: credential_id(provider, ManagedCredentialRole::Backup),
        }
        .build()
    })?;

    // WHY(#4874): `load` accepts legacy plaintext credentials, which lack the
    // per-file `.json.key` sidecar encrypted credentials carry. The journaled
    // swap requires a consistent sidecar state across the pair, so migrate any
    // plaintext credential to encrypted (minting its sidecar) before rotating.
    // The provider-wide exclusive lock above serializes this migration.
    migrate_plaintext_to_encrypted(&primary_path, &primary_file)?;
    migrate_plaintext_to_encrypted(&backup_path, &backup_file)?;

    let files = prepare_rotation_journal(root, provider, &primary_path, &backup_path)?;
    commit_rotation_from_journal(&files)?;

    let mut entries = Vec::new();
    if let Some(primary) = metadata_from_path(root, provider, ManagedCredentialRole::Primary, None)?
    {
        entries.push(primary);
    }
    if let Some(backup) = metadata_from_path(root, provider, ManagedCredentialRole::Backup, None)? {
        entries.push(backup);
    }
    Ok(entries)
}

/// Re-save a credential as encrypted when it lacks its `.json.key` sidecar.
///
/// WHY: legacy plaintext credentials predate encryption-at-rest; rotating them
/// against an encrypted counterpart would otherwise trip the journal's sidecar
/// consistency invariant. Re-saving migrates the file in place and is a no-op
/// once the sidecar exists.
fn migrate_plaintext_to_encrypted(path: &Path, file: &CredentialFile) -> Result<()> {
    let key_path = path.with_extension("json.key");
    if !key_path.exists() {
        file.save(path).map_err(|source| io_error(path, source))?;
    }
    Ok(())
}

pub(crate) fn remove(root: &Path, id: &str) -> Result<()> {
    let (provider, role) = parse_id(id)?;
    recover_provider_rotation(root, &provider)?;
    let path = credential_path(root, &provider, role)?;
    if !path.exists() {
        return Err(error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: id.to_owned(),
        }
        .build());
    }

    // WHY: prevent operators from deleting the only usable credential for a
    // provider. If a backup exists, the primary may be removed; the backup is
    // still usable and can be rotated or promoted separately.
    if role == ManagedCredentialRole::Primary {
        let backup_path = credential_path(root, &provider, ManagedCredentialRole::Backup)?;
        let backup_loadable = CredentialFile::load(&backup_path).is_some();
        if !backup_loadable {
            return Err(error::RemoveLastPrimarySnafu { provider }.build());
        }
    }

    remove_file_if_exists(&path)?;
    remove_file_if_exists(&path.with_extension("json.key"))?;
    remove_file_if_exists(&path.with_extension("json.lock"))
}

fn provider_lock_path(root: &Path, provider: &str) -> PathBuf {
    root.join(format!(".{provider}.lock"))
}

#[derive(Debug)]
struct RotationFiles {
    journal: PathBuf,
    primary_copy: PathBuf,
    backup_copy: PathBuf,
    primary_key_copy: PathBuf,
    backup_key_copy: PathBuf,
    primary_commit: PathBuf,
    backup_commit: PathBuf,
    primary_key_commit: PathBuf,
    backup_key_commit: PathBuf,
    primary_path: PathBuf,
    backup_path: PathBuf,
    primary_key_path: PathBuf,
    backup_key_path: PathBuf,
    has_key_pair: bool,
}

fn rotation_files(
    root: &Path,
    provider: &str,
    primary_path: PathBuf,
    backup_path: PathBuf,
) -> RotationFiles {
    let sidecar = |label: &str| root.join(format!(".{provider}.rotate.{label}"));
    let primary_key_path = primary_path.with_extension("json.key");
    let backup_key_path = backup_path.with_extension("json.key");
    let has_key_pair = primary_key_path.exists() || backup_key_path.exists();
    RotationFiles {
        journal: rotation_journal_path(root, provider),
        primary_copy: sidecar("primary.old"),
        backup_copy: sidecar("backup.old"),
        primary_key_copy: sidecar("primary.key.old"),
        backup_key_copy: sidecar("backup.key.old"),
        primary_commit: sidecar("primary.commit"),
        backup_commit: sidecar("backup.commit"),
        primary_key_commit: sidecar("primary.key.commit"),
        backup_key_commit: sidecar("backup.key.commit"),
        primary_path,
        backup_path,
        primary_key_path,
        backup_key_path,
        has_key_pair,
    }
}

fn rotation_journal_path(root: &Path, provider: &str) -> PathBuf {
    root.join(format!(".{provider}{ROTATE_JOURNAL_SUFFIX}"))
}

fn recover_all_rotations(root: &Path) -> Result<()> {
    for entry in std::fs::read_dir(root).map_err(|source| io_error(root, source))? {
        let entry = entry.map_err(|source| io_error(root, source))?;
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(provider) = file_name
            .strip_prefix('.')
            .and_then(|name| name.strip_suffix(ROTATE_JOURNAL_SUFFIX))
        else {
            continue;
        };
        if validate_provider(provider).is_ok() {
            recover_provider_rotation(root, provider)?;
        }
    }
    Ok(())
}

fn recover_provider_rotation(root: &Path, provider: &str) -> Result<()> {
    validate_provider(provider)?;
    if !root.exists() {
        return Ok(());
    }
    let lock_path = provider_lock_path(root, provider);
    let _lock = CredentialFileLock::exclusive_at(&lock_path)
        .map_err(|source| io_error(&lock_path, source))?;
    recover_provider_rotation_locked(root, provider)
}

fn recover_provider_rotation_locked(root: &Path, provider: &str) -> Result<()> {
    let journal = rotation_journal_path(root, provider);
    if !journal.exists() {
        return Ok(());
    }
    let primary_path = credential_path(root, provider, ManagedCredentialRole::Primary)?;
    let backup_path = credential_path(root, provider, ManagedCredentialRole::Backup)?;
    let files = rotation_files(root, provider, primary_path, backup_path);
    commit_rotation_from_journal(&files)
}

fn prepare_rotation_journal(
    root: &Path,
    provider: &str,
    primary_path: &Path,
    backup_path: &Path,
) -> Result<RotationFiles> {
    let files = rotation_files(
        root,
        provider,
        primary_path.to_path_buf(),
        backup_path.to_path_buf(),
    );
    if files.primary_key_path.exists() != files.backup_key_path.exists() {
        return Err(io_error(
            root,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "credential key sidecars are inconsistent",
            ),
        ));
    }

    copy_restricted(primary_path, &files.primary_copy)
        .map_err(|source| io_error(primary_path, source))?;
    copy_restricted(backup_path, &files.backup_copy)
        .map_err(|source| io_error(backup_path, source))?;
    if files.has_key_pair {
        copy_restricted(&files.primary_key_path, &files.primary_key_copy)
            .map_err(|source| io_error(&files.primary_key_path, source))?;
        copy_restricted(&files.backup_key_path, &files.backup_key_copy)
            .map_err(|source| io_error(&files.backup_key_path, source))?;
    }

    write_restricted(&files.journal, b"rotation-v1\n")
        .map_err(|source| io_error(&files.journal, source))?;
    Ok(files)
}

fn commit_rotation_from_journal(files: &RotationFiles) -> Result<()> {
    replace_with_copy(
        &files.backup_copy,
        &files.primary_path,
        &files.primary_commit,
    )?;
    replace_with_copy(
        &files.primary_copy,
        &files.backup_path,
        &files.backup_commit,
    )?;
    if files.has_key_pair {
        replace_with_copy(
            &files.backup_key_copy,
            &files.primary_key_path,
            &files.primary_key_commit,
        )?;
        replace_with_copy(
            &files.primary_key_copy,
            &files.backup_key_path,
            &files.backup_key_commit,
        )?;
    }

    remove_file_if_exists(&files.primary_copy)?;
    remove_file_if_exists(&files.backup_copy)?;
    remove_file_if_exists(&files.primary_key_copy)?;
    remove_file_if_exists(&files.backup_key_copy)?;
    remove_file_if_exists(&files.journal)
}

fn replace_with_copy(source: &Path, destination: &Path, temp: &Path) -> Result<()> {
    copy_restricted(source, temp).map_err(|source| io_error(temp, source))?;
    std::fs::rename(temp, destination).map_err(|source| io_error(destination, source))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))
            .map_err(|source| io_error(destination, source))?;
    }
    Ok(())
}

fn copy_restricted(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    std::fs::OpenOptions::new()
        .read(true)
        .open(source)?
        .read_to_end(&mut bytes)?;
    write_restricted(destination, &bytes)
}

fn write_restricted(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()
}

fn metadata_from_path(
    root: &Path,
    provider: &str,
    role: ManagedCredentialRole,
    last_validated: Option<String>,
) -> Result<Option<ManagedCredential>> {
    let path = credential_path(root, provider, role)?;
    let Some(file) = CredentialFile::load(&path) else {
        return Ok(None);
    };
    let status = credential_status(&file);
    Ok(Some(ManagedCredential {
        id: credential_id(provider, role),
        provider: provider.to_owned(),
        role,
        redacted_preview: redact_secret(file.token.expose_secret()),
        status,
        last_validated,
    }))
}

fn credential_status(file: &CredentialFile) -> ManagedCredentialStatus {
    if file.token.expose_secret().is_empty() {
        return ManagedCredentialStatus::Expired;
    }
    if file
        .seconds_remaining()
        .is_some_and(|remaining| remaining <= 0)
    {
        return ManagedCredentialStatus::Expired;
    }
    ManagedCredentialStatus::Valid
}

fn credential_path(root: &Path, provider: &str, role: ManagedCredentialRole) -> Result<PathBuf> {
    validate_provider(provider)?;
    std::fs::create_dir_all(root).map_err(|source| io_error(root, source))?;
    let filename = match role {
        ManagedCredentialRole::Primary => format!("{provider}.json"),
        ManagedCredentialRole::Backup => format!("{provider}{BACKUP_SUFFIX}.json"),
    };
    let path = root.join(filename);
    koina::fs::validate_within_root(&path, root).map_err(|source| io_error(&path, source))
}

fn parse_path_role(path: &Path) -> Option<(String, ManagedCredentialRole)> {
    let stem = path.file_stem().and_then(|s| s.to_str())?;
    let (provider, role) = stem.strip_suffix(BACKUP_SUFFIX).map_or_else(
        || (stem, ManagedCredentialRole::Primary),
        |provider| (provider, ManagedCredentialRole::Backup),
    );
    if validate_provider(provider).is_err() {
        return None;
    }
    Some((provider.to_owned(), role))
}

fn parse_id(id: &str) -> Result<(String, ManagedCredentialRole)> {
    let Some((provider, role)) = id.split_once(':') else {
        return Err(error::InvalidApiKeySnafu.build());
    };
    validate_provider(provider)?;
    let role = role
        .parse::<ManagedCredentialRole>()
        .map_err(|_role_err| error::InvalidApiKeySnafu.build())?;
    Ok((provider.to_owned(), role))
}

fn validate_provider(provider: &str) -> Result<()> {
    let valid = !provider.is_empty()
        && provider
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if valid {
        Ok(())
    } else {
        Err(error::InvalidApiKeySnafu.build())
    }
}

fn validate_credential_secret(key: &SecretString) -> Result<()> {
    let secret = key.expose_secret();
    if secret.trim() != secret {
        return Err(error::InvalidCredentialSecretSnafu {
            reason: "credential secret must not have leading or trailing whitespace".to_owned(),
        }
        .build());
    }
    if secret.chars().count() < MIN_CREDENTIAL_SECRET_CHARS {
        return Err(error::InvalidCredentialSecretSnafu {
            reason: format!(
                "credential secret must be at least {MIN_CREDENTIAL_SECRET_CHARS} characters"
            ),
        }
        .build());
    }
    Ok(())
}

fn is_json_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(JSON_EXT)
}

fn credential_id(provider: &str, role: ManagedCredentialRole) -> String {
    format!("{provider}:{}", role.as_str())
}

fn redact_secret(secret: &str) -> String {
    if secret.chars().count() < MIN_CREDENTIAL_SECRET_CHARS {
        return REDACTED_SECRET_PLACEHOLDER.to_owned();
    }
    let tail_chars: Vec<char> = secret.chars().rev().take(4).collect();
    if tail_chars.len() == 4 {
        let tail: String = tail_chars.into_iter().rev().collect();
        format!("...{tail}")
    } else {
        REDACTED_SECRET_PLACEHOLDER.to_owned()
    }
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path, source)),
    }
}

fn io_error(path: &Path, source: std::io::Error) -> error::Error {
    error::IoSnafu {
        path: path.to_path_buf(),
    }
    .into_error(source)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn redact_secret_hides_all_short_inputs() {
        for len in 1..=8 {
            let raw = "a".repeat(len);
            let redacted = redact_secret(&raw);

            assert_eq!(redacted, REDACTED_SECRET_PLACEHOLDER);
            assert!(
                !redacted.contains(&raw),
                "redaction for {len}-character input must not contain the original"
            );
        }
    }

    #[test]
    fn redact_secret_keeps_only_tail_for_normal_provider_keys() {
        let raw = "sk-ant-api03-synthetic-secret-1234";

        let redacted = redact_secret(raw);

        assert_eq!(redacted, "...1234");
        assert!(!redacted.contains("synthetic-secret"));
        assert!(!redacted.contains("sk-ant"));
    }

    #[test]
    fn add_rejects_short_secret_before_storage() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        let short = SecretString::from("abcd1234");

        let result = add(&root, "anthropic", &short, ManagedCredentialRole::Primary);

        assert!(
            matches!(result, Err(error::Error::InvalidCredentialSecret { .. })),
            "short provider credential must fail validation, got {result:?}"
        );
        assert!(
            !root.join("anthropic.json").exists(),
            "invalid credential must not create a stored credential"
        );
    }

    #[test]
    fn add_rejects_whitespace_wrapped_secret_before_storage() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        let wrapped = SecretString::from(" sk-test-secret-1234 ");

        let result = add(&root, "anthropic", &wrapped, ManagedCredentialRole::Primary);

        assert!(
            matches!(result, Err(error::Error::InvalidCredentialSecret { .. })),
            "whitespace-wrapped provider credential must fail validation, got {result:?}"
        );
        assert!(!root.join("anthropic.json").exists());
    }

    #[test]
    fn add_list_validate_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        let raw = SecretString::from("sk-test-roundtrip-secret");

        let added = add(&root, "anthropic", &raw, ManagedCredentialRole::Backup).unwrap();
        assert_eq!(added.id, "anthropic:backup");
        assert_eq!(added.redacted_preview, "...cret");
        assert!(!added.redacted_preview.contains("roundtrip"));

        let listed = list(&root).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed.first().unwrap().redacted_preview, "...cret");

        let validated = validate(&root, "anthropic:backup").unwrap();
        assert_eq!(validated.status, ManagedCredentialStatus::Valid);
        assert!(validated.last_validated.is_some());

        remove(&root, "anthropic:backup").unwrap();
        assert!(list(&root).unwrap().is_empty());
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "test seeds a legacy plaintext credential fixture (no sidecar)"
    )]
    fn rotate_migrates_legacy_plaintext_primary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        std::fs::create_dir_all(&root).unwrap();
        // Legacy plaintext primary (no .json.key sidecar) — load() supports this.
        std::fs::write(
            root.join("anthropic.json"),
            br#"{"token":"sk-plaintext-primary"}"#,
        )
        .unwrap();
        // Encrypted backup via the normal add path (mints a .json.key sidecar).
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-encrypted-backup"),
            ManagedCredentialRole::Backup,
        )
        .unwrap();

        let rotated = rotate(&root, "anthropic").unwrap();
        assert_eq!(rotated.len(), 2);
        // The plaintext primary was migrated: both files now carry key sidecars.
        assert!(root.join("anthropic.json.key").exists());
        assert!(root.join("anthropic.backup.json.key").exists());
        // Content swapped and still loadable.
        let primary = CredentialFile::load(&root.join("anthropic.json")).unwrap();
        let backup = CredentialFile::load(&root.join("anthropic.backup.json")).unwrap();
        assert_eq!(primary.token.expose_secret(), "sk-encrypted-backup");
        assert_eq!(backup.token.expose_secret(), "sk-plaintext-primary");
        assert!(
            rotated
                .iter()
                .all(|entry| !entry.redacted_preview.contains("sk-"))
        );
    }

    #[test]
    fn rotate_swaps_primary_and_backup_without_returning_raw_secret() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-primary-1111"),
            ManagedCredentialRole::Primary,
        )
        .unwrap();
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-backup-2222"),
            ManagedCredentialRole::Backup,
        )
        .unwrap();

        let rotated = rotate(&root, "anthropic").unwrap();

        assert_eq!(rotated.len(), 2);
        let primary = CredentialFile::load(&root.join("anthropic.json")).unwrap();
        let backup = CredentialFile::load(&root.join("anthropic.backup.json")).unwrap();
        assert_eq!(primary.token.expose_secret(), "sk-backup-2222");
        assert_eq!(backup.token.expose_secret(), "sk-primary-1111");
        assert!(
            rotated
                .iter()
                .all(|entry| !entry.redacted_preview.contains("sk-"))
        );
    }

    #[test]
    fn rotate_is_idempotent_or_consistent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-primary-1111"),
            ManagedCredentialRole::Primary,
        )
        .unwrap();
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-backup-2222"),
            ManagedCredentialRole::Backup,
        )
        .unwrap();

        rotate(&root, "anthropic").unwrap();
        rotate(&root, "anthropic").unwrap();

        let primary = CredentialFile::load(&root.join("anthropic.json")).unwrap();
        let backup = CredentialFile::load(&root.join("anthropic.backup.json")).unwrap();
        let primary_secret = primary.token.expose_secret();
        let backup_secret = backup.token.expose_secret();
        assert!(
            (primary_secret == "sk-primary-1111" && backup_secret == "sk-backup-2222")
                || (primary_secret == "sk-backup-2222" && backup_secret == "sk-primary-1111"),
            "after two rotations the pair must be coherent, got primary={primary_secret} backup={backup_secret}"
        );
    }

    #[test]
    fn rotate_recovers_after_partial_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-primary-1111"),
            ManagedCredentialRole::Primary,
        )
        .unwrap();
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-backup-2222"),
            ManagedCredentialRole::Backup,
        )
        .unwrap();

        let primary_path = credential_path(&root, "anthropic", ManagedCredentialRole::Primary)
            .expect("primary path");
        let backup_path = credential_path(&root, "anthropic", ManagedCredentialRole::Backup)
            .expect("backup path");
        let files =
            prepare_rotation_journal(&root, "anthropic", &primary_path, &backup_path).unwrap();

        replace_with_copy(
            &files.backup_copy,
            &files.primary_path,
            &files.primary_commit,
        )
        .unwrap();
        replace_with_copy(
            &files.backup_key_copy,
            &files.primary_key_path,
            &files.primary_key_commit,
        )
        .unwrap();

        let before_recovery_primary = CredentialFile::load(&primary_path).unwrap();
        assert_eq!(
            before_recovery_primary.token.expose_secret(),
            "sk-backup-2222"
        );

        recover_provider_rotation(&root, "anthropic").unwrap();

        let primary = CredentialFile::load(&primary_path).unwrap();
        let backup = CredentialFile::load(&backup_path).unwrap();
        assert_eq!(primary.token.expose_secret(), "sk-backup-2222");
        assert_eq!(backup.token.expose_secret(), "sk-primary-1111");
        assert!(
            !files.journal.exists(),
            "journal must be removed after recovery completes"
        );
    }

    #[test]
    fn remove_last_primary_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-primary-1111"),
            ManagedCredentialRole::Primary,
        )
        .unwrap();

        let result = remove(&root, "anthropic:primary");
        assert!(
            matches!(result, Err(error::Error::RemoveLastPrimary { .. })),
            "removing the only usable credential for a provider must fail, got {result:?}"
        );
    }

    #[test]
    fn remove_succeeds_when_backup_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-primary-1111"),
            ManagedCredentialRole::Primary,
        )
        .unwrap();
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-backup-2222"),
            ManagedCredentialRole::Backup,
        )
        .unwrap();

        remove(&root, "anthropic:primary").expect("removing primary with a backup must succeed");
        assert!(CredentialFile::load(&root.join("anthropic.json")).is_none());
        assert!(CredentialFile::load(&root.join("anthropic.backup.json")).is_some());
    }

    #[test]
    fn add_duplicate_returns_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        add(
            &root,
            "anthropic",
            &SecretString::from("sk-first-1111"),
            ManagedCredentialRole::Primary,
        )
        .unwrap();

        let result = add(
            &root,
            "anthropic",
            &SecretString::from("sk-second"),
            ManagedCredentialRole::Primary,
        );
        assert!(
            matches!(result, Err(error::Error::Duplicate { .. })),
            "adding the same credential twice must fail with Duplicate, got {result:?}"
        );

        let primary = CredentialFile::load(&root.join("anthropic.json")).unwrap();
        assert_eq!(
            primary.token.expose_secret(),
            "sk-first-1111",
            "duplicate add must not overwrite the existing credential"
        );
    }
}
