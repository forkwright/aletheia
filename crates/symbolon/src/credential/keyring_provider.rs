//! OS keyring credential provider (behind the `keyring` feature).

use tracing::{debug, warn};

use aletheia_koina::credential::{Credential, CredentialProvider, CredentialSource};
use aletheia_koina::secret::SecretString;

const DEFAULT_SERVICE: &str = "aletheia";
const DEFAULT_USERNAME: &str = "api-token";

/// Reads credentials from the OS keyring (GNOME Keyring, macOS Keychain,
/// Windows Credential Manager).
///
/// Falls through silently when the keyring is unavailable (headless server,
/// no D-Bus session, locked keychain) so downstream providers get a chance.
pub(crate) struct KeyringCredentialProvider {
    service: String,
    username: String,
}

impl KeyringCredentialProvider {
    /// Create a provider using the default service name (`aletheia`) and
    /// username (`api-token`).
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            service: DEFAULT_SERVICE.to_owned(),
            username: DEFAULT_USERNAME.to_owned(),
        }
    }

    /// Create a provider with custom service and username identifiers.
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
    pub(crate) fn store(&self, token: &str) -> Result<(), keyring::Error> {
        self.entry()?.set_password(token)
    }

    /// Remove the stored credential from the OS keyring.
    ///
    /// # Errors
    ///
    /// Returns the keyring error if the backend is unavailable or deletion
    /// fails. `NoEntry` errors are mapped to `Ok(())` since the goal state
    /// (no credential present) is already achieved.
    pub(crate) fn delete(&self) -> Result<(), keyring::Error> {
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
