//! JWT auth extractor — validates Bearer tokens via symbolon.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use aletheia_symbolon::types::Role;

use crate::error::ApiError;
use crate::state::AppState;

/// Authenticated user claims extracted from a JWT Bearer token.
#[derive(Debug, Clone)]
pub struct Claims {
    /// Subject identifier (user or service principal).
    pub sub: String,
    /// Authorization role governing API access.
    pub role: Role,
    /// Optional nous scope — when set, restricts access to a single agent.
    pub nous_id: Option<String>,
}

impl FromRequestParts<Arc<AppState>> for Claims {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized)?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized)?;

        let claims = state
            .jwt_manager
            .validate(token)
            .map_err(|_err| ApiError::Unauthorized)?;

        Ok(Self {
            sub: claims.sub,
            role: claims.role,
            nous_id: claims.nous_id,
        })
    }
}
