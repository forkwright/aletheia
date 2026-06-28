//! Operator credential management endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use koina::http::BEARER_PREFIX;
use koina::secret::SecretString;
use serde::{Deserialize, Serialize};
use symbolon::types::{Action, ManagedCredential, ManagedCredentialRole, ManagedCredentialStatus};
use tracing::instrument;
use utoipa::{IntoParams, ToSchema};

use crate::credential_runtime::{
    CredentialMutationEffect, CredentialOperationRecord, CredentialRuntimeError,
    CredentialRuntimeManager,
};
use crate::error::ApiError;
use crate::extract::Claims;
use crate::middleware::RequestId;
use crate::state::AppState;

const CREDENTIAL_AUDIT_TOPIC: &str = "credential.audit";
const ACTION_ADD: &str = "add";
const ACTION_VALIDATE: &str = "validate";
const ACTION_ROTATE: &str = "rotate";
const ACTION_REMOVE: &str = "remove";
const RESULT_SUCCESS: &str = "success";
const RESULT_FAILURE: &str = "failure";
const RUNTIME_EFFECT_NO_CHANGE: &str = "no_runtime_change";
const RUNTIME_EFFECT_NOT_APPLIED: &str = "not_applied";

#[derive(Clone, Copy)]
struct CredentialOperation<'a> {
    action: &'static str,
    provider: Option<&'a str>,
    role: Option<&'a str>,
    result: &'static str,
    runtime_effect: &'a str,
    error_code: Option<&'a str>,
    credential_status: Option<&'a str>,
}

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
    /// Local validation status.
    pub status: String,
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
    let role_label = request.role.trim().to_owned();

    let result: Result<(ManagedCredential, CredentialMutationEffect), ApiError> = match state
        .credential_runtime
        .validate_provider(&provider)
        .map_err(map_runtime_error)
    {
        Ok(()) => match role_label.parse::<ManagedCredentialRole>() {
            Ok(role) => {
                let root = state.oikos.credentials();
                match state
                    .auth_facade
                    .add_credential(&root, &provider, &request.key, role)
                    .map_err(map_symbolon_error)
                {
                    Ok(credential) => {
                        let effect =
                            apply_mutation_effect(&state.credential_runtime, &provider).await;
                        Ok((credential, effect))
                    }
                    Err(err) => Err(err),
                }
            }
            Err(_role_err) => Err(bad_request("role must be primary or backup")),
        },
        Err(err) => Err(err),
    };

    match result {
        Ok((credential, effect)) => {
            let audit_provider = credential.provider.clone();
            let audit_role = credential.role.as_str().to_owned();
            let effect_label = effect.as_str();
            let operation = CredentialOperation {
                action: ACTION_ADD,
                provider: Some(&audit_provider),
                role: Some(&audit_role),
                result: RESULT_SUCCESS,
                runtime_effect: effect_label,
                error_code: None,
                credential_status: None,
            };
            state
                .credential_runtime
                .record_mutation_result(operation.to_record(&request_id))
                .await;
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Ok((
                StatusCode::CREATED,
                Json(CredentialResponse::from_managed(credential, Some(effect))),
            ))
        }
        Err(err) => {
            let code = api_error_code(&err);
            let operation = CredentialOperation {
                action: ACTION_ADD,
                provider: Some(&provider),
                role: Some(&role_label),
                result: RESULT_FAILURE,
                runtime_effect: RUNTIME_EFFECT_NOT_APPLIED,
                error_code: Some(&code),
                credential_status: None,
            };
            state
                .credential_runtime
                .record_mutation_result(operation.to_record(&request_id))
                .await;
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Err(err)
        }
    }
}

