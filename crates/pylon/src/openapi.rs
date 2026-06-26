//! `OpenAPI` specification generation.
#![expect(
    clippy::expect_used,
    reason = "OpenAPI JSON serialization is infallible"
)]
#![expect(
    clippy::needless_for_each,
    reason = "utoipa OpenApi derive expands to generated for_each loops"
)]

use std::sync::Arc;

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use utoipa::OpenApi;

use koina::http::CONTENT_TYPE_JSON;

use crate::state::AppState;

/// Utoipa `OpenAPI` spec root -- aggregates all API paths and schemas.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Aletheia API",
        version = "1.0.0",
        description = "HTTP gateway for the Aletheia agent platform.\n\n## Stability Tiers\n\n- **Stable (v1):** Session and nous endpoints under `/api/v1/`. Breaking changes require a version bump to v2.\n- **Infrastructure:** Health and docs endpoints. Unversioned, may change without notice."
    ),
    paths(
        crate::handlers::health::check,
        crate::handlers::health::detailed,
        crate::handlers::health::deprecated_health_check,
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
        crate::handlers::sessions::streaming::reconnect_turn,
        crate::handlers::sessions::approvals::resolve,
        crate::handlers::sessions::approvals::approve_tool,
        crate::handlers::sessions::approvals::deny_tool,
        crate::handlers::sessions::history,
        crate::handlers::events::subscribe,
        crate::handlers::events::discovery,
        crate::handlers::ops::tools,
        crate::handlers::credentials::list_credentials,
        crate::handlers::credentials::add_credential,
        crate::handlers::credentials::validate_credential,
        crate::handlers::credentials::rotate_credentials,
        crate::handlers::credentials::remove_credential,
        crate::handlers::nous::list,
        crate::handlers::nous::get_status,
        crate::handlers::nous::tools,
        crate::handlers::nous::update_enabled,
        crate::handlers::nous::update_tool,
        crate::handlers::nous::recover,
        crate::handlers::nous::create,
        crate::handlers::workspace::list_files,
        crate::handlers::workspace::git_status,
        crate::handlers::workspace::file_content,
        crate::handlers::workspace::write_file_content,
        crate::handlers::workspace::open_file,
        crate::handlers::workspace::file_diff,
        crate::handlers::workspace::search,
        crate::handlers::config::get_config,
        crate::handlers::config::get_section,
        crate::handlers::config::update_section,
        crate::handlers::knowledge::list_facts,
        crate::handlers::knowledge::import_facts,
        crate::handlers::knowledge::ingest,
        crate::handlers::knowledge::webhook_ingest,
        crate::handlers::knowledge::get_fact,
        crate::handlers::knowledge::get_entity,
        crate::handlers::knowledge::entity_memories,
        crate::handlers::knowledge::merge_entities,
        crate::handlers::knowledge::flag_entity,
        crate::handlers::knowledge::delete_entity,
        crate::handlers::knowledge::forget_fact,
        crate::handlers::knowledge::restore_fact,
        crate::handlers::knowledge::update_confidence,
        crate::handlers::knowledge::update_sensitivity,
        crate::handlers::knowledge::list_entities,
        crate::handlers::knowledge::entity_relationships,
        crate::handlers::knowledge::search,
        crate::handlers::knowledge::explain,
        crate::handlers::knowledge::timeline,
        crate::handlers::knowledge::check_graph_health,
        crate::handlers::sessions::purge,
        crate::handlers::config::reload_config,
        crate::handlers::insights::get_agent_perf,
        crate::handlers::insights::get_agent_perf_one,
        crate::handlers::insights::get_quality_metrics,
        crate::handlers::insights::get_token_metrics,
        crate::handlers::insights::get_cost_metrics,
        crate::handlers::insights::get_journal,
        crate::handlers::planning::get_verification,
        crate::handlers::planning::refresh_verification,
        crate::handlers::providers::list,
        crate::handlers::providers::route,
    ),
    components(schemas(
        crate::handlers::health::HealthResponse,
        crate::handlers::health::HealthCheck,
        crate::handlers::health::LivenessResponse,
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
        crate::handlers::nous::NousToggleRequest,
        crate::handlers::nous::ToolToggleRequest,
        crate::handlers::nous::AgentDefinition,
        crate::handlers::nous::CreateAgentResponse,
        crate::handlers::nous::RecoverResponse,
        crate::handlers::ops::OpsToolsResponse,
        crate::handlers::ops::ToolCatalogEntry,
        crate::handlers::ops::LiveInvocationEntry,
        crate::handlers::credentials::CredentialsListResponse,
        crate::handlers::credentials::CredentialResponse,
        crate::handlers::credentials::CredentialRemoveResponse,
        crate::handlers::credentials::AddCredentialRequest,
        crate::handlers::credentials::RotateCredentialQuery,
        crate::credential_runtime::CredentialMutationEffect,
        crate::handlers::workspace::FileEntry,
        crate::handlers::workspace::GitStatusEntry,
        crate::handlers::workspace::SearchResult,
        crate::handlers::workspace::FilesQuery,
        crate::handlers::workspace::ContentQuery,
        crate::handlers::workspace::DiffQuery,
        crate::handlers::workspace::SearchQuery,
        crate::handlers::workspace::WriteContentRequest,
        crate::handlers::workspace::WriteContentResponse,
        crate::handlers::workspace::OpenRequest,
        crate::handlers::workspace::OpenResponse,
        crate::handlers::config::ConfigUpdateResponse,
        crate::handlers::config::ConfigReloadResponse,
        crate::handlers::config::ConfigSectionPayload,
        crate::handlers::config::AgentsConfig,
        crate::handlers::config::GatewayConfig,
        crate::handlers::config::ChannelsConfig,
        crate::handlers::config::FeatureFlagConfig,
        crate::handlers::config::ChannelBinding,
        crate::handlers::config::EmbeddingSettings,
        crate::handlers::config::DataConfig,
        crate::handlers::config::MaintenanceConfig,
        crate::handlers::config::ModelPricing,
        crate::handlers::knowledge::IngestRequest,
        crate::handlers::knowledge::IngestResponse,
        crate::handlers::knowledge::EntitiesResponse,
        crate::handlers::knowledge::EntityListItem,
        crate::handlers::knowledge::RelationshipsResponse,
        crate::handlers::knowledge::EntityRelationship,
        crate::handlers::knowledge::RelationshipDirection,
        crate::handlers::knowledge::EntityMemory,
        crate::handlers::knowledge::MergeRequest,
        crate::handlers::knowledge::FlagRequest,
        crate::handlers::knowledge::FlagSeverity,
        crate::handlers::knowledge::SearchResult,
        crate::handlers::knowledge::SearchResponse,
        crate::handlers::knowledge::FactorScoreBreakdown,
        crate::handlers::knowledge::ExplainDecision,
        crate::handlers::knowledge::ExplainCandidate,
        crate::handlers::knowledge::RecallWeightsView,
        crate::handlers::knowledge::ExplainResponse,
        crate::handlers::knowledge::WebhookIngestResponse,
        crate::error::ErrorResponse,
        crate::error::ErrorBody,
        crate::error::FieldError,
        crate::stream::SseEvent,
        crate::stream::UsageData,
        crate::types::insights::AgentPerformance,
        crate::types::insights::AgentPerformanceListResponse,
        crate::types::insights::AnomalyAlert,
        crate::types::insights::UnavailableMetric,
        crate::types::insights::QualityMetricsResponse,
        crate::types::insights::QualitySeries,
        crate::types::insights::TimeSeriesPoint,
        crate::types::insights::TokenMetricsResponse,
        crate::types::insights::TokenSeriesPoint,
        crate::types::insights::AgentTokenRow,
        crate::types::insights::ModelTokenRow,
        crate::types::insights::CostMetricsResponse,
        crate::types::insights::CostSeriesPoint,
        crate::types::insights::AgentCostRow,
        crate::types::insights::JournalEvent,
        crate::types::insights::JournalResponse,
        crate::handlers::providers::ProviderListResponse,
        crate::handlers::providers::ProviderInfo,
        crate::handlers::providers::ProviderRouteResponse,
        crate::handlers::providers::ModelProviderReadiness,
    )),
    modifiers(&VersionFromCrate),
)]
pub(crate) struct ApiDoc;

