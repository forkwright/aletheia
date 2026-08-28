//! Operator credential management endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use koina::http::BEARER_PREFIX;
use koina::secret::SecretString;
use serde::{Deserialize, Serialize};
use symbolon::types::{
    Action, Claims, ManagedCredential, ManagedCredentialRole, ManagedCredentialStatus,
};
use tracing::instrument;
use utoipa::{IntoParams, ToSchema};

use crate::credential_runtime::{
    CredentialMutationEffect, CredentialRuntimeError, CredentialRuntimeManager,
};
use crate::error::{ApiError, UnauthorizedReason};
use crate::event_bus::{DomainEvent, EventBus};
use crate::middleware::RequestId;
use crate::state::AppState;

/// Response body for credential list and mutation endpoints.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only secret-safe credential metadata; no raw credential material is present in this type
#[derive(Debug, Serialize, ToSchema)]
pub struct CredentialsListResponse {
    /// Secret-safe credential metadata.
    pub credentials: Vec<CredentialResponse>,
    /// Runtime effect of the mutation on the live provider chain.
    ///
    /// WHY: list responses produced by mutating endpoints (rotate) must expose
    /// whether the running harness will use the new state without restart.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_effect: Option<CredentialMutationEffect>,
}

/// Outcome of a provider-aware credential validation call.
///
/// WHY(#4875): mirrors `symbolon::types::ProviderValidationState` on the wire
/// with a typed schema (rather than a bare string) so `OpenAPI` documents the
/// exact set of validation states a caller must handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialValidationState {
    /// The provider authenticated the credential.
    Accepted,
    /// The provider explicitly rejected the credential.
    Rejected,
    /// The credential is expired according to locally-known metadata.
    Expired,
    /// The stored credential value is empty or otherwise malformed.
    Malformed,
    /// The provider could not be reached; not evidence the key is bad.
    Unreachable,
    /// No live-check strategy exists for this provider; local inspection only.
    Unknown,
}

impl From<symbolon::types::ProviderValidationState> for CredentialValidationState {
    fn from(state: symbolon::types::ProviderValidationState) -> Self {
        match state {
            symbolon::types::ProviderValidationState::Accepted => Self::Accepted,
            symbolon::types::ProviderValidationState::Rejected => Self::Rejected,
            symbolon::types::ProviderValidationState::Expired => Self::Expired,
            symbolon::types::ProviderValidationState::Malformed => Self::Malformed,
            symbolon::types::ProviderValidationState::Unreachable => Self::Unreachable,
            // WHY: covers `Unknown` plus any future variant this match
            // doesn't yet know about — symbolon's enum is #[non_exhaustive]
            // so it can grow without a breaking change there. Every
            // unrecognized variant must fall back to `Unknown`, never
            // silently mapped to Accepted/Rejected, which would be a
            // provider-acceptance claim this crate cannot back up.
            _ => Self::Unknown,
        }
    }
}

impl CredentialValidationState {
    /// The `status` string this validation state maps to. `Unknown` has no
    /// mapping — callers fall back to the local-inspection status instead of
    /// reporting a provider claim this crate cannot back up.
    fn as_status_str(self) -> Option<&'static str> {
        match self {
            Self::Accepted => Some("valid"),
            Self::Rejected => Some("invalid"),
            Self::Expired => Some("expired"),
            Self::Malformed => Some("malformed"),
            Self::Unreachable => Some("unreachable"),
            Self::Unknown => None,
        }
    }
}

