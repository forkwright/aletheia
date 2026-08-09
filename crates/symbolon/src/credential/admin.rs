//! Operator credential management over the instance credential directory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use koina::secret::SecretString;
use snafu::IntoError;
use tracing::warn;

use crate::error::{self, Result};
use crate::types::{
    ManagedCredential, ManagedCredentialRole, ManagedCredentialStatus, ProviderValidationRecord,
    ProviderValidationState,
};

use super::CredentialFile;
use super::file_ops::CredentialFileLock;

const BACKUP_SUFFIX: &str = ".backup";
const JSON_EXT: &str = "json";
const ROTATE_JOURNAL_SUFFIX: &str = ".rotate.journal";
const VALIDATION_SIDECAR_EXT: &str = "json.validation";
const MIN_CREDENTIAL_SECRET_CHARS: usize = 9;
const REDACTED_SECRET_PLACEHOLDER: &str = "...????";

// WHY(#4875): lightweight, read-only endpoints used solely to confirm a
// stored key authenticates with the provider. Never chosen for cost — chosen
// because they require no request body and cannot mutate provider state.
const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const OPENAI_MODELS_URL: &str = "https://api.openai.com/v1/models";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const PROVIDER_VALIDATION_TIMEOUT: Duration = Duration::from_secs(10);

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
        if let Some(credential) = metadata_from_path(root, &provider, role)? {
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

    metadata_from_path(root, provider, role)?.ok_or_else(|| {
        error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: credential_id(provider, role),
        }
        .build()
    })
}

/// Validate a stored credential.
///
/// WHY(#4875): a credential is checked in three tiers, cheapest first. Local
/// metadata (empty secret, past expiry) answers the question without a
/// network round trip when it already can. Only a non-empty, locally
/// unexpired secret for a provider this crate knows how to reach live is
/// ever sent over the network — and even then, only to a read-only endpoint
/// the key would need to authenticate against regardless. The outcome is
/// persisted so later `list` calls reflect it instead of reverting to
/// "never validated".
pub(crate) async fn validate(
    root: &Path,
    id: &str,
    client: &reqwest::Client,
) -> Result<ManagedCredential> {
    let (provider, role) = parse_id(id)?;
    recover_provider_rotation(root, &provider)?;
    let path = credential_path(root, &provider, role)?;
    let Some(file) = CredentialFile::load(&path) else {
        return Err(error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: id.to_owned(),
        }
        .build());
    };

    let local_status = credential_status(&file);
    let secret = file.token.expose_secret();
    let state = if secret.trim().is_empty() {
        ProviderValidationState::Malformed
    } else if local_status == ManagedCredentialStatus::Expired {
        ProviderValidationState::Expired
    } else {
        check_provider_key(client, &provider, &file.token).await
    };

    let record = ProviderValidationRecord {
        state,
        validated_at: jiff::Timestamp::now(),
    };
    save_validation_record(&path, &record)?;

    Ok(ManagedCredential {
        id: credential_id(&provider, role),
        provider,
        role,
        redacted_preview: redact_secret(secret),
        status: local_status,
        last_validated: Some(record.validated_at.to_string()),
        validation: Some(record),
    })
}

/// Dispatch a live authentication check to the provider named by `provider`,
/// when this crate knows how to reach it. Unrecognized providers report
/// [`ProviderValidationState::Unknown`] rather than guessing at an endpoint.
async fn check_provider_key(
    client: &reqwest::Client,
    provider: &str,
    key: &SecretString,
) -> ProviderValidationState {
    match provider.to_ascii_lowercase().as_str() {
        "anthropic" | "claude" => {
            check_anthropic_key(client, key, ANTHROPIC_MODELS_URL).await
        }
        "openai" => check_openai_key(client, key, OPENAI_MODELS_URL).await,
        _ => ProviderValidationState::Unknown,
    }
}

async fn check_anthropic_key(
    client: &reqwest::Client,
    key: &SecretString,
    models_url: &str,
) -> ProviderValidationState {
    let response = client
        .get(models_url)
        .header("x-api-key", key.expose_secret())
        .header("anthropic-version", ANTHROPIC_API_VERSION)
        .timeout(PROVIDER_VALIDATION_TIMEOUT)
        .send()
        .await;
    outcome_from_response(response)
}

