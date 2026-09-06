//! JWT auth extractor: validates Bearer tokens via symbolon.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use symbolon::types::Role;

use koina::http::BEARER_PREFIX;

use crate::error::{ApiError, UnauthorizedReason};
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
    /// `true` only for the synthetic identity `auth_mode = "none"`
    /// fabricates -- no Bearer token was presented or validated.
    ///
    /// WHY(#7234): `require_read_role` checks this to restore the exact
    /// pre-#7227 behavior of the read floors that PR added (session,
    /// knowledge, nous, planning content): before #7227, none of those
    /// routes checked role at all, so a `none_role` instance -- which
    /// schema-defaults to the deliberately least-privileged `"readonly"`
    /// (SECURITY #5169, #5342) -- could always read its own data. #7227's
    /// floor is the first check in the codebase to gate routes that
    /// previously had none, so disabling auth entirely must not
    /// retroactively lock such an instance out of them. Routes gated
    /// before that PR (credentials, config, metrics aggregates, knowledge
    /// writes, workspace) are unaffected: they have always evaluated
    /// `none_role`'s real value, matching every other authenticated route,
    /// and `require_role` (not `require_read_role`) still does that for
    /// them here.
    pub unauthenticated: bool,
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
                unauthenticated: true,
            });
        }

        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized {
                reason: UnauthorizedReason::MissingCredentials,
                location: snafu::location!(),
            })?;

        let token = header
            .strip_prefix(BEARER_PREFIX)
            .ok_or(ApiError::Unauthorized {
                reason: UnauthorizedReason::MalformedAuthorizationHeader,
                location: snafu::location!(),
            })?;

        let claims = state.auth_facade.validate_token(token).map_err(|err| {
            let reason = token_rejection_reason(&err);
            // WHY(#6826): the wire response stays coarse, but the
            // validator's cause (expired vs malformed vs revoked) is
            // operator-visible in logs instead of discarded.
            tracing::info!(
                reason = reason.as_str(),
                error = %err,
                "bearer token rejected"
            );
            ApiError::Unauthorized {
                reason,
                location: snafu::location!(),
            }
        })?;

        Ok(Self {
            sub: claims.sub,
            role: claims.role,
            nous_id: claims.nous_id,
            unauthenticated: false,
        })
    }
}

/// Map a rejected Bearer token's validator error to its wire reason.
///
/// WHY(#6826): expired tokens get their own reason so a client can render
/// "re-authenticate" rather than "log in"; every other validation failure
/// (malformed, bad signature, wrong kind, revoked) stays the coarser
/// `invalid_token`.
pub(crate) fn token_rejection_reason(err: &symbolon::error::Error) -> UnauthorizedReason {
    match err {
        symbolon::error::Error::ExpiredToken { .. } => UnauthorizedReason::TokenExpired,
        _ => UnauthorizedReason::InvalidToken,
    }
}

/// Reject the request if the caller's role is below `minimum`.
pub(crate) fn require_role(claims: &Claims, minimum: Role) -> Result<(), ApiError> {
    if claims.role < minimum {
        return Err(ApiError::forbidden("insufficient permissions"));
    }
    Ok(())
}

/// Reject the request if the caller's role is below `minimum` -- unless the
/// caller is the synthetic `auth_mode = "none"` identity, which always
/// passes regardless of its `none_role`.
///
/// WHY(#7234): use this instead of `require_role` only for a read floor that
/// #7227 (or a PR built on it) newly added to a route that previously had no
/// role check at all. A route that has required a role since before #7227
/// (credentials, config, metrics aggregates, knowledge writes, workspace)
/// must keep calling `require_role` directly -- for those, `none_role`'s
/// real value has always been the answer, and bypassing it here would
/// reintroduce the full-privilege-under-`auth.mode=none` exposure
/// SECURITY #5169/#5342 deliberately closed by defaulting `none_role` to
/// `"readonly"`. A real Bearer token (token/jwt mode) with an insufficient
/// role is always rejected here exactly like `require_role`; only the
/// no-token-presented identity is exempt.
pub(crate) fn require_read_role(claims: &Claims, minimum: Role) -> Result<(), ApiError> {
    if claims.unauthenticated {
        return Ok(());
    }
    require_role(claims, minimum)
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
