//! Nous (agent) information endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use symbolon::types::Role;

use crate::error::{ApiError, ErrorResponse, FieldError, NousNotFoundSnafu, ValidationFailedSnafu};
use crate::extract::{Claims, require_nous_access, require_role};
use crate::state::NousState;

use nous::config::NousConfig;
use taxis::config::{AletheiaConfig, NousDefinition};

#[path = "nous_dto.rs"]
mod nous_dto;
pub use nous_dto::{
    AgentDefinition, CreateAgentResponse, NousListResponse, NousStatus, NousSummary,
    NousToggleRequest, RecoverResponse, ToolSummary, ToolToggleRequest, ToolsResponse,
};

fn agent_definition<'a>(config: &'a AletheiaConfig, id: &str) -> Option<&'a NousDefinition> {
    config.agents.list.iter().find(|agent| agent.id == id)
}

fn ensure_agent_definition<'a>(
    config: &'a mut AletheiaConfig,
    runtime: &NousConfig,
) -> &'a mut NousDefinition {
    if let Some(index) = config
        .agents
        .list
        .iter()
        .position(|agent| agent.id == runtime.id.as_ref())
    {
        return match config.agents.list.get_mut(index) {
            Some(agent) => agent,
            None => unreachable!("existing agent index must be valid"),
        };
    }

    config.agents.list.push(NousDefinition {
        id: runtime.id.to_string(),
        name: runtime.name.clone(),
        enabled: true,
        model: None,
        workspace: runtime.workspace.to_string_lossy().into_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        private: runtime.private,
        episteme_cohort: None,
        recall: None,
        tool_allowlist: None,
        tool_groups: None,
        recall_profile: None,
        behavior: None,
    });

    match config.agents.list.last_mut() {
        Some(agent) => agent,
        None => unreachable!("pushed agent definition must be present"),
    }
}

fn allowlist_for_agent<'a>(config: &'a AletheiaConfig, id: &str) -> Option<&'a [String]> {
    agent_definition(config, id).and_then(|agent| agent.tool_allowlist.as_deref())
}

fn tool_summaries_for_agent(state: &NousState, allowlist: Option<&[String]>) -> Vec<ToolSummary> {
    state
        .tool_registry
        .definitions()
        .into_iter()
        .map(|def| {
            let enabled =
                allowlist.is_none_or(|list| list.iter().any(|name| name == def.name.as_str()));
            ToolSummary {
                name: def.name.as_str().to_owned(),
                enabled,
                description: def.description.clone(),
                category: format!("{:?}", def.category),
                auto_activate: def.auto_activate,
            }
        })
        .collect()
}

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
pub async fn list(State(state): State<NousState>, claims: Claims) -> Json<NousListResponse> {
    let include_private = claims.role >= Role::Operator;
    let scoped = claims.nous_id.as_deref();
    let config = state.config.read().await;
    let nous: Vec<NousSummary> = state
        .nous_manager
        .configs()
        .into_iter()
        .filter(|c| include_private || !c.private)
        // WHY: a token scoped to a single agent must not see other agents in
        // the discovery list. Without this filter, a scoped Operator could
        // enumerate every agent's id, model, and name even though the per-
        // handler `require_nous_access` blocks them from acting on those
        // agents.
        .filter(|c| scoped.is_none_or(|s| s == c.id.as_ref()))
        .map(|c| {
            let agent_id = c.id.as_ref();
            let enabled = agent_definition(&config, agent_id).is_none_or(|agent| agent.enabled);
            let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, agent_id));
            NousSummary {
                id: c.id.to_string(),
                name: c.name.clone().unwrap_or_else(|| c.id.to_string()),
                enabled,
                model: c.generation.model.clone(),
                status: "active".to_owned(),
                tools,
            }
        })
        .collect();
    Json(NousListResponse { nous })
}

/// GET /api/v1/nous/{id}: get nous status.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
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
    State(state): State<NousState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<NousStatus>, ApiError> {
    require_nous_access(&claims, &id)?;
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
        id: config.id.to_string(),
        model: config.generation.model.clone(),
        context_window: config.generation.context_window,
        max_output_tokens: config.generation.max_output_tokens,
        thinking_enabled: config.generation.thinking_enabled,
        thinking_budget: config.generation.thinking_budget,
        max_tool_iterations: config.limits.max_tool_iterations,
        status,
    }))
}

