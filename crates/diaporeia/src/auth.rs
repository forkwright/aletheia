//! Bearer token authentication middleware for MCP transport.
//!
//! Mirrors pylon's `Claims` extractor but operates as an Axum middleware layer
//! so it can wrap the opaque `StreamableHttpService` (which does not use
//! Axum extractors). When `auth_mode == "none"`, requests pass through without
//! a token.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use axum::middleware::Next;
use tracing::warn;

use koina::http::BEARER_PREFIX;

use crate::state::DiaporeiaState;

/// Axum middleware that validates Bearer JWT tokens on MCP requests.
///
/// # Auth modes
///
/// - `"none"`: permits anonymous requests and lets MCP handlers resolve
///   `none_role` from shared config, defaulting to `Readonly` if malformed.
/// - Any other value: requires a valid `Authorization: Bearer <token>` header;
///   returns 401 Unauthorized on missing/invalid tokens.
#[tracing::instrument(skip_all)]
pub async fn mcp_auth(
    state: Arc<DiaporeiaState>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    if state.auth_mode == "none" {
        return next.run(req).await;
    }

    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix(BEARER_PREFIX));

    let Some(token) = token else {
        warn!("MCP request rejected: missing or malformed Authorization header");
        return unauthorized();
    };

    // INVARIANT: when auth_mode != "none", auth_facade is always Some
    // (enforced where DiaporeiaState is built in `aletheia::commands::server`).
    let Some(ref auth_facade) = state.auth_facade else {
        warn!("MCP request rejected: auth_facade unavailable despite auth_mode != \"none\"");
        return unauthorized();
    };

    match auth_facade.validate_token(token) {
        Ok(_) => next.run(req).await,
        Err(_err) => {
            // SAFETY: logging rejection status, not the token value.
            warn!("MCP request rejected: invalid Bearer token"); // kanon:ignore SECURITY/credential-logging -- logs rejection event, not the token
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
