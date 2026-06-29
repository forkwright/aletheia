//! JWT auth extractor: validates Bearer tokens via symbolon.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use symbolon::types::Role;

use koina::http::BEARER_PREFIX;

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
        if let Some(claims) = parts.extensions.get::<Claims>() {
            return Ok(claims.clone());
        }

        if state.auth_mode == "none" {
            // WHY(#5765): A misconfigured none_role must not silently degrade to
            // Readonly. Log the fallback so operators can detect typos even if
            // startup validation is bypassed.
            let role = state.none_role.parse::<Role>().unwrap_or_else(|_| {
                tracing::error!(
                    none_role = %state.none_role,
                    "auth.mode=none: none_role is not a valid role; falling back to readonly"
                );
                Role::Readonly
            });
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

        let claims =
            state
                .auth_facade
                .validate_token(token)
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
    if let Some(ref scoped) = claims.nous_id
        && scoped != target_nous_id
    {
        return Err(ApiError::forbidden("access denied for this agent"));
    }
    Ok(())
}
