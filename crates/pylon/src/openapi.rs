//! `OpenAPI` specification generation.
#![expect(
    clippy::expect_used,
    reason = "OpenAPI JSON serialization is infallible"
)]
#![allow(clippy::needless_for_each)] // triggered by utoipa OpenApi derive macro

use axum::http::header;
use axum::response::IntoResponse;

use utoipa::OpenApi;

use aletheia_koina::http::CONTENT_TYPE_JSON;

/// Utoipa `OpenAPI` spec root: aggregates all API paths and schemas.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Aletheia API",
        version = "1.0.0",
        description = "HTTP gateway for the Aletheia agent platform.\n\n## Stability Tiers\n\n- **Stable (v1):** Session and nous endpoints under `/api/v1/`. Breaking changes require a version bump to v2.\n- **Infrastructure:** Health and docs endpoints. Unversioned, may change without notice."
    ),
    paths(
        crate::handlers::health::check,
        crate::handlers::metrics::expose,
        crate::handlers::sessions::list_sessions,
        crate::handlers::sessions::create,
        crate::handlers::sessions::get_session,
        crate::handlers::sessions::close,
        crate::handlers::sessions::archive,
        crate::handlers::sessions::unarchive,
        crate::handlers::sessions::rename,
        crate::handlers::sessions::streaming::send_message,
        crate::handlers::sessions::streaming::stream_turn,
        crate::handlers::sessions::streaming::events,
        crate::handlers::sessions::history,
        crate::handlers::nous::list,
        crate::handlers::nous::get_status,
        crate::handlers::nous::tools,
        crate::handlers::config::get_config,
        crate::handlers::config::get_section,
        crate::handlers::config::update_section,
        crate::handlers::knowledge::list_facts,
        crate::handlers::knowledge::get_fact,
        crate::handlers::knowledge::forget_fact,
        crate::handlers::knowledge::restore_fact,
        crate::handlers::knowledge::update_confidence,
        crate::handlers::knowledge::list_entities,
        crate::handlers::knowledge::entity_relationships,
        crate::handlers::knowledge::search,
        crate::handlers::knowledge::timeline,
    ),
    components(schemas(
        crate::handlers::health::HealthResponse,
        crate::handlers::health::HealthCheck,
        crate::handlers::sessions::types::CreateSessionRequest,
        crate::handlers::sessions::types::RenameSessionRequest,
        crate::handlers::sessions::types::SendMessageRequest,
        crate::handlers::sessions::types::SessionResponse,
        crate::handlers::sessions::types::ListSessionsResponse,
        crate::handlers::sessions::types::SessionListItem,
        crate::handlers::sessions::types::HistoryResponse,
        crate::handlers::sessions::types::HistoryMessage,
        crate::handlers::nous::NousListResponse,
        crate::handlers::nous::NousSummary,
        crate::handlers::nous::NousStatus,
        crate::handlers::nous::ToolsResponse,
        crate::handlers::nous::ToolSummary,
        crate::handlers::config::ConfigUpdateResponse,
        crate::error::ErrorResponse,
        crate::error::ErrorBody,
        crate::stream::SseEvent,
        crate::stream::UsageData,
    )),
    modifiers(&SecurityAddon),
)]
pub(crate) struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                utoipa::openapi::security::SecurityScheme::Http(
                    utoipa::openapi::security::Http::new(
                        utoipa::openapi::security::HttpAuthScheme::Bearer,
                    ),
                ),
            );
        }
    }
}

/// Serve the generated `OpenAPI` specification as JSON.
pub async fn openapi_json() -> impl IntoResponse {
    let spec = ApiDoc::openapi()
        .to_json()
        .expect("OpenAPI spec serialization");
    ([(header::CONTENT_TYPE, CONTENT_TYPE_JSON)], spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_spec_serializes_without_panic() {
        #[expect(clippy::expect_used, reason = "test assertion")]
        let json = ApiDoc::openapi()
            .to_json()
            .expect("OpenAPI spec serialization must not fail");
        assert!(!json.is_empty());
    }

    #[test]
    fn openapi_spec_json_contains_api_health_path() {
        #[expect(clippy::expect_used, reason = "test assertion")]
        let json = ApiDoc::openapi()
            .to_json()
            .expect("OpenAPI spec serialization must not fail");
        assert!(
            json.contains("/api/health"),
            "spec JSON must include the health endpoint path"
        );
    }
}