/// Secret-safe credential metadata returned to clients.
#[derive(Debug, Serialize, ToSchema)]
pub struct CredentialResponse {
    /// Stable identifier in `{provider}:{role}` form.
    // kanon:ignore RUST/primitive-for-domain-id — id mirrors ManagedCredential.id, a compound {provider}:{role} string; newtype would require cross-crate coordination
    pub id: String,
    /// Provider name associated with the credential.
    pub provider: String,
    /// Role of this credential for its provider.
    pub role: String,
    /// Redacted preview of the credential, never raw secret material.
    #[serde(rename = "masked_key")]
    pub redacted_preview: String,
    /// Effective status: the persisted provider-validation outcome when one
    /// exists, otherwise local-inspection status (does the file load, has it
    /// not locally expired). `provider_verified` says which kind this is.
    pub status: String,
    /// `true` when `status` reflects an actual provider round trip
    /// (`validation_state` is `Some` and not `unknown`), `false` when it is
    /// local inspection only.
    ///
    /// WHY(#4875): `status: "valid"` previously meant only "the file loaded
    /// and isn't locally expired" — never that the provider accepted the
    /// key. This flag lets the UI distinguish the two without guessing from
    /// the string value.
    pub provider_verified: bool,
    /// The raw persisted provider-validation outcome, when this credential
    /// has ever been validated. `None` means never validated — distinct from
    /// every [`CredentialValidationState`] variant, all of which mean an
    /// attempt was made (including `unknown`, for a provider this crate has
    /// no live-check strategy for).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_state: Option<CredentialValidationState>,
    /// Last validation timestamp when produced by a validation call.
    pub last_validated: Option<String>,
    /// Whether per-credential usage counters are backed by authoritative
    /// provider/session telemetry.
    ///
    /// WHY: hardcoded zero counters were previously returned as factual usage
    /// telemetry. This flag lets the UI hide or mark them unavailable until
    /// real telemetry exists (#4922).
    pub usage_counters_available: bool,
    /// Usage counters when authoritative telemetry is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_counters: Option<CredentialUsageCounters>,
    /// Runtime effect of the mutation that produced this credential response.
    ///
    /// WHY: single-credential mutation responses must expose whether the live
    /// provider chain will use the new state without restart (#4872).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_effect: Option<CredentialMutationEffect>,
}

/// Response body for a credential removal.
#[derive(Debug, Serialize, ToSchema)]
pub struct CredentialRemoveResponse {
    /// Runtime effect of the removal on the live provider chain.
    ///
    /// WHY: 204 responses cannot carry a body, so removal now returns 200 with
    /// an explicit effect so callers know whether a restart is required.
    pub runtime_effect: CredentialMutationEffect,
}

/// Usage counters backed by authoritative telemetry.
///
/// When `CredentialResponse::usage_counters_available` is `false`, this struct
/// is omitted from the response so placeholder zeros cannot be presented as
/// real usage.
#[derive(Debug, Serialize, ToSchema)]
pub struct CredentialUsageCounters {
    /// Requests counted against this credential today.
    pub requests_today: u64,
    /// Tokens consumed through this credential today.
    pub tokens_today: u64,
    /// Telemetry source (e.g. provider API, local session ledger).
    pub source: String,
    /// Freshness indicator for the counters (ISO 8601 timestamp or duration).
    pub freshness: String,
    /// Provider/account scope the counters cover.
    pub scope: String,
    /// Failure or degraded state, if any.
    pub state: String,
}

/// Request body for adding a provider credential.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddCredentialRequest {
    /// Provider name.
    pub provider: String,
    /// Raw key to store encrypted at rest.
    #[schema(value_type = String)]
    pub key: SecretString,
    /// Credential role: `primary` or `backup`.
    pub role: String,
}

/// Query parameters for credential rotation.
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct RotateCredentialQuery {
    /// Provider whose primary and backup credentials should be swapped.
    pub provider: String,
}

/// GET /api/v1/system/credentials: list managed credentials.
#[utoipa::path(
    get,
    path = "/api/v1/system/credentials",
    responses(
        (status = 200, description = "Secret-safe credential metadata", body = CredentialsListResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, headers))]
pub async fn list_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<CredentialsListResponse>, ApiError> {
    require_credential_operator(&state, &headers)?;
    let root = state.oikos.credentials();
    let credentials = state
        .auth_facade
        .list_credentials(&root)
        .map_err(map_symbolon_error)?;
    Ok(Json(CredentialsListResponse {
        credentials: credentials
            .into_iter()
            .map(|c| CredentialResponse::from_managed(c, None))
            .collect(),
        runtime_effect: None,
    }))
}

