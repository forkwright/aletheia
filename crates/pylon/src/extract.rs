//! JWT auth extractor: validates Bearer tokens via symbolon.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use aletheia_symbolon::types::Role;

use aletheia_koina::http::BEARER_PREFIX;

use crate::error::ApiError;
use crate::state::AppState;

/// Authenticated user claims extracted from a JWT Bearer token.
///
/// When `auth_mode` is `"none"` in the gateway config, a synthetic admin
/// identity is injected without requiring a Bearer token.
#[derive(Debug, Clone)]
pub struct Claims {
    /// Subject identifier (user or service principal).
    pub sub: String,
    /// Authorization role governing API access.
    pub role: Role,
    /// Optional nous scope: when set, restricts access to a single agent.
    pub nous_id: Option<String>,
}

impl FromRequestParts<Arc<AppState>> for Claims {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // WHY: Granting Operator role bypasses all access controls and lets
        // any network caller manage agents, modify config, and access all
        // sessions. Auth-disabled deployments should receive the least
        // privilege needed for basic use: Readonly restricts mutations.
        if state.auth_mode == "none" {
            return Ok(Self {
                sub: "anonymous".to_owned(),
                role: Role::Readonly,
                nous_id: None,
            });
        }

        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized {
                location: snafu::Location::default(),
            })?;

        let token = header
            .strip_prefix(BEARER_PREFIX)
            .ok_or(ApiError::Unauthorized {
                location: snafu::Location::default(),
            })?;

        let claims = state
            .jwt_manager
            .validate(token)
            .map_err(|_err| ApiError::Unauthorized {
                location: snafu::Location::default(),
            })?;

        Ok(Self {
            sub: claims.sub,
            role: claims.role,
            nous_id: claims.nous_id,
        })
    }
}
