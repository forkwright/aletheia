//! Nous (agent) information endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{ApiError, ErrorResponse, NousNotFoundSnafu};
use crate::extract::Claims;
use crate::state::AppState;

/// GET /api/v1/nous: list registered nous agents.
#[utoipa::path(
    get,
    path = "/api/v1/nous",
    responses(
        (status = 200, description = "List of nous agents", body = NousListResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list(State(state): State<Arc<AppState>>, _claims: Claims) -> Json<NousListResponse> {
    let nous: Vec<NousSummary> = state
        .nous_manager
        .configs()
        .into_iter()
        .map(|c| NousSummary {
            id: c.id.clone(),
            name: c.name.clone().unwrap_or_else(|| c.id.clone()),
            model: c.model.clone(),
            status: "active".to_owned(),
        })
        .collect();
    Json(NousListResponse { nous })
}

/// GET /api/v1/nous/{id}: get nous status.
#[utoipa::path(
    get,
    path = "/api/v1/nous/{id}",
    params(("id" = String, Path, description = "Nous agent ID")),
    responses(
        (status = 200, description = "Nous status", body = NousStatus),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Nous not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_status(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<NousStatus>, ApiError> {
    let config = state
        .nous_manager
        .get_config(&id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?;

    let status = match state.nous_manager.get(&id) {
        Some(handle) => handle
            .status()
            .await
            .map_or_else(|_| "unknown".to_owned(), |s| s.lifecycle.to_string()),
        None => "unknown".to_owned(),
    };

    Ok(Json(NousStatus {
        id: config.id.clone(),
        model: config.model.clone(),
        context_window: config.context_window,
        max_output_tokens: config.max_output_tokens,
        thinking_enabled: config.thinking_enabled,
        thinking_budget: config.thinking_budget,
        max_tool_iterations: config.max_tool_iterations,
        status,
    }))
}

/// GET /api/v1/nous/{id}/tools: list tools available to a nous.
#[utoipa::path(
    get,
    path = "/api/v1/nous/{id}/tools",
    params(("id" = String, Path, description = "Nous agent ID")),
    responses(
        (status = 200, description = "Available tools", body = ToolsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Nous not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn tools(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<ToolsResponse>, ApiError> {
    if state.nous_manager.get_config(&id).is_none() {
        return Err(NousNotFoundSnafu { id }.build());
    }

    let defs = state.tool_registry.definitions();
    let tools = defs
        .into_iter()
        .map(|d| ToolSummary {
            name: d.name.as_str().to_owned(),
            description: d.description.clone(),
            category: format!("{:?}", d.category),
        })
        .collect();

    Ok(Json(ToolsResponse { tools }))
}

/// Response listing all registered nous agents.
#[derive(Debug, Serialize, ToSchema)]
pub struct NousListResponse {
    /// Agent summaries.
    pub nous: Vec<NousSummary>,
}

/// Brief overview of a registered nous agent.
#[derive(Debug, Serialize, ToSchema)]
pub struct NousSummary {
    /// Agent identifier.
    pub id: String,
    /// Human-readable display name (falls back to `id`).
    pub name: String,
    /// LLM model assigned to this agent.
    pub model: String,
    /// Lifecycle status (e.g. `"active"`).
    pub status: String,
}

/// Detailed status of a single nous agent.
#[derive(Debug, Serialize, ToSchema)]
pub struct NousStatus {
    /// Agent identifier.
    pub id: String,
    /// LLM model assigned to this agent.
    pub model: String,
    /// Maximum context window in tokens.
    pub context_window: u32,
    /// Maximum output tokens per turn.
    pub max_output_tokens: u32,
    /// Whether extended thinking is enabled.
    pub thinking_enabled: bool,
    /// Token budget for extended thinking.
    pub thinking_budget: u32,
    /// Maximum tool iterations per turn.
    pub max_tool_iterations: u32,
    /// Actor lifecycle status.
    pub status: String,
}

/// Response listing tools available to a nous agent.
#[derive(Debug, Serialize, ToSchema)]
pub struct ToolsResponse {
    /// Tool summaries.
    pub tools: Vec<ToolSummary>,
}

/// Brief description of a registered tool.
#[derive(Debug, Serialize, ToSchema)]
pub struct ToolSummary {
    /// Tool name as sent to the LLM.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Tool category (e.g. `"Builtin"`, `"Pack"`).
    pub category: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after len assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn nous_list_response_serializes_nous_array() {
        let resp = NousListResponse {
            nous: vec![NousSummary {
                id: "alice".to_owned(),
                name: "Alice".to_owned(),
                model: "anthropic/claude-opus-4-6".to_owned(),
                status: "active".to_owned(),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("nous").is_some());
        assert_eq!(json["nous"][0]["id"], "alice");
        assert_eq!(json["nous"][0]["status"], "active");
    }

    #[test]
    fn nous_list_response_empty_array() {
        let resp = NousListResponse { nous: vec![] };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["nous"].as_array().unwrap().is_empty());
    }

    #[test]
    fn nous_summary_name_falls_back_to_id() {
        // The handler constructs name as: name.unwrap_or_else(|| id.clone())
        // Test that summary with name set serializes name correctly
        let summary = NousSummary {
            id: "bob".to_owned(),
            name: "bob".to_owned(), // fallback case: name == id
            model: "anthropic/claude-sonnet-4-6".to_owned(),
            status: "active".to_owned(),
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "bob");
        assert_eq!(json["id"], "bob");
    }

    #[test]
    fn nous_status_serializes_all_config_fields() {
        let status = NousStatus {
            id: "syn".to_owned(),
            model: "anthropic/claude-opus-4-6".to_owned(),
            context_window: 200_000,
            max_output_tokens: 4096,
            thinking_enabled: true,
            thinking_budget: 10_000,
            max_tool_iterations: 10,
            status: "active".to_owned(),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["id"], "syn");
        assert_eq!(json["context_window"], 200_000);
        assert_eq!(json["thinking_enabled"], true);
        assert_eq!(json["max_tool_iterations"], 10);
    }

    #[test]
    fn tools_response_serializes_tool_list() {
        let resp = ToolsResponse {
            tools: vec![ToolSummary {
                name: "read_file".to_owned(),
                description: "Read a file from disk".to_owned(),
                category: "Builtin".to_owned(),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["tools"][0]["name"], "read_file");
        assert_eq!(json["tools"][0]["category"], "Builtin");
    }

    #[test]
    fn nous_not_found_error_is_404() {
        use crate::error::{ApiError, NousNotFoundSnafu};
        use axum::response::IntoResponse;
        let err: ApiError = NousNotFoundSnafu {
            id: "unknown-nous".to_owned(),
        }
        .build();
        let response = err.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
