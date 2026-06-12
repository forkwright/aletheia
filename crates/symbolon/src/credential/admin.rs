//! Operator credential management over the instance credential directory.

use std::path::{Path, PathBuf};

use koina::secret::SecretString;
use snafu::IntoError;

use crate::error::{self, Result};
use crate::types::{ManagedCredential, ManagedCredentialRole, ManagedCredentialStatus};

use super::CredentialFile;

const BACKUP_SUFFIX: &str = ".backup";
const JSON_EXT: &str = "json";

pub(crate) fn list(root: &Path) -> Result<Vec<ManagedCredential>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

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
    let path = credential_path(root, provider, role)?;
    if path.exists() {
        return Err(error::DuplicateSnafu {
            entity: "credential".to_owned(),
            id: credential_id(provider, role),
        }
        .build());
    }

    let credential = CredentialFile {
        token: key.clone(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    credential
        .save(&path)
        .map_err(|source| io_error(&path, source))?;
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

    let primary = CredentialFile::load(&primary_path).ok_or_else(|| {
        error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: credential_id(provider, ManagedCredentialRole::Primary),
        }
        .build()
    })?;
    let backup = CredentialFile::load(&backup_path).ok_or_else(|| {
        error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: credential_id(provider, ManagedCredentialRole::Backup),
        }
        .build()
    })?;

    backup
        .save(&primary_path)
        .map_err(|source| io_error(&primary_path, source))?;
    primary
        .save(&backup_path)
        .map_err(|source| io_error(&backup_path, source))?;

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

pub(crate) fn remove(root: &Path, id: &str) -> Result<()> {
    let (provider, role) = parse_id(id)?;
    let path = credential_path(root, &provider, role)?;
    if !path.exists() {
        return Err(error::NotFoundSnafu {
            entity: "credential".to_owned(),
            id: id.to_owned(),
        }
        .build());
    }

    remove_file_if_exists(&path)?;
    remove_file_if_exists(&path.with_extension("json.key"))?;
    remove_file_if_exists(&path.with_extension("json.lock"))
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

fn is_json_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(JSON_EXT)
}

fn credential_id(provider: &str, role: ManagedCredentialRole) -> String {
    format!("{provider}:{}", role.as_str())
}

fn redact_secret(secret: &str) -> String {
    let tail_chars: Vec<char> = secret.chars().rev().take(4).collect();
    if tail_chars.len() == 4 {
        let tail: String = tail_chars.into_iter().rev().collect();
        format!("...{tail}")
    } else {
        "...????".to_owned()
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

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
}