/// POST /api/v1/system/credentials: add a managed credential.
#[utoipa::path(
    post,
    path = "/api/v1/system/credentials",
    request_body = AddCredentialRequest,
    responses(
        (status = 201, description = "Credential added", body = CredentialResponse),
        (status = 400, description = "Invalid credential request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 409, description = "Credential already exists"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, headers, request))]
pub async fn add_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<AddCredentialRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let claims = require_credential_operator(&state, &headers)?;
    let provider = request.provider.trim().to_owned();
    let request_id = request_id.to_string();

    // WHY(#4878): every exit from this handler -- not just the happy path --
    // must publish an audit event, so a `macro_rules!` local to this
    // function (not a closure: `return` inside a closure would return from
    // the closure, not the handler) captures "audit the failure, then
    // return it" once instead of repeating the same event-construction
    // block at every fallible step.
    macro_rules! audit_fail {
        ($err:expr, $credential_role:expr) => {{
            let err = $err;
            CredentialAuditEvent {
                topic: CREDENTIAL_MUTATION_TOPIC,
                claims: &claims,
                provider: &provider,
                credential_role: $credential_role,
                action: "add",
                result: "error",
                error_code: Some(audit_error_code(&err)),
                request_id: &request_id,
                runtime_effect: None,
                validation_state: None,
            }
            .publish(&state.event_bus)
            .await;
            return Err(err);
        }};
    }

    if let Err(err) = state.credential_runtime.validate_provider(&provider) {
        audit_fail!(map_runtime_error(err), None);
    }
    let role = match request.role.parse::<ManagedCredentialRole>() {
        Ok(role) => role,
        Err(_role_err) => audit_fail!(bad_request("role must be primary or backup"), None),
    };
    let root = state.oikos.credentials();
    let credential = match state
        .auth_facade
        .add_credential(&root, &provider, &request.key, role)
    {
        Ok(credential) => credential,
        Err(err) => audit_fail!(map_symbolon_error(err), Some(role.as_str())),
    };
    let effect = apply_mutation_effect(&state.credential_runtime, &provider).await;
    CredentialAuditEvent {
        topic: CREDENTIAL_MUTATION_TOPIC,
        claims: &claims,
        provider: &provider,
        credential_role: Some(role.as_str()),
        action: "add",
        result: "ok",
        error_code: None,
        request_id: &request_id,
        runtime_effect: Some(effect),
        validation_state: None,
    }
    .publish(&state.event_bus)
    .await;
    Ok((
        StatusCode::CREATED,
        Json(CredentialResponse::from_managed(credential, Some(effect))),
    ))
}

/// POST /api/v1/system/credentials/{id}/validate: validate one credential.
///
/// WHY(#4875): this performs a real, provider-aware validation — a network
/// round trip against the provider's own API for providers this crate knows
/// how to reach live, skipped only when local metadata (empty secret, past
/// expiry) already answers the question. The outcome is persisted, so a
/// non-empty but invalid key can never come back as `status: "valid"`, and a
/// subsequent `GET /credentials` reflects the same result rather than
/// reverting to "never validated". See `CredentialResponse::validation_state`
/// for the full outcome set and `provider_verified` for whether `status`
/// reflects a real provider round trip or local inspection only.
#[utoipa::path(
    post,
    path = "/api/v1/system/credentials/{id}/validate",
    params(("id" = String, Path, description = "Credential id in provider:role form")),
    responses(
        (status = 200, description = "Credential validation result. `validation_state` is one of accepted, rejected, expired, malformed, unreachable, unknown; `status` and `provider_verified` summarize it.", body = CredentialResponse),
        (status = 400, description = "Invalid credential id"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Credential not found"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, headers))]
pub async fn validate_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<String>,
) -> Result<Json<CredentialResponse>, ApiError> {
    let claims = require_credential_operator(&state, &headers)?;
    let provider = provider_from_id(&id).unwrap_or(&id).to_owned();
    let credential_role = role_from_id(&id).map(str::to_owned);
    let request_id = request_id.to_string();

    macro_rules! audit_fail {
        ($err:expr) => {{
            let err = $err;
            CredentialAuditEvent {
                topic: CREDENTIAL_VALIDATION_TOPIC,
                claims: &claims,
                provider: &provider,
                credential_role: credential_role.as_deref(),
                action: "validate",
                result: "error",
                error_code: Some(audit_error_code(&err)),
                request_id: &request_id,
                runtime_effect: None,
                validation_state: None,
            }
            .publish(&state.event_bus)
            .await;
            return Err(err);
        }};
    }

    if provider_from_id(&id).is_some()
        && let Err(err) = state.credential_runtime.validate_provider(&provider)
    {
        audit_fail!(map_runtime_error(err));
    }
    let root = state.oikos.credentials();
    let credential = match state.auth_facade.validate_credential(&root, &id).await {
        Ok(credential) => credential,
        Err(err) => audit_fail!(map_symbolon_error(err)),
    };
    CredentialAuditEvent {
        topic: CREDENTIAL_VALIDATION_TOPIC,
        claims: &claims,
        provider: &provider,
        credential_role: credential_role.as_deref(),
        action: "validate",
        result: "ok",
        error_code: None,
        request_id: &request_id,
        runtime_effect: None,
        validation_state: credential
            .validation
            .map(|record| CredentialValidationState::from(record.state)),
    }
    .publish(&state.event_bus)
    .await;
    Ok(Json(CredentialResponse::from_managed(credential, None)))
}

/// POST /api/v1/system/credentials/rotate: swap primary and backup credentials.
#[utoipa::path(
    post,
    path = "/api/v1/system/credentials/rotate",
    params(RotateCredentialQuery),
    responses(
        (status = 200, description = "Rotated credential metadata", body = CredentialsListResponse),
        (status = 400, description = "Invalid provider"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Primary or backup credential not found"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, headers))]
pub async fn rotate_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<RotateCredentialQuery>,
) -> Result<Json<CredentialsListResponse>, ApiError> {
    let claims = require_credential_operator(&state, &headers)?;
    let provider = query.provider.trim().to_owned();
    let request_id = request_id.to_string();

    macro_rules! audit_fail {
        ($err:expr) => {{
            let err = $err;
            CredentialAuditEvent {
                topic: CREDENTIAL_MUTATION_TOPIC,
                claims: &claims,
                // WHY: rotate swaps both roles for a provider, so no single
                // role is the subject -- `None` is the honest value, not a
                // guess at "primary" or "backup".
                credential_role: None,
                provider: &provider,
                action: "rotate",
                result: "error",
                error_code: Some(audit_error_code(&err)),
                request_id: &request_id,
                runtime_effect: None,
                validation_state: None,
            }
            .publish(&state.event_bus)
            .await;
            return Err(err);
        }};
    }

    if let Err(err) = state.credential_runtime.validate_provider(&provider) {
        audit_fail!(map_runtime_error(err));
    }
    let root = state.oikos.credentials();
    let credentials = match state.auth_facade.rotate_credentials(&root, &provider) {
        Ok(credentials) => credentials,
        Err(err) => audit_fail!(map_symbolon_error(err)),
    };
    let effect = apply_mutation_effect(&state.credential_runtime, &provider).await;
    CredentialAuditEvent {
        topic: CREDENTIAL_MUTATION_TOPIC,
        claims: &claims,
        credential_role: None,
        provider: &provider,
        action: "rotate",
        result: "ok",
        error_code: None,
        request_id: &request_id,
        runtime_effect: Some(effect),
        validation_state: None,
    }
    .publish(&state.event_bus)
    .await;
    Ok(Json(CredentialsListResponse {
        credentials: credentials
            .into_iter()
            .map(|c| CredentialResponse::from_managed(c, None))
            .collect(),
        runtime_effect: Some(effect),
    }))
}

/// DELETE /api/v1/system/credentials/{id}: remove one managed credential.
#[utoipa::path(
    delete,
    path = "/api/v1/system/credentials/{id}",
    params(("id" = String, Path, description = "Credential id in provider:role form")),
    responses(
        (status = 200, description = "Credential removed", body = CredentialRemoveResponse),
        (status = 400, description = "Invalid credential id"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Credential not found"),
        (status = 409, description = "Cannot remove the last primary credential for the provider"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, headers))]
pub async fn remove_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<String>,
) -> Result<Json<CredentialRemoveResponse>, ApiError> {
    let claims = require_credential_operator(&state, &headers)?;
    let credential_role = role_from_id(&id).map(str::to_owned);
    let request_id = request_id.to_string();

    // WHY: an invalid id (no provider prefix) has no provider to name in the
    // audit event either -- fall back to the raw id so the failure is still
    // attributable to what the caller sent, not silently dropped.
    let provider = provider_from_id(&id).unwrap_or(&id).to_owned();

    macro_rules! audit_fail {
        ($err:expr) => {{
            let err = $err;
            CredentialAuditEvent {
                topic: CREDENTIAL_MUTATION_TOPIC,
                claims: &claims,
                provider: &provider,
                credential_role: credential_role.as_deref(),
                action: "remove",
                result: "error",
                error_code: Some(audit_error_code(&err)),
                request_id: &request_id,
                runtime_effect: None,
                validation_state: None,
            }
            .publish(&state.event_bus)
            .await;
            return Err(err);
        }};
    }

    let Some(provider_ref) = provider_from_id(&id) else {
        audit_fail!(bad_request("invalid credential id"));
    };
    if let Err(err) = state.credential_runtime.validate_provider(provider_ref) {
        audit_fail!(map_runtime_error(err));
    }
    let root = state.oikos.credentials();
    if let Err(err) = state.auth_facade.remove_credential(&root, &id) {
        audit_fail!(map_symbolon_error(err));
    }
    let effect = apply_mutation_effect(&state.credential_runtime, &provider).await;
    CredentialAuditEvent {
        topic: CREDENTIAL_MUTATION_TOPIC,
        claims: &claims,
        provider: &provider,
        credential_role: credential_role.as_deref(),
        action: "remove",
        result: "ok",
        error_code: None,
        request_id: &request_id,
        runtime_effect: Some(effect),
        validation_state: None,
    }
    .publish(&state.event_bus)
    .await;
    Ok(Json(CredentialRemoveResponse {
        runtime_effect: effect,
    }))
}

/// Authenticate and authorize the caller for credential management, and
/// return the decoded claims so callers can attribute audit events to an
/// actor (#4878).
fn require_credential_operator(state: &AppState, headers: &HeaderMap) -> Result<Claims, ApiError> {
    let header = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
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
        let reason = crate::extract::token_rejection_reason(&err);
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
    state
        .auth_facade
        .authorize(&claims, &Action::ManageCredentials)
        .map_err(|_err| ApiError::Forbidden {
            message: "insufficient permissions".to_owned(),
            location: snafu::location!(),
        })?;
    Ok(claims)
}

/// Domain event topic for credential add/rotate/remove (state-changing).
const CREDENTIAL_MUTATION_TOPIC: &str = "credential.mutation";
/// Domain event topic for credential validation (read-only provider probe).
const CREDENTIAL_VALIDATION_TOPIC: &str = "credential.validation";

/// A single credential-management audit event.
///
/// WHY(#4878): add/validate/rotate/remove are high-trust operations that
/// previously left no audit trail: no actor, no outcome, nothing an operator
/// or security tooling could subscribe to. Every one of these endpoints
/// publishes exactly one of these, on both success and failure, and this
/// type never carries raw credential material — only metadata.
///
/// # Payload contract
///
/// ```json
/// {
///   "actor": "<claims.sub>",
///   "actor_role": "operator" | "admin",
///   "provider": "<provider name>",
///   "credential_role": "primary" | "backup" | null,
///   "action": "add" | "validate" | "rotate" | "remove",
///   "result": "ok" | "error",
///   "error_code": "<ApiError variant name>" | null,
///   "request_id": "<ulid>",
///   "runtime_effect": "applied" | "restart_required" | "pending_reload" | "not_supported_by_runtime" | null,
///   "validation_state": "accepted" | "rejected" | "expired" | "malformed" | "unreachable" | "unknown" | null
/// }
/// ```
///
/// `runtime_effect` is only ever set for `add`/`rotate`/`remove` (topic
/// `credential.mutation`); `validation_state` only for `validate` (topic
/// `credential.validation`) — the other is always `null` on either topic.
struct CredentialAuditEvent<'a> {
    topic: &'static str,
    claims: &'a Claims,
    provider: &'a str,
    credential_role: Option<&'a str>,
    action: &'static str,
    result: &'static str,
    error_code: Option<&'static str>,
    request_id: &'a str,
    runtime_effect: Option<CredentialMutationEffect>,
    validation_state: Option<CredentialValidationState>,
}

impl CredentialAuditEvent<'_> {
    async fn publish(self, event_bus: &EventBus) {
        // SAFETY: every field here is metadata (actor id, role, provider
        // name, action/result enums, a request id) -- never the credential
        // value itself. No field on this type can carry secret material.
        let payload = serde_json::json!({
            "actor": self.claims.sub,
            "actor_role": self.claims.role,
            "provider": self.provider,
            "credential_role": self.credential_role,
            "action": self.action,
            "result": self.result,
            "error_code": self.error_code,
            "request_id": self.request_id,
            "runtime_effect": self.runtime_effect,
            "validation_state": self.validation_state,
        }); // kanon:ignore SECURITY/credential-logging -- audit payload is built exclusively from actor/provider/action/result metadata, never the credential value
        event_bus
            .publish(DomainEvent::new(event_bus.next_id(), self.topic, payload))
            .await;
    }
}

