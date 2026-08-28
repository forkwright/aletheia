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
use rmcp::service::RequestContext;
use tracing::warn;

use koina::http::BEARER_PREFIX;
use symbolon::types::{Claims, Role};

use crate::state::DiaporeiaState;

/// Verified caller identity resolved for MCP RBAC.
#[derive(Debug, Clone)]
pub(crate) struct McpCaller {
    /// Subject identifier from a signed token, or `"anonymous"` in no-auth mode.
    pub(crate) sub: String,
    /// Authorization role governing MCP access.
    pub(crate) role: Role,
    /// Optional nous scope: when set, restricts access to a single agent.
    pub(crate) nous_id: Option<String>,
}

/// Resolve the caller identity from an MCP request context.
///
/// `auth_mode == "none"` returns the configured anonymous role. Every other
/// mode requires a Bearer token and a configured validator. Invalid tokens or
/// missing validator state resolve to `None` so callers fail closed.
#[must_use]
pub(crate) fn resolve_caller(
    state: &DiaporeiaState,
    context: &RequestContext<rmcp::RoleServer>,
) -> Option<McpCaller> {
    if state.auth_mode == "none" {
        return Some(anonymous_caller(state));
    }

    let token = bearer_token_from_context(context)?;
    validate_bearer_token(state, token)
}

fn anonymous_caller(state: &DiaporeiaState) -> McpCaller {
    // WHY(#5765): startup validation (`taxis::validate::validate_gateway`)
    // rejects an invalid `none_role` before the server can boot, but this
    // parse stays defense-in-depth — log loudly if that guard is ever
    // bypassed, so a fallback to Readonly is never silent.
    let role = state.none_role.parse::<Role>().unwrap_or_else(|_| {
        tracing::error!(
            none_role = %state.none_role,
            "auth.mode=none: none_role is not a valid role; falling back to readonly"
        );
        Role::Readonly
    });
    McpCaller {
        sub: "anonymous".to_owned(),
        role,
        nous_id: None,
    }
}

fn bearer_token_from_context(context: &RequestContext<rmcp::RoleServer>) -> Option<&str> {
    let parts = context.extensions.get::<http::request::Parts>()?;
    let header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())?;
    header.strip_prefix(BEARER_PREFIX)
}

fn validate_bearer_token(state: &DiaporeiaState, token: &str) -> Option<McpCaller> {
    let Some(ref auth_facade) = state.auth_facade else {
        tracing::error!(
            "INVARIANT violation: auth_facade is None but auth_mode != \"none\"; denying MCP access"
        );
        return None;
    };

    // WHY(#5566): auth-required MCP paths must only consume claims after
    // signed validation. Do not add an unsigned decode fallback here.
    auth_facade
        .validate_token(token)
        .ok()
        .map(caller_from_claims)
}

fn caller_from_claims(claims: Claims) -> McpCaller {
    McpCaller {
        sub: claims.sub,
        role: claims.role,
        nous_id: claims.nous_id,
    }
}

/// Environment variable carrying the bearer token that identifies a stdio
/// MCP session's fixed principal (#5184).
///
/// Stdio has no per-request HTTP context to carry a bearer token the way the
/// streamable HTTP transport does — [`bearer_token_from_context`] reads a
/// header off `http::request::Parts`, which a raw JSON-RPC-over-stdio
/// connection never populates. Under an authenticated `auth_mode`, that made
/// every stdio tool call resolve to no caller and be denied — unconditionally
/// and silently. This variable lets an operator bind ONE verified principal
/// for the whole stdio session instead, read once at startup.
pub const STDIO_TOKEN_ENV: &str = "ALETHEIA_MCP_STDIO_TOKEN";

/// Resolve the fixed principal for a stdio MCP session under an
/// authenticated `auth_mode`.
///
/// Returns `None` when the token is missing, empty, or fails validation —
/// callers must fail closed on `None` (refuse to start the stdio transport)
/// rather than falling back to anonymous access. This function is not
/// meaningful under `auth_mode == "none"`: that mode resolves the anonymous
/// caller unconditionally and per-request via [`resolve_caller`], with no
/// need for a bound principal.
#[must_use]
pub(crate) fn resolve_stdio_principal(state: &DiaporeiaState) -> Option<McpCaller> {
    let token = std::env::var(STDIO_TOKEN_ENV).ok()?;
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    validate_bearer_token(state, token)
}

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
