//! Nous (agent) information endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;

use crate::error::{ApiError, NousNotFoundSnafu};
use crate::extract::Claims;
use crate::state::AppState;

/// GET /api/nous — list registered nous agents.
///
/// Reads the nous configuration registry from [`AppState`] and returns a summary
/// of every registered agent.
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

/// GET /api/nous/{id} — get nous status.
///
/// Looks up the nous by id in [`AppState`] and queries the live actor for its
/// current lifecycle status. Returns 404 if the nous id is not registered.
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

/// GET /api/nous/{id}/tools — list tools available to a nous.
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

/// Response body for `GET /api/nous`.
#[derive(Debug, Serialize)]
pub struct NousListResponse {
    pub nous: Vec<NousSummary>,
}

/// Brief summary of a registered nous agent.
#[derive(Debug, Serialize)]
pub struct NousSummary {
    /// Unique agent identifier.
    pub id: String,
    /// LLM model name.
    pub model: String,
    /// Current lifecycle status (e.g., `"active"`, `"idle"`).
    pub status: String,
}

/// Full configuration and runtime status for a nous agent.
#[derive(Debug, Serialize)]
pub struct NousStatus {
    pub id: String,
    pub model: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,
    pub max_tool_iterations: u32,
    /// Current lifecycle status string.
    pub status: String,
}

/// Response body for `GET /api/nous/{id}/tools`.
#[derive(Debug, Serialize)]
pub struct ToolsResponse {
    pub tools: Vec<ToolSummary>,
}

/// Brief summary of a tool available to a nous agent.
#[derive(Debug, Serialize)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
    pub category: String,
}
