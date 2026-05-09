//! Shared types for authentication and authorization.

use serde::{Deserialize, Serialize};

use koina::secret::SecretString;

/// Role in the RBAC model.
///
/// Ordered by privilege level: Readonly < Operator < Admin.
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
    /// Expiration (unix seconds).
    pub exp: i64,
    /// Unique token ID (for revocation).
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
            Self::ReadDashboard => f.write_str("read dashboard"),
        }
    }
}

/// Stored user record.
#[derive(Debug, Clone)]
pub struct User {
    /// Stable user identifier.
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
#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    /// Stable key identifier.
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