/// Stable machine-readable code for an [`ApiError`], for audit events.
///
/// WHY: mirrors the intent of `ApiError`'s `code` field in its error
/// response envelope, without needing that field to be `pub(crate)`-visible
/// here — a small, explicit match is clearer than widening that visibility
/// for one caller.
fn audit_error_code(err: &ApiError) -> &'static str {
    match err {
        ApiError::BadRequest { .. } => "bad_request",
        ApiError::Unauthorized { .. } => "unauthorized",
        ApiError::Forbidden { .. } => "forbidden",
        ApiError::NotFound { .. } => "not_found",
        ApiError::Conflict { .. } => "conflict",
        _ => "internal_error",
    }
}

fn map_symbolon_error(err: symbolon::error::Error) -> ApiError {
    match err {
        symbolon::error::Error::InvalidApiKey { .. } => bad_request("invalid credential id"),
        symbolon::error::Error::InvalidCredentialSecret { reason, .. } => bad_request(&reason),
        symbolon::error::Error::NotFound { entity, id, .. } => ApiError::NotFound {
            path: format!("{entity}/{id}"),
            location: snafu::location!(),
        },
        symbolon::error::Error::Duplicate { entity, id, .. } => ApiError::Conflict {
            message: format!("duplicate {entity}: {id}"),
            location: snafu::location!(),
        },
        symbolon::error::Error::RemoveLastPrimary { provider, .. } => ApiError::Conflict {
            message: format!("cannot remove the last primary credential for provider '{provider}'"),
            location: snafu::location!(),
        },
        symbolon::error::Error::PermissionDenied { .. } => ApiError::Forbidden {
            message: "insufficient permissions".to_owned(),
            location: snafu::location!(),
        },
        symbolon::error::Error::Io { .. } | symbolon::error::Error::Storage { .. } => {
            ApiError::Internal {
                message: err.to_string(),
                location: snafu::location!(),
            }
        }
        _ => ApiError::Internal {
            message: err.to_string(),
            location: snafu::location!(),
        },
    }
}