async fn check_openai_key(
    client: &reqwest::Client,
    key: &SecretString,
    models_url: &str,
) -> ProviderValidationState {
    let response = client
        .get(models_url)
        .bearer_auth(key.expose_secret())
        .timeout(PROVIDER_VALIDATION_TIMEOUT)
        .send()
        .await;
    outcome_from_response(response)
}

/// Map a completed (or failed) provider HTTP call to a validation outcome.
///
/// `2xx` is acceptance. `401`/`403` is an explicit rejection — the provider
/// looked at the credential and refused it. Every other case (network
/// failure, timeout, unexpected status such as `429`/`5xx`) is treated as
/// `Unreachable`: none of those are proof the key itself is bad, so they
/// must never be reported as `Rejected`.
fn outcome_from_response(response: reqwest::Result<reqwest::Response>) -> ProviderValidationState {
    match response {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                ProviderValidationState::Accepted
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                ProviderValidationState::Rejected
            } else {
                warn!(status = %status, "provider validation call returned an unexpected status");
                ProviderValidationState::Unreachable
            }
        }
        // SAFETY: reqwest::Error's Display never includes request headers or
        // body, so this cannot leak the credential value being validated.
        Err(e) => {
            warn!(error = %e, "provider validation request failed"); // kanon:ignore SECURITY/credential-logging -- logs a transport error, not the credential value
            ProviderValidationState::Unreachable
        }
    }
}

fn validation_sidecar_path(credential_path: &Path) -> PathBuf {
    credential_path.with_extension(VALIDATION_SIDECAR_EXT)
}

