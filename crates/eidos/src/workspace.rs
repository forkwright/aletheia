//! Workspace and project identity primitives.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Stable project partition derived from a canonical remote URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    /// Build a project ID from a Git remote URL.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectIdError::EmptyRemote`] when the remote URL is empty
    /// after trimming whitespace.
    pub fn from_git_remote(remote_url: impl AsRef<str>) -> Result<Self, ProjectIdError> {
        let normalized = normalize_remote_url(remote_url.as_ref())?;
        let digest = Sha256::digest(normalized.as_bytes());
        let mut id = String::with_capacity(64);
        for byte in digest {
            id.push(hex_digit(byte >> 4));
            id.push(hex_digit(byte & 0x0f));
        }
        Ok(Self(id))
    }

    /// Build a project ID from its stored SHA-256 lowercase hex form.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectIdError::InvalidHex`] when the value is not exactly
    /// 64 ASCII hex characters.
    pub fn from_sha256_hex(value: impl AsRef<str>) -> Result<Self, ProjectIdError> {
        let trimmed = value.as_ref().trim();
        if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ProjectIdError::InvalidHex);
        }
        Ok(Self(trimmed.to_ascii_lowercase()))
    }

    /// The lowercase SHA-256 hex identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ProjectId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Project ID derivation errors.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectIdError {
    /// The remote URL was empty after trimming whitespace.
    EmptyRemote,
    /// The stored project ID was not a 64-character SHA-256 hex string.
    InvalidHex,
}

impl fmt::Display for ProjectIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRemote => f.write_str("git remote URL cannot be empty"),
            Self::InvalidHex => f.write_str("project ID must be 64 hex characters"),
        }
    }
}

impl std::error::Error for ProjectIdError {}

fn normalize_remote_url(remote_url: &str) -> Result<&str, ProjectIdError> {
    let trimmed = remote_url.trim();
    if trimmed.is_empty() {
        return Err(ProjectIdError::EmptyRemote);
    }
    Ok(trimmed)
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '?',
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn project_id_is_stable_for_same_remote() {
        let first = ProjectId::from_git_remote("https://github.com/forkwright/aletheia.git")
            .expect("valid remote");
        let second = ProjectId::from_git_remote(" https://github.com/forkwright/aletheia.git ")
            .expect("valid remote");

        assert_eq!(first, second);
        assert_eq!(first.as_str().len(), 64);
        assert!(first.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn project_id_changes_by_remote() {
        let aletheia = ProjectId::from_git_remote("https://github.com/forkwright/aletheia.git")
            .expect("valid");
        let kanon =
            ProjectId::from_git_remote("https://github.com/forkwright/kanon.git").expect("valid");

        assert_ne!(aletheia, kanon);
    }

    #[test]
    fn empty_remote_is_rejected() {
        assert!(matches!(
            ProjectId::from_git_remote("   "),
            Err(ProjectIdError::EmptyRemote)
        ));
    }

    #[test]
    fn project_id_serializes_as_string() {
        let id = ProjectId::from_git_remote("https://github.com/forkwright/aletheia")
            .expect("valid remote");

        let json = serde_json::to_string(&id).expect("serialize");
        let back: ProjectId = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(id, back);
    }

    #[test]
    fn project_id_constructs_from_stored_hex() {
        let id = ProjectId::from_git_remote("https://github.com/forkwright/aletheia")
            .expect("valid remote");
        let back = ProjectId::from_sha256_hex(id.as_str().to_ascii_uppercase())
            .expect("stored hex should parse");

        assert_eq!(id, back);
    }
}
