//! Shared types for authentication and authorization.

use serde::{Deserialize, Serialize};

use koina::secret::SecretString;

/// Role in the RBAC model.
///
/// Ordered by privilege level: Readonly < Operator < Admin.
// kanon:ignore RUST/no-debug-derive-on-public-types — Role is a privilege-level enum with no secret data; Debug is required for tracing and log output
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Role {
    /// Dashboard access only. No mutations.
    Readonly,
    /// Per-nous scoped. Can access own sessions, use own tools, read shared workspace.
    Agent,
    /// Full access. Can manage agents, users, read all sessions, configure system.
    Operator,
    /// Superuser. All Operator permissions plus system administration.
    Admin,
}

impl Role {
    /// String representation for storage.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Readonly => "readonly",
            Self::Agent => "agent",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "readonly" => Ok(Self::Readonly),
            "agent" => Ok(Self::Agent),
            "operator" => Ok(Self::Operator),
            "admin" => Ok(Self::Admin),
            other => Err(format!("unknown role: {other}")),
        }
    }
}

/// Distinguishes access tokens from refresh tokens.
// kanon:ignore RUST/no-debug-derive-on-public-types — TokenKind discriminates access vs refresh with no sensitive payload; Debug is required for observability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TokenKind {
    /// Short-lived token for API access.
    Access,
    /// Long-lived token used to obtain new access tokens.
    Refresh,
}

/// JWT claims payload.
// kanon:ignore RUST/no-debug-derive-on-public-types — Claims contains only non-secret metadata (sub, role, timestamps); token secrets live in SecretString wrappers outside this struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject: user or agent ID.
    pub sub: String,
    /// RBAC role.
    pub role: Role,
    /// For agent tokens, the nous ID this token is scoped to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nous_id: Option<String>,
    /// Issuer.
    pub iss: String,
    /// Issued-at (unix seconds).
    pub iat: i64,
    /// Not-before (unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    /// Expiration (unix seconds).
    pub exp: i64,
    /// Unique token ID (for revocation).
    // kanon:ignore RUST/primitive-for-domain-id — jti is a JWT standard claim (RFC 7519) generated as a ULID string; newtype would break serde interop with JWT consumers
    pub jti: String,
    /// Access or refresh.
    pub kind: TokenKind,
}

/// An access + refresh token pair returned from login or refresh.
pub struct TokenPair {
    /// Access token used for authenticated API requests.
    pub access_token: SecretString,
    /// Refresh token used to obtain a new token pair.
    pub refresh_token: SecretString,
}

/// Actions that can be authorized via RBAC.
#[non_exhaustive]
pub enum Action {
    /// Read a session belonging to a specific nous.
    ReadSession {
        /// Nous identifier whose session is being read.
        nous_id: String,
    },
    /// Write to a session belonging to a specific nous.
    WriteSession {
        /// Nous identifier whose session is being written.
        nous_id: String,
    },
    /// Manage agent configurations.
    ManageAgents,
    /// Manage user accounts.
    ManageUsers,
    /// Manage provider credential files.
    ManageCredentials,
    /// Read the dashboard (metrics, status).
    ReadDashboard,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadSession { nous_id } => write!(f, "read session (nous: {nous_id})"),
            Self::WriteSession { nous_id } => write!(f, "write session (nous: {nous_id})"),
            Self::ManageAgents => f.write_str("manage agents"),
            Self::ManageUsers => f.write_str("manage users"),
            Self::ManageCredentials => f.write_str("manage credentials"),
            Self::ReadDashboard => f.write_str("read dashboard"),
        }
    }
}

/// Credential role within a provider's local file set.
// kanon:ignore RUST/no-debug-derive-on-public-types — role enum contains no secret data; Debug is safe
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManagedCredentialRole {
    /// Active credential used by provider resolution.
    Primary,
    /// Standby credential used for operator-controlled rotation.
    Backup,
}

impl ManagedCredentialRole {
    /// Stable wire string for the role.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Backup => "backup",
        }
    }
}

impl std::fmt::Display for ManagedCredentialRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ManagedCredentialRole {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "primary" => Ok(Self::Primary),
            "backup" => Ok(Self::Backup),
            other => Err(format!("unknown credential role: {other}")),
        }
    }
}

/// Credential usability status reported to operators.
// kanon:ignore RUST/no-debug-derive-on-public-types — status enum contains no secret data; Debug is safe
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManagedCredentialStatus {
    /// Credential loaded and has not expired locally.
    Valid,
    /// Credential is expired or locally unusable.
    Expired,
    /// Credential has not been tested.
    Untested,
}

impl ManagedCredentialStatus {
    /// Stable wire string for the status.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Expired => "expired",
            Self::Untested => "untested",
        }
    }
}

/// Secret-safe credential metadata for operator APIs.
// kanon:ignore RUST/no-debug-derive-on-public-types — redacted_preview is masked; raw secret material is never stored in this type
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedCredential {
    /// Stable identifier in `{provider}:{role}` form.
    pub id: String,
    /// Provider name associated with the credential.
    pub provider: String,
    /// Role of this credential for its provider.
    pub role: ManagedCredentialRole,
    /// Redacted key preview, never raw credential material.
    pub redacted_preview: String,
    /// Local validation status.
    pub status: ManagedCredentialStatus,
    /// Last validation timestamp, when this response was produced by validation.
    pub last_validated: Option<String>,
}

/// Stored user record.
// kanon:ignore RUST/no-debug-derive-on-public-types — User stores only metadata and a password hash (not a plaintext secret); Debug is safe for logs
#[derive(Debug, Clone)]
pub struct User {
    /// Stable user identifier.
    // kanon:ignore RUST/primitive-for-domain-id — user ID is a stable ULID string generated by the auth store; newtype would require cross-crate coordination
    pub id: String,
    /// Login username.
    pub username: String,
    /// Password hash encoded by the configured password hasher.
    pub password_hash: String,
    /// User authorization role.
    pub role: Role,
    /// Creation timestamp in RFC3339-like UTC format.
    pub created_at: String,
    /// Last update timestamp in RFC3339-like UTC format.
    pub updated_at: String,
}

/// Stored API key metadata (never includes the secret).
// kanon:ignore RUST/no-debug-derive-on-public-types — ApiKeyRecord stores only metadata and a hash (not the secret key); Debug is safe for logs and admin UIs
#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    /// Stable key identifier.
    // kanon:ignore RUST/primitive-for-domain-id — API key ID is a stable ULID string; newtype would add boilerplate without safety gain
    pub id: String,
    /// Public key prefix.
    pub prefix: String,
    /// Secret hash for validation.
    pub key_hash: String,
    /// Role granted by the key.
    pub role: Role,
    /// Optional nous scope granted by the key.
    pub nous_id: Option<String>,
    /// Creation timestamp in RFC3339-like UTC format.
    pub created_at: String,
    /// Optional expiration timestamp in RFC3339-like UTC format.
    pub expires_at: Option<String>,
    /// Optional last-used timestamp in RFC3339-like UTC format.
    pub last_used_at: Option<String>,
    /// Optional revocation timestamp in RFC3339-like UTC format.
    pub revoked_at: Option<String>,
}