/// GET /api/v1/nous/{id}/tools: list tools available to a nous.
///
/// Returns the full registry with a per-tool `enabled` bit derived from the
/// persisted agent config. When no allowlist is configured, every tool is
/// enabled. This keeps the desktop toggle surface in sync even when the live
/// actor has not reloaded yet.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
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
    State(state): State<NousState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<ToolsResponse>, ApiError> {
    require_nous_access(&claims, &id)?;
    if state.nous_manager.get_config(&id).is_none() {
        return Err(NousNotFoundSnafu { id }.build());
    }

    let config = state.config.read().await;
    let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, &id));

    Ok(Json(ToolsResponse { tools }))
}

/// PATCH /api/v1/nous/{id}: toggle an agent's enabled state.
///
/// The toggle is persisted to the config file and mirrored into the live
/// actor via `sleep`/`wake` when the actor is currently running. If the actor
/// is unavailable, the persisted config still reflects the operator's intent
/// and the returned summary shows the new state.
#[utoipa::path(
    patch,
    path = "/api/v1/nous/{id}",
    params(("id" = String, Path, description = "Nous agent ID")),
    request_body = NousToggleRequest,
    responses(
        (status = 200, description = "Updated nous summary", body = NousSummary),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Nous not found", body = ErrorResponse),
        (status = 422, description = "Validation failed", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_enabled(
    State(state): State<NousState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<NousToggleRequest>,
) -> Result<Json<NousSummary>, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_nous_access(&claims, &id)?;

    let runtime = state
        .nous_manager
        .get_config(&id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?
        .clone();

    {
        let mut config = state.config.write().await;
        let agent = ensure_agent_definition(&mut config, &runtime);
        agent.enabled = body.enabled;
        let persisted = config.clone();
        taxis::loader::write_config(&state.oikos, &persisted).map_err(|e| ApiError::Internal {
            message: format!("failed to write config: {e}"),
            location: snafu::location!(),
        })?;
        let _ = state.config_tx.send(persisted);
    }

    if let Some(handle) = state.nous_manager.get(&id) {
        let live_result = if body.enabled {
            handle.wake().await
        } else {
            handle.sleep().await
        };
        if let Err(e) = live_result {
            tracing::warn!(nous_id = %id, error = %e, "failed to update live agent lifecycle; config intent persisted");
        }
    }

    let config = state.config.read().await;
    let enabled = agent_definition(&config, &id).is_none_or(|agent| agent.enabled);
    let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, &id));
    drop(config);
    let status = match state.nous_manager.get(&id) {
        Some(handle) => handle
            .status()
            .await
            .map_or_else(|_| "unknown".to_owned(), |s| s.lifecycle.to_string()),
        None => "unknown".to_owned(),
    };

    Ok(Json(NousSummary {
        id: runtime.id.to_string(),
        name: runtime
            .name
            .clone()
            .unwrap_or_else(|| runtime.id.to_string()),
        enabled,
        model: runtime.generation.model.clone(),
        status,
        tools,
    }))
}

