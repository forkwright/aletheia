//! Placeholder auth extractor — always returns test claims.
//! Real auth middleware (symbolon) will be wired in a future PR.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::ApiError;

/// Authenticated user claims.
#[derive(Debug, Clone)]
pub struct Claims {
    pub sub: String,
}

impl<S: Send + Sync> FromRequestParts<S> for Claims {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let sub = parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anonymous")
            .to_owned();

        Ok(Self { sub })
    }
}
