//! Bearer token authentication middleware for MCP transport.
//!
//! Mirrors pylon's `Claims` extractor but operates as an Axum middleware layer
//! so it can wrap the opaque `StreamableHttpService` (which does not use
//! Axum extractors). When `auth_mode == "none"`, requests pass through without
//! a token and receive anonymous claims via request extensions.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use axum::middleware::Next;
use tracing::warn;

use aletheia_koina::http::BEARER_PREFIX;
use aletheia_symbolon::types::Role;

use crate::state::DiaporeiaState;

/// Validated identity attached to request extensions after successful auth.
///
/// Parallel to `pylon::extract::Claims` but decoupled: diaporeia does not
/// depend on pylon, and the two may diverge as per-tool RBAC is added.
#[derive(Debug, Clone)]
pub struct McpClaims {
    /// Subject identifier (user or service principal).
    pub sub: String,
    /// Authorization role governing API access.
    pub role: Role,
    /// Optional nous scope: when set, restricts access to a single agent.
    pub nous_id: Option<String>,
}

/// Axum middleware that validates Bearer JWT tokens on MCP requests.
///
/// # Auth modes
///
/// - `"none"`: injects anonymous `McpClaims` with the configured `none_role`.
/// - Any other value: requires a valid `Authorization: Bearer <token>` header;
///   returns 401 Unauthorized on missing/invalid tokens.
///
/// Validated claims are inserted into request extensions so downstream handlers
/// can access them via `req.extensions().get::<McpClaims>()`.
pub async fn mcp_auth(
    state: Arc<DiaporeiaState>,
    mut req: Request<Body>,
    next: Next,
) -> Response<Body> {
    if state.auth_mode == "none" {
        let role = state.none_role.parse::<Role>().unwrap_or(Role::Readonly);
        req.extensions_mut().insert(McpClaims {
            sub: "anonymous".to_owned(),
            role,
            nous_id: None,
        });
        return next.run(req).await;
    }

    // Extract Bearer token from Authorization header.
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix(BEARER_PREFIX));

    let Some(token) = token else {
        warn!("MCP request rejected: missing or malformed Authorization header");
        return unauthorized();
    };

    // Validate via JwtManager. When auth_mode != "none" the jwt_manager is
    // always Some (enforced at construction in server/mod.rs).
    let Some(ref jwt) = state.jwt_manager else {
        // INVARIANT: should never happen if construction is correct.
        warn!("MCP request rejected: jwt_manager unavailable despite auth_mode != \"none\"");
        return unauthorized();
    };

    match jwt.validate(token) {
        Ok(claims) => {
            req.extensions_mut().insert(McpClaims {
                sub: claims.sub,
                role: claims.role,
                nous_id: claims.nous_id,
            });
            next.run(req).await
        }
        Err(_err) => {
            warn!("MCP request rejected: invalid Bearer token");
            unauthorized()
        }
    }
}

/// Build a 401 Unauthorized response with an empty body.
#[expect(
    clippy::expect_used,
    reason = "static 401 response with empty body: infallible in practice"
)]
fn unauthorized() -> Response<Body> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .body(Body::empty())
        .expect("static 401 response must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_claims_is_send_sync() {
        const _: fn() = || {
            fn assert<T: Send + Sync + Clone>() {}
            assert::<McpClaims>();
        };
    }
}