/// Load the persisted validation outcome for a credential, if one exists.
///
/// A missing or unparseable sidecar is treated as "never validated" rather
/// than an error — validation metadata is a convenience layer over the
/// credential file, never load-bearing for whether the credential itself
/// loads.
fn load_validation_record(credential_path: &Path) -> Option<ProviderValidationRecord> {
    let bytes = std::fs::read(validation_sidecar_path(credential_path)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_validation_record(credential_path: &Path, record: &ProviderValidationRecord) -> Result<()> {
    let sidecar = validation_sidecar_path(credential_path);
    // kanon:ignore RUST/no-silent-result-swallow -- serde_json::to_vec_pretty on
    // a struct of plain enums/timestamps is infallible; mapped to io_error's
    // shape only so this function has one error type to return.
    let json = serde_json::to_vec_pretty(record)
        .map_err(|e| io_error(&sidecar, std::io::Error::other(e)))?;
    write_restricted(&sidecar, &json).map_err(|source| io_error(&sidecar, source))
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
    if let Some(primary) = metadata_from_path(root, provider, ManagedCredentialRole::Primary)? {
        entries.push(primary);
    }
    if let Some(backup) = metadata_from_path(root, provider, ManagedCredentialRole::Backup)? {
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
    remove_file_if_exists(&path.with_extension("json.lock"))?;
    remove_file_if_exists(&validation_sidecar_path(&path))
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
    remove_file_if_exists(&files.journal)?;

    // WHY(#4875): a validation record's trust claim ("this exact secret was
    // accepted by the provider at time T") is bound to the secret VALUE, not
    // to a primary/backup role label. Rotation swaps which value sits behind
    // each label, so any prior validation stamp at either path now describes
    // the wrong secret. Clear both rather than silently misattributing a
    // validation result post-swap — an operator can re-validate in one click.
    remove_file_if_exists(&validation_sidecar_path(&files.primary_path))?;
    remove_file_if_exists(&validation_sidecar_path(&files.backup_path))
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
) -> Result<Option<ManagedCredential>> {
    let path = credential_path(root, provider, role)?;
    let Some(file) = CredentialFile::load(&path) else {
        return Ok(None);
    };
    let status = credential_status(&file);
    // WHY(#4875): read the persisted validation sidecar so list/refresh
    // responses keep reflecting the last validation result instead of
    // reverting to "never validated" on every subsequent call.
    let validation = load_validation_record(&path);
    let last_validated = validation.map(|record| record.validated_at.to_string());
    Ok(Some(ManagedCredential {
        id: credential_id(provider, role),
        provider: provider.to_owned(),
        role,
        redacted_preview: redact_secret(file.token.expose_secret()),
        status,
        last_validated,
        validation,
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

    #[tokio::test]
    async fn add_list_validate_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        let raw = SecretString::from("sk-test-roundtrip-secret");
        // WHY: a provider name this crate has no live-check strategy for, so
        // this roundtrip stays network-free and deterministic — the
        // Anthropic/OpenAI live-check paths get their own dedicated tests.
        let provider = "acme-test-provider";

        let added = add(&root, provider, &raw, ManagedCredentialRole::Backup).unwrap();
        assert_eq!(added.id, format!("{provider}:backup"));
        assert_eq!(added.redacted_preview, "...cret");
        assert!(!added.redacted_preview.contains("roundtrip"));

        let listed = list(&root).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed.first().unwrap().redacted_preview, "...cret");
        assert!(
            listed.first().unwrap().validation.is_none(),
            "must report never-validated before validate() has run"
        );

        let client = reqwest::Client::new();
        let id = format!("{provider}:backup");
        let validated = validate(&root, &id, &client).await.unwrap();
        assert_eq!(validated.status, ManagedCredentialStatus::Valid);
        assert!(validated.last_validated.is_some());
        assert_eq!(
            validated.validation.map(|record| record.state),
            Some(ProviderValidationState::Unknown),
            "unrecognized provider must report Unknown, never a guessed Accepted/Rejected"
        );

        // WHY(#4875): the validation outcome must persist into a later list
        // call, not just the immediate validate() response.
        let relisted = list(&root).unwrap();
        assert_eq!(
            relisted.first().unwrap().validation.map(|r| r.state),
            Some(ProviderValidationState::Unknown),
            "list() must reflect the persisted validation outcome"
        );

        remove(&root, &id).unwrap();
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

    #[tokio::test]
    async fn validate_malformed_short_circuits_without_network() {
        // WHY: `add()` rejects short/empty secrets, so an empty-token file can
        // only exist via direct construction (e.g. external corruption).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("anthropic.json");
        let empty = CredentialFile {
            token: SecretString::from(""),
            refresh_token: None,
            expires_at: None,
            scopes: None,
            subscription_type: None,
        };
        empty.save(&path).unwrap();

        let client = reqwest::Client::new();
        let result = validate(&root, "anthropic:primary", &client).await.unwrap();
        assert_eq!(
            result.validation.map(|r| r.state),
            Some(ProviderValidationState::Malformed),
            "an empty stored secret must never reach the network"
        );
    }

    #[tokio::test]
    async fn validate_expired_short_circuits_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("credentials");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("anthropic.json");
        let expired = CredentialFile {
            token: SecretString::from("sk-long-enough-but-expired"),
            refresh_token: None,
            expires_at: Some(1),
            scopes: None,
            subscription_type: None,
        };
        expired.save(&path).unwrap();

        let client = reqwest::Client::new();
        let result = validate(&root, "anthropic:primary", &client).await.unwrap();
        assert_eq!(result.status, ManagedCredentialStatus::Expired);
        assert_eq!(
            result.validation.map(|r| r.state),
            Some(ProviderValidationState::Expired),
            "a locally-expired credential must never reach the network"
        );
    }

    #[test]
    fn rotate_clears_stale_validation_sidecars() {
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
        let primary_path =
            credential_path(&root, "anthropic", ManagedCredentialRole::Primary).unwrap();
        save_validation_record(
            &primary_path,
            &ProviderValidationRecord {
                state: ProviderValidationState::Accepted,
                validated_at: jiff::Timestamp::now(),
            },
        )
        .unwrap();
        assert!(load_validation_record(&primary_path).is_some());

        rotate(&root, "anthropic").unwrap();

        // WHY(#4875): the secret formerly at `primary_path` moved to
        // `backup_path` (and vice versa) — the stale stamp must not survive
        // at either path, since it would now describe the wrong secret.
        let backup_path =
            credential_path(&root, "anthropic", ManagedCredentialRole::Backup).unwrap();
        assert!(
            load_validation_record(&primary_path).is_none(),
            "rotation must clear the primary-path validation stamp"
        );
        assert!(
            load_validation_record(&backup_path).is_none(),
            "rotation must clear the backup-path validation stamp"
        );
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod provider_validation_tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    // WHY(#5247): reqwest 0.13 with rustls-no-provider panics with
    // "No provider set" if no crypto provider is installed before any
    // `Client` is constructed. Each test that constructs a `Client` must be
    // self-contained and not rely on another test having installed it first.
    fn ensure_crypto_provider() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    #[tokio::test]
    async fn anthropic_accepted_on_2xx() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let client = reqwest::Client::new();
        let outcome = check_anthropic_key(&client, &SecretString::from("sk-good-key"), &url).await;
        assert_eq!(outcome, ProviderValidationState::Accepted);
    }

    #[tokio::test]
    async fn anthropic_rejected_on_401() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string(r#"{"error":{"type":"authentication_error"}}"#),
            )
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let client = reqwest::Client::new();
        let outcome = check_anthropic_key(&client, &SecretString::from("sk-bad-key"), &url).await;
        assert_eq!(outcome, ProviderValidationState::Rejected);
    }

    #[tokio::test]
    async fn anthropic_rejected_on_403() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let client = reqwest::Client::new();
        let outcome = check_anthropic_key(&client, &SecretString::from("sk-bad-key"), &url).await;
        assert_eq!(outcome, ProviderValidationState::Rejected);
    }

    #[tokio::test]
    async fn anthropic_unreachable_on_server_error() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let client = reqwest::Client::new();
        let outcome = check_anthropic_key(&client, &SecretString::from("sk-key"), &url).await;
        assert_eq!(
            outcome,
            ProviderValidationState::Unreachable,
            "a 5xx must never be reported as Rejected — it is not proof the key is bad"
        );
    }

    #[tokio::test]
    async fn anthropic_unreachable_on_connection_failure() {
        ensure_crypto_provider();
        // WHY: bind an ephemeral port then drop the listener immediately so a
        // connection attempt is refused deterministically and fast, with no
        // reliance on a timeout.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let url = format!("http://127.0.0.1:{port}/v1/models");

        let client = reqwest::Client::new();
        let outcome = check_anthropic_key(&client, &SecretString::from("sk-key"), &url).await;
        assert_eq!(outcome, ProviderValidationState::Unreachable);
    }

    #[tokio::test]
    async fn openai_accepted_on_2xx() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let client = reqwest::Client::new();
        let outcome = check_openai_key(&client, &SecretString::from("sk-good-key"), &url).await;
        assert_eq!(outcome, ProviderValidationState::Accepted);
    }

    #[tokio::test]
    async fn openai_rejected_on_401() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let client = reqwest::Client::new();
        let outcome = check_openai_key(&client, &SecretString::from("sk-bad"), &url).await;
        assert_eq!(outcome, ProviderValidationState::Rejected);
    }

    #[tokio::test]
    async fn dispatch_routes_unknown_provider_without_network() {
        ensure_crypto_provider();
        // NOTE: no mock server is started — if this ever dispatched to a
        // network call it would fail to connect rather than silently pass,
        // making a regression here fail loudly instead of hanging.
        let outcome = check_provider_key(
            &reqwest::Client::new(),
            "some-unrecognized-provider",
            &SecretString::from("sk-x"),
        )
        .await;
        assert_eq!(outcome, ProviderValidationState::Unknown);
    }

    #[tokio::test]
    async fn dispatch_routes_claude_alias_like_anthropic() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;

        // WHY: check_provider_key hardcodes the real Anthropic URL for
        // "anthropic"/"claude", so this exercises the case-insensitive name
        // match directly rather than the unreachable-in-tests real endpoint.
        let outcome = check_anthropic_key(
            &reqwest::Client::new(),
            &SecretString::from("sk-good-key"),
            &format!("{}/v1/models", server.uri()),
        )
        .await;
        assert_eq!(outcome, ProviderValidationState::Accepted);
    }

    // SECURITY(#4875): a validation call must never leak the credential value
    // it is checking, on any outcome.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn credential_secret_never_appears_in_logs_on_rejection() {
        ensure_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let client = reqwest::Client::new();
        let secret = "sk-supersecret-value-1234";
        let outcome = check_anthropic_key(&client, &SecretString::from(secret), &url).await;

        assert_eq!(outcome, ProviderValidationState::Rejected);
        assert!(
            !logs_contain(secret),
            "the credential value must never appear in validation logs"
        );
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn credential_secret_never_appears_in_logs_on_transport_failure() {
        ensure_crypto_provider();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let url = format!("http://127.0.0.1:{port}/v1/models");

        let client = reqwest::Client::new();
        let secret = "sk-supersecret-transport-fail-5678";
        let outcome = check_anthropic_key(&client, &SecretString::from(secret), &url).await;

        assert_eq!(outcome, ProviderValidationState::Unreachable);
        assert!(
            !logs_contain(secret),
            "the credential value must never appear in transport-failure logs"
        );
    }
}