/// Override the hardcoded `OpenAPI` `info.version` with the crate version
/// from `Cargo.toml` so the spec tracks releases automatically.
struct VersionFromCrate;

impl utoipa::Modify for VersionFromCrate {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        env!("CARGO_PKG_VERSION").clone_into(&mut openapi.info.version);
    }
}

fn auth_requires_bearer(auth_mode: &str) -> bool {
    auth_mode != "none"
}

fn add_bearer_auth_scheme(value: &mut serde_json::Value) {
    let components = value
        .as_object_mut()
        .expect("OpenAPI JSON root must be an object")
        .entry("components")
        .or_insert_with(|| serde_json::json!({}));
    let security_schemes = components
        .as_object_mut()
        .expect("OpenAPI components must be an object")
        .entry("securitySchemes")
        .or_insert_with(|| serde_json::json!({}));
    security_schemes
        .as_object_mut()
        .expect("OpenAPI securitySchemes must be an object")
        .insert(
            "bearer_auth".to_owned(),
            serde_json::json!({
                "type": "http",
                "scheme": "bearer"
            }),
        );
}

fn remove_operation_security(value: &mut serde_json::Value) {
    let Some(paths) = value
        .get_mut("paths")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for path_item in paths.values_mut() {
        let Some(operations) = path_item.as_object_mut() else {
            continue;
        };
        for operation in operations.values_mut() {
            if let Some(operation) = operation.as_object_mut() {
                operation.remove("security");
            }
        }
    }
}

