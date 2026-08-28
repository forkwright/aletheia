//! TUI bearer-token secret storage (#5321).
//!
//! Thin adapter over the shared [`skene::secret_store::TokenStore`] core
//! (#7027), which owns the keyring/fallback state machine, cipher framing,
//! key handling, and secure atomic writes. koilon manages exactly one
//! connection at a time (unlike proskenion's multi-server desktop store),
//! so there is a single fixed keyring account and fallback file pair
//! rather than a per-server reference — [`TOKEN_REF`] is that fixed
//! reference, passed to every `TokenStore` call.

use std::path::Path;

use koina::secret::SecretString;
pub(crate) use skene::secret_store::SecretStoreError;
use skene::secret_store::TokenStore;

/// The keyring account name and fallback-file stem for the single TUI
/// connection token. Also the value written into `ConfigFile.token_ref` to
/// mark that a token is stored via this module rather than in plaintext.
pub(crate) const TOKEN_REF: &str = "tui-default";

fn keyring_enabled() -> bool {
    !cfg!(test)
}

const STORE: TokenStore = TokenStore::new(
    "aletheia-tui",
    "aletheia",
    "ALETHEIA_TUI_TOKEN_V1:",
    keyring_enabled,
);

/// Store the TUI bearer token in the OS keyring or encrypted fallback.
///
/// # Errors
///
/// Returns an error if the OS keyring is unavailable and encrypted fallback
/// storage cannot be written.
pub(crate) fn store_token(base: &Path, token: &str) -> Result<(), SecretStoreError> {
    STORE.store_token(base, TOKEN_REF, token)
}

/// Load the TUI bearer token.
///
/// # Errors
///
/// Returns an error if encrypted fallback data exists but cannot be read or
/// decrypted.
pub(crate) fn load_token(base: &Path) -> Result<Option<SecretString>, SecretStoreError> {
    STORE.load_token(base, TOKEN_REF)
}

/// Delete the TUI bearer token from keyring and fallback storage.
///
/// # Errors
///
/// Returns an error only when removing local encrypted fallback files fails.
pub(crate) fn delete_token(base: &Path) -> Result<(), SecretStoreError> {
    STORE.delete_token(base, TOKEN_REF)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn fallback_round_trip_does_not_store_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token = "bearer-secret-4491";

        store_token(base, token).unwrap();
        let restored = load_token(base).unwrap();

        assert_eq!(
            restored.as_ref().map(SecretString::expose_secret),
            Some(token)
        );
    }

    #[cfg(unix)]
    #[test]
    fn stored_secret_is_owner_only_on_every_write() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        store_token(base, "first").unwrap();

        let path = base
            .join("aletheia")
            .join("secrets")
            .join(format!("{TOKEN_REF}.token"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        store_token(base, "second").unwrap();

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            file_mode & 0o777,
            0o600,
            "secret file expected 0o600, got {:o}",
            file_mode & 0o777
        );
    }

    #[test]
    fn delete_token_removes_fallback_files() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let secrets_dir = base.join("aletheia").join("secrets");

        store_token(base, "secret").unwrap();
        assert!(secrets_dir.join(format!("{TOKEN_REF}.token")).exists());
        assert!(secrets_dir.join(format!("{TOKEN_REF}.key")).exists());

        delete_token(base).unwrap();

        assert!(!secrets_dir.join(format!("{TOKEN_REF}.token")).exists());
        assert!(!secrets_dir.join(format!("{TOKEN_REF}.key")).exists());
    }

    #[test]
    fn load_token_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_token(dir.path()).unwrap().is_none());
    }
}
