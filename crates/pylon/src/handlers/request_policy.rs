//! Request-policy metadata for first-party clients.

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use tracing::instrument;
use utoipa::ToSchema;

use symbolon::types::Role;
use taxis::config::AletheiaConfig;

use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::ConfigState;

/// Response from `GET /api/v1/system/request-policy`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestPolicyResponse {
    /// First-party request policy clients should use for state-changing requests.
    pub request_policy: RequestPolicy,
}

impl RequestPolicyResponse {
    fn from_config(config: &AletheiaConfig) -> Self {
        Self {
            request_policy: RequestPolicy {
                csrf: CsrfRequestPolicy {
                    enabled: config.gateway.csrf.enabled,
                    header_name: config.gateway.csrf.header_name.clone(),
                    header_value: config.gateway.csrf.header_value.expose_secret().to_owned(),
                },
            },
        }
    }
}

/// First-party request policy clients should use for state-changing requests.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestPolicy {
    /// CSRF/request-header behavior for state-changing API requests.
    pub csrf: CsrfRequestPolicy,
}

/// CSRF/request-header behavior for state-changing API requests.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CsrfRequestPolicy {
    /// Whether the server enforces the configured request header.
    pub enabled: bool,
    /// Header name to send on mutating requests.
    pub header_name: String,
    /// Header value to send on mutating requests.
    pub header_value: String, // kanon:ignore RUST/plain-string-secret -- CSRF request marker exposed only to authenticated operators; not a credential.
}

/// GET /api/v1/system/request-policy: first-party request metadata.
#[utoipa::path(
    get,
    path = "/api/v1/system/request-policy",
    responses(
        (status = 200, description = "First-party request policy", body = RequestPolicyResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn get_request_policy(
    State(state): State<ConfigState>,
    claims: Claims,
) -> Result<Json<RequestPolicyResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;
    let config = state.config.read().await;
    Ok(Json(RequestPolicyResponse::from_config(&config)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use koina::secret::SecretString;

    #[test]
    fn response_preserves_configured_csrf_policy() {
        let mut config = AletheiaConfig::default();
        config.gateway.csrf.enabled = true;
        config.gateway.csrf.header_name = "x-aletheia-csrf".to_owned();
        config.gateway.csrf.header_value = SecretString::from("custom-csrf-value");

        let response = RequestPolicyResponse::from_config(&config);

        assert!(response.request_policy.csrf.enabled);
        assert_eq!(response.request_policy.csrf.header_name, "x-aletheia-csrf");
        assert_eq!(
            response.request_policy.csrf.header_value,
            "custom-csrf-value"
        );
    }
}
