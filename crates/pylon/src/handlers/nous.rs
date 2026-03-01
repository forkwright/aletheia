//! Nous (agent) information endpoints.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;

use crate::error::{ApiError, NousNotFoundSnafu};
use crate::extract::Claims;
use crate::state::AppState;

/// GET /api/nous — list registered nous agents.
pub async fn list(State(state): State<Arc<AppState>>, _claims: Claims) -> Json<NousListResponse> {
    let config = state.session_manager.config();
    // Currently single-nous — return the configured agent
    Json(NousListResponse {
        nous: vec![NousSummary {
            id: config.id.clone(),
            model: config.model.clone(),
            status: "active".to_owned(),
        }],
    })
}

/// GET /api/nous/{id} — get nous status.
pub async fn get_status(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<NousStatus>, ApiError> {
    let config = state.session_manager.config();
    if config.id != id {
        return Err(NousNotFoundSnafu { id }.build());
    }

    Ok(Json(NousStatus {
        id: config.id.clone(),
        model: config.model.clone(),
        context_window: config.context_window,
        max_output_tokens: config.max_output_tokens,
        thinking_enabled: config.thinking_enabled,
        thinking_budget: config.thinking_budget,
        max_tool_iterations: config.max_tool_iterations,
        status: "active".to_owned(),
    }))
}

/// GET /api/nous/{id}/tools — list tools available to a nous.
pub async fn tools(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<ToolsResponse>, ApiError> {
    let config = state.session_manager.config();
    if config.id != id {
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

#[derive(Debug, Serialize)]
pub struct NousListResponse {
    pub nous: Vec<NousSummary>,
}

#[derive(Debug, Serialize)]
pub struct NousSummary {
    pub id: String,
    pub model: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct ToolsResponse {
    pub tools: Vec<ToolSummary>,
}

#[derive(Debug, Serialize)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
    pub category: String,
}
