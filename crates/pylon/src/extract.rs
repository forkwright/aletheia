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
        if state.auth_mode == "none" {
            let role = state.none_role.parse::<Role>().unwrap_or(Role::Readonly);
            return Ok(Self {
                sub: "anonymous".to_owned(),
                role,
                nous_id: None,
            });
        }

        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized {
                location: snafu::location!(),
            })?;

        let token = header
            .strip_prefix(BEARER_PREFIX)
            .ok_or(ApiError::Unauthorized {
                location: snafu::location!(),
            })?;

        let claims = state
            .jwt_manager
            .validate(token)
            .map_err(|_err| ApiError::Unauthorized {
                location: snafu::location!(),
            })?;

        Ok(Self {
            sub: claims.sub,
            role: claims.role,
            nous_id: claims.nous_id,
        })
    }
}

/// Reject the request if the caller's role is below `minimum`.
pub(crate) fn require_role(claims: &Claims, minimum: Role) -> Result<(), ApiError> {
    if claims.role < minimum {
        return Err(ApiError::forbidden("insufficient permissions"));
    }
    Ok(())
}

/// Reject the request if the caller has a scoped `nous_id` that does not match `target_nous_id`.
pub(crate) fn require_nous_access(claims: &Claims, target_nous_id: &str) -> Result<(), ApiError> {
    if let Some(ref scoped) = claims.nous_id {
        if scoped != target_nous_id {
            return Err(ApiError::forbidden("access denied for this agent"));
        }
    }
    Ok(())
}