/// POST /api/v1/system/credentials/{id}/validate: validate one credential.
#[utoipa::path(
    post,
    path = "/api/v1/system/credentials/{id}/validate",
    params(("id" = String, Path, description = "Credential id in provider:role form")),
    responses(
        (status = 200, description = "Credential validation result", body = CredentialResponse),
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
    let (provider, role) = credential_parts_from_id(&id);

    let result: Result<ManagedCredential, ApiError> = if let Some(provider) = provider.as_deref() {
        state
            .credential_runtime
            .validate_provider(provider)
            .map_err(map_runtime_error)
            .and_then(|()| {
                let root = state.oikos.credentials();
                state
                    .auth_facade
                    .validate_credential(&root, &id)
                    .map_err(map_symbolon_error)
            })
    } else {
        let root = state.oikos.credentials();
        state
            .auth_facade
            .validate_credential(&root, &id)
            .map_err(map_symbolon_error)
    };

    match result {
        Ok(credential) => {
            let audit_provider = credential.provider.clone();
            let audit_role = credential.role.as_str().to_owned();
            let credential_status = status_str(credential.status).to_owned();
            let operation = CredentialOperation {
                action: ACTION_VALIDATE,
                provider: Some(&audit_provider),
                role: Some(&audit_role),
                result: RESULT_SUCCESS,
                runtime_effect: RUNTIME_EFFECT_NO_CHANGE,
                error_code: None,
                credential_status: Some(&credential_status),
            };
            state
                .credential_runtime
                .record_successful_validation(operation.to_record(&request_id))
                .await;
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Ok(Json(CredentialResponse::from_managed(credential, None)))
        }
        Err(err) => {
            let code = api_error_code(&err);
            let operation = CredentialOperation {
                action: ACTION_VALIDATE,
                provider: provider.as_deref(),
                role: role.as_deref(),
                result: RESULT_FAILURE,
                runtime_effect: RUNTIME_EFFECT_NO_CHANGE,
                error_code: Some(&code),
                credential_status: None,
            };
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Err(err)
        }
    }
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
    let result: Result<(Vec<ManagedCredential>, CredentialMutationEffect), ApiError> = match state
        .credential_runtime
        .validate_provider(&provider)
        .map_err(map_runtime_error)
    {
        Ok(()) => {
            let root = state.oikos.credentials();
            match state
                .auth_facade
                .rotate_credentials(&root, &provider)
                .map_err(map_symbolon_error)
            {
                Ok(credentials) => {
                    let effect = apply_mutation_effect(&state.credential_runtime, &provider).await;
                    Ok((credentials, effect))
                }
                Err(err) => Err(err),
            }
        }
        Err(err) => Err(err),
    };

    match result {
        Ok((credentials, effect)) => {
            let effect_label = effect.as_str();
            let operation = CredentialOperation {
                action: ACTION_ROTATE,
                provider: Some(&provider),
                role: Some("primary_backup"),
                result: RESULT_SUCCESS,
                runtime_effect: effect_label,
                error_code: None,
                credential_status: None,
            };
            state
                .credential_runtime
                .record_mutation_result(operation.to_record(&request_id))
                .await;
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Ok(Json(CredentialsListResponse {
                credentials: credentials
                    .into_iter()
                    .map(|c| CredentialResponse::from_managed(c, None))
                    .collect(),
                runtime_effect: Some(effect),
            }))
        }
        Err(err) => {
            let code = api_error_code(&err);
            let operation = CredentialOperation {
                action: ACTION_ROTATE,
                provider: Some(&provider),
                role: Some("primary_backup"),
                result: RESULT_FAILURE,
                runtime_effect: RUNTIME_EFFECT_NOT_APPLIED,
                error_code: Some(&code),
                credential_status: None,
            };
            state
                .credential_runtime
                .record_mutation_result(operation.to_record(&request_id))
                .await;
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Err(err)
        }
    }
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
    let (provider, role) = credential_parts_from_id(&id);

    let result: Result<CredentialMutationEffect, ApiError> = match provider.as_deref() {
        Some(provider) => match state
            .credential_runtime
            .validate_provider(provider)
            .map_err(map_runtime_error)
        {
            Ok(()) => {
                let root = state.oikos.credentials();
                match state
                    .auth_facade
                    .remove_credential(&root, &id)
                    .map_err(map_symbolon_error)
                {
                    Ok(()) => Ok(apply_mutation_effect(&state.credential_runtime, provider).await),
                    Err(err) => Err(err),
                }
            }
            Err(err) => Err(err),
        },
        None => Err(bad_request("invalid credential id")),
    };

    match result {
        Ok(effect) => {
            let effect_label = effect.as_str();
            let operation = CredentialOperation {
                action: ACTION_REMOVE,
                provider: provider.as_deref(),
                role: role.as_deref(),
                result: RESULT_SUCCESS,
                runtime_effect: effect_label,
                error_code: None,
                credential_status: None,
            };
            state
                .credential_runtime
                .record_mutation_result(operation.to_record(&request_id))
                .await;
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Ok(Json(CredentialRemoveResponse {
                runtime_effect: effect,
            }))
        }
        Err(err) => {
            let code = api_error_code(&err);
            let operation = CredentialOperation {
                action: ACTION_REMOVE,
                provider: provider.as_deref(),
                role: role.as_deref(),
                result: RESULT_FAILURE,
                runtime_effect: RUNTIME_EFFECT_NOT_APPLIED,
                error_code: Some(&code),
                credential_status: None,
            };
            state
                .credential_runtime
                .record_mutation_result(operation.to_record(&request_id))
                .await;
            publish_credential_audit(&state, &claims, &request_id, operation).await;
            Err(err)
        }
    }
}

