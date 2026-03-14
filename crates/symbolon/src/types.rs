//! Shared types for authentication and authorization.

use serde::{Deserialize, Serialize};

/// Role in the RBAC model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Role {
    /// Full access. Can manage agents, users, read all sessions, configure system.
    Operator,
    /// Per-nous scoped. Can access own sessions, use own tools, read shared workspace.
    Agent,
    /// Dashboard access only. No mutations.
    Readonly,
}

impl Role {
    /// String representation for storage.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Agent => "agent",
            Self::Readonly => "readonly",
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
            "operator" => Ok(Self::Operator),
            "agent" => Ok(Self::Agent),
            "readonly" => Ok(Self::Readonly),
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
    /// Subject — user or agent ID.
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "auth facade internal; exercised by crate tests")
)]
pub(crate) struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

/// Actions that can be authorized via RBAC.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "auth facade internal; exercised by crate tests")
)]
#[non_exhaustive]
pub(crate) enum Action {
    /// Read a session belonging to a specific nous.
    ReadSession { nous_id: String },
    /// Write to a session belonging to a specific nous.
    WriteSession { nous_id: String },
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
#[expect(
    dead_code,
    reason = "auth facade internal; some fields unused even in crate tests"
)]
pub(crate) struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: Role,
    pub created_at: String,
    pub updated_at: String,
}

/// Stored API key metadata (never includes the secret).
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "auth facade internal; some fields unused even in crate tests"
)]
pub(crate) struct ApiKeyRecord {
    pub id: String,
    pub prefix: String,
    pub key_hash: String,
    pub role: Role,
    pub nous_id: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}
