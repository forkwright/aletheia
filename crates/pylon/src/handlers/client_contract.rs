//! First-party client contract discovery.

use axum::Json;
use axum::extract::Extension;
use serde::Serialize;

use crate::extract::Claims;
use crate::middleware::CsrfState;

/// First-party client runtime contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientContractResponse {
    /// CSRF contract currently enforced by the running middleware.
    pub csrf: CsrfClientContract,
}

/// CSRF header requirements for first-party clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CsrfClientContract {
    /// Whether mutating requests must include the CSRF header.
    pub enabled: bool,
    /// Header name to send when CSRF is enabled.
    pub header_name: String,
    /// Header value to send when CSRF is enabled.
    ///
    /// This is available only from the authenticated client contract endpoint,
    /// not from public health or redacted config APIs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_value: Option<String>,
}

/// GET /api/v1/client/contract: authenticated first-party client settings.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no side
/// effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/client/contract",
    responses(
        (status = 200, description = "First-party client runtime contract", body = ClientContractResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get(
    _claims: Claims,
    Extension(csrf): Extension<CsrfState>,
) -> Json<ClientContractResponse> {
    Json(ClientContractResponse {
        csrf: CsrfClientContract {
            enabled: csrf.enabled,
            header_name: csrf.header_name,
            header_value: csrf
                .enabled
                .then(|| csrf.header_value.expose_secret().to_owned()),
        },
    })
}
