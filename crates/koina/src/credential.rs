//! Credential provider trait for dynamic API key resolution.

use std::fmt;

use crate::secret::SecretString;

/// Origin of a resolved credential.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CredentialSource {
    /// Read from an environment variable.
    Environment,
    /// Read from a credential file on disk.
    File,
    /// Obtained via OAuth token refresh.
    OAuth,
    /// Read from the OS keyring (e.g. GNOME Keyring, macOS Keychain).
    Keyring,
}

impl fmt::Display for CredentialSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment => write!(f, "environment"),
            Self::File => write!(f, "file"),
            Self::OAuth => write!(f, "oauth"),
            Self::Keyring => write!(f, "keyring"),
        }
    }
}

/// A resolved credential paired with its source.
pub struct Credential {
    /// The secret value (API key or access token).
    pub secret: SecretString,
    /// Where this credential was obtained from.
    pub source: CredentialSource,
}

/// Trait for credential resolution. Called per-request to support mid-session
/// token rotation and background OAuth refresh.
///
/// Implementations must be `Send + Sync` for use across threads and in async
/// contexts. The `get_credential()` method is intentionally synchronous: the
/// refreshing providers store the current token in memory and refresh
/// asynchronously in a background task.
pub trait CredentialProvider: Send + Sync {
    /// Resolve the current credential value.
    ///
    /// Returns `None` if no credential is available (env var unset, file
    /// missing, etc.). Callers should try the next provider in the chain.
    fn get_credential(&self) -> Option<Credential>;

    /// Human-readable name for diagnostics logging.
    fn name(&self) -> &str;
}

impl fmt::Debug for dyn CredentialProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialProvider")
            .field("name", &self.name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(CredentialSource: Send, Sync, Clone);
    assert_impl_all!(Credential: Send, Sync);

    #[test]
    fn source_display() {
        assert_eq!(CredentialSource::Environment.to_string(), "environment");
        assert_eq!(CredentialSource::File.to_string(), "file");
        assert_eq!(CredentialSource::OAuth.to_string(), "oauth");
        assert_eq!(CredentialSource::Keyring.to_string(), "keyring");
    }

    #[test]
    fn source_equality() {
        assert_eq!(CredentialSource::Environment, CredentialSource::Environment);
        assert_ne!(CredentialSource::File, CredentialSource::OAuth);
        assert_ne!(CredentialSource::Keyring, CredentialSource::File);
    }
}
