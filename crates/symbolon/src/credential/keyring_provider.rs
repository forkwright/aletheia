//! OS keyring credential provider (behind the `keyring` feature).

use std::path::Path;

use tracing::{debug, warn};

use koina::credential::{Credential, CredentialProvider, CredentialSource};
use koina::secret::SecretString;

const DEFAULT_SERVICE: &str = "aletheia";
const DEFAULT_USERNAME: &str = "api-token";

/// Reads credentials from the OS keyring (GNOME Keyring, macOS Keychain,
/// Windows Credential Manager).
///
/// Falls through silently when the keyring is unavailable (headless server,
/// no D-Bus session, locked keychain) so downstream providers get a chance.
pub struct KeyringCredentialProvider {
    service: String,
    username: String,
}

impl KeyringCredentialProvider {
    /// Create a provider using the fixed global service name (`aletheia`)
    /// and username (`api-token`).
    ///
    /// WARNING(#5250): NOT namespaced to any instance or credential
    /// provider. Every deployment on the machine that constructs a bare
    /// `new()` reads and writes the SAME keyring entry, so a stale token
    /// left by another install, account, or role silently answers here.
    /// Production call sites must use
    /// [`for_instance`](Self::for_instance) instead; this constructor
    /// exists only for the pre-namespacing default and direct tests.
    #[must_use]
    pub fn new() -> Self {
        Self {
            service: DEFAULT_SERVICE.to_owned(),
            username: DEFAULT_USERNAME.to_owned(),
        }
    }

    /// Create a provider namespaced to one Aletheia instance and one
    /// credential-provider name (e.g. `"anthropic"`).
    ///
    /// WHY(#5250): a fixed global service/user identity let a stale keyring
    /// entry from another install, account, or role silently override this
    /// instance's intended credential. Namespacing the keyring service by
    /// `provider_name` and the account by the instance's oikos root makes
    /// every (instance, provider) pair its own keyring entry, so entries
    /// cannot collide across deployments co-installed on one machine.
    ///
    /// `instance_root` should be the instance's oikos root path -- stable
    /// across restarts of the same deployment, distinct across deployments.
    #[must_use]
    pub fn for_instance(instance_root: &Path, provider_name: &str) -> Self {
        Self {
            service: format!("{DEFAULT_SERVICE}:{provider_name}"),
            username: instance_root.display().to_string(),
        }
    }

    /// Create a provider with custom service and username identifiers.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_identifiers(
        service: impl Into<String>,
        username: impl Into<String>,
    ) -> Self {
        Self {
            service: service.into(),
            username: username.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(&self.service, &self.username)
    }

    /// Store a token in the OS keyring.
    ///
    /// # Errors
    ///
    /// Returns the keyring error if the backend is unavailable or the
    /// write fails (e.g. user denied access).
    pub fn store(&self, token: &str) -> Result<(), keyring::Error> {
        self.entry()?.set_password(token)
    }

    /// Remove the stored credential from the OS keyring.
    ///
    /// # Errors
    ///
    /// Returns the keyring error if the backend is unavailable or deletion
    /// fails. `NoEntry` errors are mapped to `Ok(())` since the goal state
    /// (no credential present) is already achieved.
    pub fn delete(&self) -> Result<(), keyring::Error> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

impl Default for KeyringCredentialProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialProvider for KeyringCredentialProvider {
    fn get_credential(&self) -> Option<Credential> {
        let entry = match self.entry() {
            Ok(e) => e,
            Err(e) => {
                debug!(error = %e, "keyring entry creation failed, skipping");
                return None;
            }
        };

        match entry.get_password() {
            Ok(token) if token.is_empty() => None,
            Ok(token) => Some(Credential {
                secret: SecretString::from(token),
                source: CredentialSource::Keyring,
            }),
            Err(keyring::Error::NoEntry) => None,
            Err(e) => {
                warn!(error = %e, "keyring read failed, falling through to next provider");
                None
            }
        }
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait requires &str return"
    )]
    fn name(&self) -> &str {
        "keyring"
    }
}

#[cfg(test)]
#[path = "keyring_provider_tests.rs"]
mod keyring_provider_tests;