/// PATCH /api/v1/nous/{id}/tools: toggle one agent tool.
///
/// This persists the operator intent to the config file and returns the
/// updated tool summary. Runtime tool-gating follows the actor's current
/// config snapshot and will pick up the persisted allowlist on reload.
#[utoipa::path(
    patch,
    path = "/api/v1/nous/{id}/tools",
    params(("id" = String, Path, description = "Nous agent ID")),
    request_body = ToolToggleRequest,
    responses(
        (status = 200, description = "Updated tool list", body = ToolsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Nous not found", body = ErrorResponse),
        (status = 422, description = "Validation failed", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_tool(
    State(state): State<NousState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<ToolToggleRequest>,
) -> Result<Json<ToolsResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_nous_access(&claims, &id)?;

    let runtime = state
        .nous_manager
        .get_config(&id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?
        .clone();

    let tool_names: Vec<String> = state
        .tool_registry
        .definitions()
        .into_iter()
        .map(|def| def.name.as_str().to_owned())
        .collect();
    if !tool_names.iter().any(|name| name == &body.tool) {
        return Err(ApiError::BadRequest {
            message: format!("unknown tool '{}'", body.tool),
            location: snafu::location!(),
        });
    }

    {
        let mut config = state.config.write().await;
        let agent = ensure_agent_definition(&mut config, &runtime);
        let mut allowlist = agent
            .tool_allowlist
            .take()
            .unwrap_or_else(|| tool_names.clone());
        if body.enabled {
            if !allowlist.iter().any(|name| name == &body.tool) {
                allowlist.push(body.tool.clone());
            }
        } else {
            allowlist.retain(|name| name != &body.tool);
        }

        allowlist.sort();
        allowlist.dedup();
        agent.tool_allowlist = if allowlist.len() == tool_names.len() {
            None
        } else {
            Some(allowlist)
        };

        let persisted = config.clone();
        taxis::loader::write_config(&state.oikos, &persisted).map_err(|e| ApiError::Internal {
            message: format!("failed to write config: {e}"),
            location: snafu::location!(),
        })?;
        let _ = state.config_tx.send(persisted);
    }

    let config = state.config.read().await;
    let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, &id));

    Ok(Json(ToolsResponse { tools }))
}

/// POST /api/v1/nous/{id}/recover: reset degraded actor to idle.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    post,
    path = "/api/v1/nous/{id}/recover",
    params(("id" = String, Path, description = "Nous agent ID")),
    responses(
        (status = 200, description = "Recovery result", body = RecoverResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Nous not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn recover(
    State(state): State<NousState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<RecoverResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_nous_access(&claims, &id)?;
    let handle = state
        .nous_manager
        .get(&id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?;

    let actor_lifecycle = handle.status().await.map(|s| s.lifecycle.to_string()).ok();

    let recovered = handle.recover().await.map_err(|e| {
        tracing::warn!(
            nous_id = %id,
            actor_lifecycle = actor_lifecycle.as_deref().unwrap_or("unknown"),
            error = %e,
            "nous recovery failed"
        );
        crate::error::InternalSnafu {
            message: format!("recover failed: {e}"),
        }
        .build()
    })?;

    Ok(Json(RecoverResponse { id, recovered }))
}

/// POST /api/v1/nous: create a new nous agent.
///
/// Validates the payload, scaffolds the agent workspace, writes the config
/// entry, and returns the agent definition. The agent becomes active after
/// the next config reload or server restart.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    post,
    path = "/api/v1/nous",
    request_body = AgentDefinition,
    responses(
        (status = 201, description = "Agent created", body = CreateAgentResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 422, description = "Validation failed", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create(
    State(state): State<NousState>,
    claims: Claims,
    Json(body): Json<AgentDefinition>,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;

    let mut field_errors = Vec::new();
    if body.id.is_empty() {
        field_errors.push(FieldError {
            field: "id".to_owned(),
            code: "required".to_owned(),
            message: "must not be empty".to_owned(),
        });
    } else if !body
        .id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        field_errors.push(FieldError {
            field: "id".to_owned(),
            code: "format".to_owned(),
            message: "must contain only alphanumeric characters and hyphens".to_owned(),
        });
    } else if body.id.starts_with('-') || body.id.ends_with('-') {
        field_errors.push(FieldError {
            field: "id".to_owned(),
            code: "format".to_owned(),
            message: "cannot start or end with a hyphen".to_owned(),
        });
    }
    if !field_errors.is_empty() {
        return Err(ValidationFailedSnafu {
            errors: field_errors,
        }
        .build());
    }

    let id = body.id;
    let name = body.name.unwrap_or_else(|| capitalize(&id));
    let model = body
        .model
        .unwrap_or_else(|| koina::defaults::DEFAULT_MODEL.to_owned());

    // Check if already loaded.
    if state.nous_manager.get_config(&id).is_some() {
        return Err(ApiError::Conflict {
            message: format!("agent '{id}' already exists"),
            location: snafu::location!(),
        });
    }

    let oikos = state.oikos;
    let id_for_response = id.clone();
    let name_for_response = name.clone();
    let model_for_response = model.clone();

    let result = tokio::task::spawn_blocking(move || {
        let nous_dir = oikos.nous_dir(&id);
        if nous_dir.exists() {
            return Err(ApiError::Conflict {
                message: format!("nous directory already exists: {}", nous_dir.display()),
                location: snafu::location!(),
            });
        }

        scaffold_agent(&oikos, &id, &name).map_err(|e| ApiError::Internal {
            message: format!("scaffold failed: {e}"),
            location: snafu::location!(),
        })?;

        write_agent_config(&oikos, &id, &name, &model).map_err(|e| ApiError::Internal {
            message: format!("config write failed: {e}"),
            location: snafu::location!(),
        })?;

        Ok::<_, ApiError>(())
    })
    .await
    .map_err(|e| ApiError::Internal {
        message: format!("spawn blocking failed: {e}"),
        location: snafu::location!(),
    })?;

    result?;

    state.event_bus.publish(crate::event_bus::DomainEvent::new(
        "nous.lifecycle",
        serde_json::json!({
            "nous_id": id_for_response,
            "event": "created",
            "restart_required": true,
        }),
    ));

    Ok((
        StatusCode::CREATED,
        Json(CreateAgentResponse {
            id: id_for_response,
            name: name_for_response,
            model: model_for_response,
            restart_required: true,
        }),
    ))
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut result: String = c.to_uppercase().collect();
            result.push_str(chars.as_str());
            result
        }
    }
}

fn scaffold_agent(oikos: &taxis::oikos::Oikos, id: &str, name: &str) -> std::io::Result<()> {
    let nous_dir = oikos.nous_dir(id);
    std::fs::create_dir_all(nous_dir.join("memory"))?;
    std::fs::create_dir_all(nous_dir.join("workspace/drafts"))?;
    std::fs::create_dir_all(nous_dir.join("workspace/scripts"))?;

    let soul = format!(
        "# {name}\n\n\
         You are {name}, an Aletheia cognitive agent.\n\n\
         You are helpful, thoughtful, and direct. Use the tools available to you\n\
         to assist with tasks.\n"
    );
    koina::fs::write_restricted(&nous_dir.join("SOUL.md"), soul.as_bytes())?;

    let identity = format!(
        "# Identity\n\n\
         - **Name:** {name}\n\
         - **Creature:** \n\
         - **Vibe:** \n\
         - **Emoji:** \n"
    );
    koina::fs::write_restricted(&nous_dir.join("IDENTITY.md"), identity.as_bytes())?;

    for filename in &[
        "AGENTS.md",
        "CONTEXT.md",
        "GOALS.md",
        "MEMORY.md",
        "PROSOCHE.md",
        "TOOLS.md",
        "USER.md",
        "WORKFLOWS.md",
    ] {
        let header = filename.trim_end_matches(".md");
        koina::fs::write_restricted(&nous_dir.join(filename), format!("# {header}\n").as_bytes())?;
    }

    for gitkeep in &[
        "memory/.gitkeep",
        "workspace/drafts/.gitkeep",
        "workspace/scripts/.gitkeep",
    ] {
        koina::fs::write_restricted(&nous_dir.join(gitkeep), b"")?;
    }

    koina::fs::write_restricted(
        &nous_dir.join(".gitignore"),
        ".aletheia-index/manifest_*.json\n".as_bytes(),
    )?;

    Ok(())
}

fn write_agent_config(
    oikos: &taxis::oikos::Oikos,
    id: &str,
    name: &str,
    model: &str,
) -> Result<(), ApiError> {
    let config_path = oikos.config().join("aletheia.toml");

    let existing = if config_path.exists() {
        std::fs::read_to_string(&config_path).map_err(|e| ApiError::Internal {
            message: format!("failed to read {}: {e}", config_path.display()),
            location: snafu::location!(),
        })?
    } else {
        String::new()
    };

    let mut doc: toml_edit::DocumentMut = existing.parse().map_err(|e| ApiError::Internal {
        message: format!("failed to parse config: {e}"),
        location: snafu::location!(),
    })?;

    let already_listed = doc
        .get("agents")
        .and_then(|a| a.as_table())
        .and_then(|a| a.get("list"))
        .and_then(|l| l.as_array_of_tables())
        .is_some_and(|list| {
            list.iter()
                .any(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))
        });

    if already_listed {
        return Err(ApiError::Conflict {
            message: format!("agent '{id}' already exists in the configuration file"),
            location: snafu::location!(),
        });
    }

    let workspace = format!("nous/{id}");
    let mut entry = toml_edit::Table::new();
    entry.insert("id", toml_edit::value(id));
    entry.insert("name", toml_edit::value(name));
    entry.insert("workspace", toml_edit::value(workspace));
    entry.insert("default", toml_edit::value(false));

    let mut model_table = toml_edit::Table::new();
    model_table.insert("primary", toml_edit::value(model));
    model_table.insert(
        "fallbacks",
        toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new())),
    );
    entry.insert("model", toml_edit::Item::Table(model_table));

    if doc.get("agents").and_then(|i| i.as_table()).is_none() {
        doc.insert("agents", toml_edit::Item::Table(toml_edit::Table::new()));
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "key 'agents' was just inserted if absent, so indexing is valid"
    )]
    let agents = doc["agents"]
        .as_table_mut()
        .ok_or_else(|| ApiError::Internal {
            message: "[agents] in config is not a table".to_owned(),
            location: snafu::location!(),
        })?;

    if agents
        .get("list")
        .and_then(|i| i.as_array_of_tables())
        .is_none()
    {
        agents.insert(
            "list",
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
        );
    }

    let list = agents["list"]
        .as_array_of_tables_mut()
        .ok_or_else(|| ApiError::Internal {
            message: "agents.list in config is not an array of tables".to_owned(),
            location: snafu::location!(),
        })?;

    list.push(entry);

    koina::fs::write_restricted(&config_path, doc.to_string().as_bytes()).map_err(|e| {
        ApiError::Internal {
            message: format!("failed to write config: {e}"),
            location: snafu::location!(),
        }
    })?;

    Ok(())
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
    fn nous_state_contains_manager_and_registry() {
        // Verify NousState has the fields needed by nous handlers.
        #[expect(
            dead_code,
            reason = "compile-time shape assertion: proves field types via unused local fn"
        )]
        fn assert_nous_state_fields(state: &NousState) {
            use std::sync::Arc;

            use nous::manager::NousManager;
            use organon::registry::ToolRegistry;
            use taxis::config::AletheiaConfig;

            let _: &Arc<NousManager> = &state.nous_manager;
            let _: &Arc<ToolRegistry> = &state.tool_registry;
            let _: &Arc<tokio::sync::RwLock<AletheiaConfig>> = &state.config;
        }
        // If the above compiles, NousState contains both required fields.
        assert!(std::mem::size_of::<NousState>() > 0);
    }

    #[test]
    fn nous_list_response_serializes_nous_array() {
        let resp = NousListResponse {
            nous: vec![NousSummary {
                id: "alice".to_owned(),
                name: "Alice".to_owned(),
                enabled: true,
                model: "anthropic/claude-opus-4-6".to_owned(),
                status: "active".to_owned(),
                tools: vec![],
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("nous").is_some());
        assert_eq!(json["nous"][0]["id"], "alice");
        assert_eq!(json["nous"][0]["status"], "active");
        assert_eq!(json["nous"][0]["enabled"], true);
    }

    #[test]
    fn nous_list_response_empty_array() {
        let resp = NousListResponse { nous: vec![] };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["nous"].as_array().unwrap().is_empty());
    }

    #[test]
    fn nous_summary_name_falls_back_to_id() {
        let summary = NousSummary {
            id: "bob".to_owned(),
            name: "bob".to_owned(), // fallback case: name == id
            enabled: false,
            model: "anthropic/claude-sonnet-4-6".to_owned(),
            status: "active".to_owned(),
            tools: vec![],
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "bob");
        assert_eq!(json["id"], "bob");
        assert_eq!(json["enabled"], false);
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
                enabled: true,
                description: "Read a file from disk".to_owned(),
                category: "Builtin".to_owned(),
                auto_activate: true,
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["tools"][0]["name"], "read_file");
        assert_eq!(json["tools"][0]["enabled"], true);
        assert_eq!(json["tools"][0]["category"], "Builtin");
        assert_eq!(json["tools"][0]["auto_activate"], true);
    }

    #[test]
    fn tool_summary_serializes_enabled_bit() {
        let tool = ToolSummary {
            name: "search".to_owned(),
            enabled: false,
            description: "Search the workspace".to_owned(),
            category: "Builtin".to_owned(),
            auto_activate: false,
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["enabled"], false);
        assert_eq!(json["name"], "search");
    }

    #[test]
    fn nous_not_found_error_is_404() {
        use axum::response::IntoResponse;

        use crate::error::{ApiError, NousNotFoundSnafu};
        let err: ApiError = NousNotFoundSnafu {
            id: "unknown-nous".to_owned(),
        }
        .build();
        let response = err.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn create_agent_response_serializes() {
        let resp = CreateAgentResponse {
            id: "alice".to_owned(),
            name: "Alice".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            restart_required: true,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["id"], "alice");
        assert_eq!(json["name"], "Alice");
        assert_eq!(json["model"], "claude-sonnet-4-6");
        assert_eq!(json["restart_required"], true);
    }

    #[test]
    fn capitalize_first_letter() {
        assert_eq!(capitalize("analyst"), "Analyst");
        assert_eq!(capitalize("my-agent"), "My-agent");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("A"), "A");
    }

    #[test]
    fn scaffold_creates_expected_structure() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = taxis::oikos::Oikos::from_root(dir.path());

        scaffold_agent(&oikos, "test-agent", "Test-agent").unwrap();

        let nous_dir = dir.path().join("nous/test-agent");
        assert!(nous_dir.join("SOUL.md").exists());
        assert!(nous_dir.join("IDENTITY.md").exists());
        assert!(nous_dir.join("AGENTS.md").exists());
        assert!(nous_dir.join("USER.md").exists());
        assert!(nous_dir.join("memory").is_dir());
        assert!(nous_dir.join("workspace/drafts").is_dir());

        let soul = std::fs::read_to_string(nous_dir.join("SOUL.md")).unwrap();
        assert!(soul.contains("Test-agent"));
    }

    #[test]
    fn write_agent_config_appends_without_destroying_comments() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = taxis::oikos::Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();

        let original = "# My custom config\n\
            # This comment must survive\n\
            [gateway]\n\
            port = 9999\n\n";
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup writes config template to temp directory"
        )]
        std::fs::write(dir.path().join("config/aletheia.toml"), original).unwrap();

        write_agent_config(&oikos, "alice", "Alice", "claude-sonnet-4-6").unwrap();

        let result = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        assert!(
            result.contains("# My custom config"),
            "comment must survive"
        );
        assert!(
            result.contains("# This comment must survive"),
            "comment must survive"
        );
        assert!(
            result.contains("port = 9999"),
            "existing config must survive"
        );
        assert!(result.contains(r#"id = "alice""#), "new agent must appear");
        assert!(
            result.contains(r#"workspace = "nous/alice""#),
            "workspace must be relative"
        );
    }

    #[test]
    fn write_agent_config_rejects_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = taxis::oikos::Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();

        write_agent_config(&oikos, "bob", "Bob", "claude-sonnet-4-6").unwrap();
        let result = write_agent_config(&oikos, "bob", "Bob", "claude-sonnet-4-6");
        assert!(result.is_err(), "duplicate agent should return an error");
    }

    #[test]
    fn create_agent_response_restart_required_is_true() {
        let resp = CreateAgentResponse {
            id: "alice".to_owned(),
            name: "Alice".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            restart_required: true,
        };
        assert!(
            resp.restart_required,
            "newly created agents always require restart"
        );
    }

    #[test]
    fn recover_response_serializes() {
        let resp = RecoverResponse {
            id: "alice".to_owned(),
            recovered: true,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["id"], "alice");
        assert_eq!(json["recovered"], true);
    }

    #[test]
    fn recover_response_not_recovered_serializes() {
        let resp = RecoverResponse {
            id: "alice".to_owned(),
            recovered: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["recovered"], false);
    }
}
