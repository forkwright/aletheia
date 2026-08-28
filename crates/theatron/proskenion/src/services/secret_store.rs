//! Desktop bearer-token secret storage.
//!
//! Thin adapter over the shared [`skene::secret_store::TokenStore`] core
//! (#7027), which owns the keyring/fallback state machine, cipher framing,
//! key handling, and secure atomic writes. Unlike koilon's single fixed
//! connection, proskenion addresses multiple server token references, so
//! every call here takes a caller-supplied `token_ref` rather than a fixed
//! constant.

use std::path::Path;

use koina::secret::SecretString;
pub(crate) use skene::secret_store::SecretStoreError;
use skene::secret_store::TokenStore;

fn keyring_enabled() -> bool {
    !cfg!(test)
}

const STORE: TokenStore = TokenStore::new(
    "aletheia-desktop",
    "aletheia-desktop",
    "ALETHEIA_DESKTOP_TOKEN_V1:",
    keyring_enabled,
);

/// Store a bearer token in the OS keyring or encrypted fallback.
///
/// # Errors
///
/// Returns an error if the OS keyring is unavailable and encrypted fallback
/// storage cannot be written.
pub(crate) fn store_token(
    base: &Path,
    token_ref: &str,
    token: &str,
) -> Result<(), SecretStoreError> {
    STORE.store_token(base, token_ref, token)
}

/// Load a bearer token by reference.
///
/// # Errors
///
/// Returns an error if encrypted fallback data exists but cannot be read or
/// decrypted.
pub(crate) fn load_token(
    base: &Path,
    token_ref: &str,
) -> Result<Option<SecretString>, SecretStoreError> {
    STORE.load_token(base, token_ref)
}

/// Delete a bearer token from keyring and fallback storage.
///
/// # Errors
///
/// Returns an error only when removing local encrypted fallback files fails.
pub(crate) fn delete_token(base: &Path, token_ref: &str) -> Result<(), SecretStoreError> {
    STORE.delete_token(base, token_ref)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn fallback_file(base: &Path, token_ref: &str) -> std::path::PathBuf {
        base.join("aletheia-desktop")
            .join("secrets")
            .join(format!("{token_ref}.token"))
    }

    fn fallback_key_file(base: &Path, token_ref: &str) -> std::path::PathBuf {
        base.join("aletheia-desktop")
            .join("secrets")
            .join(format!("{token_ref}.key"))
    }

    #[test]
    fn fallback_round_trip_does_not_store_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_test";
        let token = "bearer-secret-4491";

        store_token(base, token_ref, token).unwrap();
        let restored = load_token(base, token_ref).unwrap();

        assert_eq!(
            restored.as_ref().map(SecretString::expose_secret),
            Some(token)
        );
        let raw = std::fs::read_to_string(fallback_file(base, token_ref)).unwrap();
        assert!(!raw.contains(token));
        assert!(raw.starts_with("ALETHEIA_DESKTOP_TOKEN_V1:"));
    }

    /// The secret file is owner-only and its directory owner-only-traversable,
    /// on a replacement as well as a first write.
    #[cfg(unix)]
    #[test]
    fn stored_secret_is_owner_only_on_every_write() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_modes";

        store_token(base, token_ref, "first").unwrap();

        let path = fallback_file(base, token_ref);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        store_token(base, token_ref, "second").unwrap();

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            file_mode & 0o777,
            0o600,
            "secret file expected 0o600, got {:o}",
            file_mode & 0o777
        );

        let parent = path.parent().unwrap();
        let dir_mode = std::fs::metadata(parent).unwrap().permissions().mode();
        assert_eq!(
            dir_mode & 0o777,
            0o700,
            "secret dir expected 0o700, got {:o}",
            dir_mode & 0o777
        );
    }

    #[test]
    fn delete_token_removes_fallback_files() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_delete";

        store_token(base, token_ref, "secret").unwrap();
        assert!(fallback_file(base, token_ref).exists());
        assert!(fallback_key_file(base, token_ref).exists());

        delete_token(base, token_ref).unwrap();

        assert!(!fallback_file(base, token_ref).exists());
        assert!(!fallback_key_file(base, token_ref).exists());
    }
}
