//! Nous (agent) information endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{ApiError, ErrorResponse, NousNotFoundSnafu};
use crate::extract::Claims;
use crate::state::AppState;

/// GET /api/v1/nous — list registered nous agents.
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
            model: c.model.clone(),
            status: "active".to_owned(),
        })
        .collect();
    Json(NousListResponse { nous })
}

/// GET /api/v1/nous/{id} — get nous status.
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

/// GET /api/v1/nous/{id}/tools — list tools available to a nous.
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

// --- Response types ---

#[derive(Debug, Serialize, ToSchema)]
pub struct NousListResponse {
    pub nous: Vec<NousSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NousSummary {
    pub id: String,
    pub model: String,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NousStatus {
    pub id: String,
    pub model: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,
    pub max_tool_iterations: u32,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ToolsResponse {
    pub tools: Vec<ToolSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
    pub category: String,
}