pub(crate) fn openapi_value_for_auth_mode(auth_mode: &str) -> serde_json::Value {
    let mut value = serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI spec serialization");
    if auth_requires_bearer(auth_mode) {
        add_bearer_auth_scheme(&mut value);
    } else {
        remove_operation_security(&mut value);
    }
    value
}

/// Serve the generated `OpenAPI` specification as JSON.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn openapi_json(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let spec = serde_json::to_string(&openapi_value_for_auth_mode(&state.auth_mode))
        .expect("OpenAPI spec serialization");
    ([(header::CONTENT_TYPE, CONTENT_TYPE_JSON)], spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_spec_serializes_without_panic() {
        #[expect(clippy::expect_used, reason = "test assertion")]
        let json = serde_json::to_string(&openapi_value_for_auth_mode("token"))
            .expect("OpenAPI spec serialization must not fail");
        assert!(!json.is_empty());
    }

    #[test]
    fn openapi_spec_version_tracks_crate_version() {
        let spec = ApiDoc::openapi();
        assert_eq!(
            spec.info.version,
            env!("CARGO_PKG_VERSION"),
            "OpenAPI spec version must match CARGO_PKG_VERSION"
        );
    }

    #[test]
    fn openapi_spec_json_contains_api_health_path() {
        #[expect(clippy::expect_used, reason = "test assertion")]
        let json = serde_json::to_string(&openapi_value_for_auth_mode("token"))
            .expect("OpenAPI spec serialization must not fail");
        assert!(
            json.contains("/api/health"),
            "spec JSON must include the health endpoint path"
        );
    }

    #[test]
    fn openapi_spec_omits_bearer_auth_when_auth_mode_none() {
        let spec = openapi_value_for_auth_mode("none");
        let components = spec
            .get("components")
            .and_then(serde_json::Value::as_object)
            .expect("components object");
        let security_schemes = components
            .get("securitySchemes")
            .and_then(serde_json::Value::as_object);
        assert!(
            security_schemes.is_none_or(|schemes| schemes.get("bearer_auth").is_none()),
            "auth_mode=none must not advertise bearer_auth"
        );
        let paths = spec
            .get("paths")
            .and_then(serde_json::Value::as_object)
            .expect("paths object");
        for path_item in paths.values() {
            for operation in path_item.as_object().expect("path item object").values() {
                assert!(
                    operation.get("security").is_none(),
                    "auth_mode=none must not mark operations as bearer-secured"
                );
            }
        }
    }

    #[test]
    fn openapi_spec_advertises_bearer_auth_when_auth_mode_token() {
        let spec = openapi_value_for_auth_mode("token");
        let bearer_auth = spec
            .get("components")
            .and_then(|components| components.get("securitySchemes"))
            .and_then(|schemes| schemes.get("bearer_auth"));
        assert!(
            bearer_auth.is_some(),
            "token auth must advertise bearer_auth"
        );
    }

    #[test]
    fn bulk_import_openapi_description_matches_configured_limit() {
        let spec = openapi_value_for_auth_mode("token");
        let default_limit = taxis::config::ApiLimitsConfig::default().max_import_batch_size;
        let description = spec
            .get("paths")
            .and_then(|paths| paths.get("/api/v1/knowledge/facts/import"))
            .and_then(|path| path.get("post"))
            .and_then(|op| op.get("requestBody"))
            .and_then(|body| body.get("description"))
            .and_then(|d| d.as_str())
            .expect("bulk import requestBody description must be present");

        assert!(
            description.contains("max_import_batch_size"),
            "OpenAPI description must name the config key that controls the limit"
        );
        assert!(
            description.contains(&default_limit.to_string()),
            "OpenAPI description must mention the current default limit ({default_limit})"
        );
    }
}
