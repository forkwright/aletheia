//! Operator credential management endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use koina::http::BEARER_PREFIX;
use koina::secret::SecretString;
use serde::{Deserialize, Serialize};
use symbolon::types::{Action, ManagedCredential, ManagedCredentialRole, ManagedCredentialStatus};
use tracing::instrument;
use utoipa::{IntoParams, ToSchema};

use crate::error::ApiError;
use crate::state::AppState;

/// Response body for credential list and mutation endpoints.
#[derive(Debug, Serialize, ToSchema)]
pub struct CredentialsListResponse {
    /// Secret-safe credential metadata.
    pub credentials: Vec<CredentialResponse>,
}

/// Secret-safe credential metadata returned to clients.
#[derive(Debug, Serialize, ToSchema)]
pub struct CredentialResponse {
    /// Stable identifier in `{provider}:{role}` form.
    pub id: String,
    /// Provider name associated with the credential.
    pub provider: String,
    /// Role of this credential for its provider.
    pub role: String,
    /// Redacted key preview, never raw secret material.
    pub masked_key: String,
    /// Local validation status.
    pub status: String,
    /// Last validation timestamp when produced by a validation call.
    pub last_validated: Option<String>,
    /// Request counter reserved for provider metrics.
    pub requests_today: u64,
    /// Token counter reserved for provider metrics.
    pub tokens_today: u64,
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
            .map(CredentialResponse::from)
            .collect(),
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
    Json(request): Json<AddCredentialRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_credential_operator(&state, &headers)?;
    let role = request
        .role
        .parse::<ManagedCredentialRole>()
        .map_err(|_role_err| bad_request("role must be primary or backup"))?;
    let root = state.oikos.credentials();
    let credential = state
        .auth_facade
        .add_credential(&root, request.provider.trim(), &request.key, role)
        .map_err(map_symbolon_error)?;
    Ok((
        StatusCode::CREATED,
        Json(CredentialResponse::from(credential)),
    ))
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
    Path(id): Path<String>,
) -> Result<Json<CredentialResponse>, ApiError> {
    require_credential_operator(&state, &headers)?;
    let root = state.oikos.credentials();
    let credential = state
        .auth_facade
        .validate_credential(&root, &id)
        .map_err(map_symbolon_error)?;
    Ok(Json(CredentialResponse::from(credential)))
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
    Query(query): Query<RotateCredentialQuery>,
) -> Result<Json<CredentialsListResponse>, ApiError> {
    require_credential_operator(&state, &headers)?;
    let root = state.oikos.credentials();
    let credentials = state
        .auth_facade
        .rotate_credentials(&root, query.provider.trim())
        .map_err(map_symbolon_error)?;
    Ok(Json(CredentialsListResponse {
        credentials: credentials
            .into_iter()
            .map(CredentialResponse::from)
            .collect(),
    }))
}

/// DELETE /api/v1/system/credentials/{id}: remove one managed credential.
#[utoipa::path(
    delete,
    path = "/api/v1/system/credentials/{id}",
    params(("id" = String, Path, description = "Credential id in provider:role form")),
    responses(
        (status = 204, description = "Credential removed"),
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
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_credential_operator(&state, &headers)?;
    let root = state.oikos.credentials();
    state
        .auth_facade
        .remove_credential(&root, &id)
        .map_err(map_symbolon_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn require_credential_operator(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
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
            tracing::error!(error = %err, "credential store error");
            ApiError::Internal {
                message: err.to_string(),
                location: snafu::location!(),
            }
        }
        _ => {
            tracing::error!(error = %err, "credential API error");
            ApiError::Internal {
                message: err.to_string(),
                location: snafu::location!(),
            }
        }
    }
}

fn bad_request(message: &str) -> ApiError {
    ApiError::BadRequest {
        message: message.to_owned(),
        location: snafu::location!(),
    }
}

impl From<ManagedCredential> for CredentialResponse {
    fn from(credential: ManagedCredential) -> Self {
        Self {
            id: credential.id,
            provider: credential.provider,
            role: credential.role.as_str().to_owned(),
            masked_key: credential.redacted_preview,
            status: status_str(credential.status).to_owned(),
            last_validated: credential.last_validated,
            requests_today: 0,
            tokens_today: 0,
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