fn map_runtime_error(err: CredentialRuntimeError) -> ApiError {
    match err {
        CredentialRuntimeError::UnsupportedProvider { provider, .. } => ApiError::BadRequest {
            message: format!(
                "provider '{provider}' is not supported by runtime credential management"
            ),
            location: snafu::location!(),
        },
    }
}

fn bad_request(message: &str) -> ApiError {
    ApiError::BadRequest {
        message: message.to_owned(),
        location: snafu::location!(),
    }
}

impl CredentialResponse {
    fn from_managed(
        credential: ManagedCredential,
        runtime_effect: Option<CredentialMutationEffect>,
    ) -> Self {
        let local_status = status_str(credential.status);
        // WHY(#4875): `status` must never claim provider acceptance it can't
        // back up. A persisted validation record wins when it carries an
        // actual outcome; `Unknown` (no live-check strategy for this
        // provider) falls back to local-inspection status, same as no
        // validation record at all.
        let validation_state = credential
            .validation
            .map(|record| CredentialValidationState::from(record.state));
        let status = validation_state
            .and_then(CredentialValidationState::as_status_str)
            .unwrap_or(local_status)
            .to_owned();
        let provider_verified =
            validation_state.is_some_and(|state| state.as_status_str().is_some());
        Self {
            id: credential.id,
            provider: credential.provider,
            role: credential.role.as_str().to_owned(),
            redacted_preview: credential.redacted_preview,
            status,
            provider_verified,
            validation_state,
            last_validated: credential.last_validated,
            // WHY: no authoritative provider/session telemetry exists yet, so
            // omit the counters entirely rather than return hardcoded zeros.
            usage_counters_available: false,
            usage_counters: None,
            runtime_effect,
        }
    }
}

fn status_str(status: ManagedCredentialStatus) -> &'static str {
    match status {
        ManagedCredentialStatus::Valid => "valid",
        ManagedCredentialStatus::Expired => "expired",
        _ => "untested",
    }
}

fn provider_from_id(id: &str) -> Option<&str> {
    id.split_once(':').map(|(provider, _)| provider)
}

fn role_from_id(id: &str) -> Option<&str> {
    id.split_once(':').map(|(_, role)| role)
}

async fn apply_mutation_effect(
    credential_runtime: &CredentialRuntimeManager,
    provider: &str,
) -> CredentialMutationEffect {
    let effect = credential_runtime.mutation_effect(provider);
    credential_runtime.record_effect(provider, effect).await;
    effect
}