fn require_credential_operator(state: &AppState, headers: &HeaderMap) -> Result<Claims, ApiError> {
    let header = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
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
    state
        .auth_facade
        .authorize(&claims, &Action::ManageCredentials)
        .map_err(|_err| ApiError::Forbidden {
            message: "insufficient permissions".to_owned(),
            location: snafu::location!(),
        })?;
    Ok(Claims {
        sub: claims.sub,
        role: claims.role,
        nous_id: claims.nous_id,
    })
}

fn map_symbolon_error(err: symbolon::error::Error) -> ApiError {
    match err {
        symbolon::error::Error::InvalidApiKey { .. } => bad_request("invalid credential id"),
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
        Self {
            id: credential.id,
            provider: credential.provider,
            role: credential.role.as_str().to_owned(),
            redacted_preview: credential.redacted_preview,
            status: status_str(credential.status).to_owned(),
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

fn credential_parts_from_id(id: &str) -> (Option<String>, Option<String>) {
    id.split_once(':').map_or((None, None), |(provider, role)| {
        (Some(provider.to_owned()), Some(role.to_owned()))
    })
}

async fn apply_mutation_effect(
    credential_runtime: &CredentialRuntimeManager,
    provider: &str,
) -> CredentialMutationEffect {
    let effect = credential_runtime.mutation_effect(provider);
    credential_runtime.record_effect(provider, effect).await;
    effect
}

impl CredentialOperation<'_> {
    fn to_record(self, request_id: &RequestId) -> CredentialOperationRecord {
        CredentialOperationRecord {
            provider: self.provider.map(ToOwned::to_owned),
            role: self.role.map(ToOwned::to_owned),
            action: self.action.to_owned(),
            result: self.result.to_owned(),
            runtime_effect: self.runtime_effect.to_owned(),
            request_id: request_id.0.clone(),
            error_code: self.error_code.map(ToOwned::to_owned),
            credential_status: self.credential_status.map(ToOwned::to_owned),
        }
    }
}

async fn publish_credential_audit(
    state: &AppState,
    claims: &Claims,
    request_id: &RequestId,
    operation: CredentialOperation<'_>,
) {
    let payload = serde_json::json!({
        "actor": {
            "principal": claims.sub.as_str(),
            "role": claims.role.to_string(),
            "nous_id": claims.nous_id.as_deref(),
        },
        "principal": claims.sub.as_str(),
        "provider": operation.provider,
        "role": operation.role,
        "action": operation.action,
        "result": operation.result,
        "request_id": request_id.0.as_str(),
        "runtime_effect": operation.runtime_effect,
        "error_code": operation.error_code,
    });
    state
        .event_bus
        .publish(crate::event_bus::DomainEvent::new(
            state.event_bus.next_id(),
            CREDENTIAL_AUDIT_TOPIC,
            payload,
        ))
        .await;
}

fn api_error_code(err: &ApiError) -> String {
    match err {
        ApiError::BadRequest { .. } => "bad_request".to_owned(),
        ApiError::Unauthorized { .. } => "unauthorized".to_owned(),
        ApiError::Forbidden { .. } => "forbidden".to_owned(),
        ApiError::NotFound { .. } => "not_found".to_owned(),
        ApiError::Conflict { .. } => "conflict".to_owned(),
        ApiError::RateLimited { .. } => "rate_limited".to_owned(),
        ApiError::ServiceUnavailable { .. } => "service_unavailable".to_owned(),
        ApiError::ValidationFailed { .. } => "validation_failed".to_owned(),
        ApiError::NotImplemented { .. } => "not_implemented".to_owned(),
        ApiError::SessionNotFound { .. } => "session_not_found".to_owned(),
        ApiError::NousNotFound { .. } => "nous_not_found".to_owned(),
        ApiError::StreamTurnConflict { .. } => "stream_turn_conflict".to_owned(),
        ApiError::UserFacing { code, .. } => code.clone(),
        ApiError::Internal { .. } => "internal_error".to_owned(),
    }
}
