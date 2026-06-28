// kanon:ignore RUST/file-too-long — nous handler covers agent CRUD, tool management, and recovery; tracked for split in #4201
//! Nous (agent) information endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use nous::config::NousConfig;
use nous::cross::AddressMask;
use symbolon::types::Role;
use taxis::config::{AletheiaConfig, NousDefinition};

use crate::error::{ApiError, ErrorResponse, FieldError, NousNotFoundSnafu, ValidationFailedSnafu};
use crate::extract::{Claims, require_nous_access, require_role};
use crate::handlers::providers::resolve_model_readiness;
use crate::state::NousState;

#[path = "nous_dto.rs"]
mod nous_dto;
pub use nous_dto::{
    AddressMaskStatus, AgentDefinition, CreateAgentResponse, NousListResponse, NousStatus,
    NousSummary, NousToggleRequest, RecoverResponse, ToolSummary, ToolToggleRequest, ToolsResponse,
};

fn agent_definition<'a>(config: &'a AletheiaConfig, id: &str) -> Option<&'a NousDefinition> {
    config.agents.list.iter().find(|agent| agent.id == id)
}

fn ensure_agent_definition<'a>(
    config: &'a mut AletheiaConfig,
    runtime: &NousConfig,
) -> Result<&'a mut NousDefinition, ApiError> {
    let idx = config
        .agents
        .list
        .iter()
        .position(|agent| agent.id == runtime.id.as_ref());

    if idx.is_none() {
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
    }

    let idx = idx.unwrap_or(config.agents.list.len() - 1);
    config
        .agents
        .list
        .get_mut(idx)
        .ok_or_else(|| ApiError::Internal {
            message: "agent list was empty after push".to_owned(),
            location: snafu::location!(),
        })
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

fn address_mask_status(mask: &AddressMask) -> AddressMaskStatus {
    AddressMaskStatus {
        kind: mask.diagnostic_kind().to_owned(),
        allowed_senders: mask.allowed_senders().to_vec(),
    }
}

fn nous_visible_to_claims(claims: &Claims, config: &NousConfig) -> bool {
    let in_scope = claims
        .nous_id
        .as_deref()
        .is_none_or(|scoped| scoped == config.id.as_ref());
    let private_visible = !config.private || claims.role >= Role::Operator;
    in_scope && private_visible
}

fn require_visible_nous(claims: &Claims, config: &NousConfig) -> Result<(), ApiError> {
    if nous_visible_to_claims(claims, config) {
        Ok(())
    } else {
        Err(ApiError::forbidden("access denied for this agent"))
    }
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
    let config = state.config.read().await;
    let nous: Vec<NousSummary> = state
        .nous_manager
        .configs()
        .into_iter()
        .filter(|c| nous_visible_to_claims(&claims, c))
        .map(|c| {
            let agent_id = c.id.as_ref();
            let enabled = agent_definition(&config, agent_id).is_none_or(|agent| agent.enabled);
            let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, agent_id));
            let mut models = vec![c.generation.model.clone()];
            models.extend(c.generation.fallback_models.clone());
            NousSummary {
                id: c.id.to_string(),
                name: c.name.clone().unwrap_or_else(|| c.id.to_string()),
                enabled,
                model: c.generation.model.clone(),
                fallback_models: c.generation.fallback_models.clone(),
                provider_readiness: resolve_model_readiness(&state.provider_registry, &models),
                status: "active".to_owned(),
                tools,
                config_applied: None,
                live_applied: None,
                reload_required: None,
                restart_required: None,
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
    let config = state
        .nous_manager
        .get_config(&id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?;
    require_visible_nous(&claims, config)?;

    let (
        status,
        background_failure_total_count,
        background_failure_recent_count,
        background_failure_latest_message,
        background_failure_latest_kind,
        background_health_degraded,
    ) = match state.nous_manager.get(&id) {
        Some(handle) => match handle.status().await {
            Ok(s) => (
                s.lifecycle.to_string(),
                s.background_failure_total_count,
                s.background_failure_recent_count,
                s.background_failure_latest_message,
                s.background_failure_latest_kind,
                s.background_health_degraded,
            ),
            Err(_) => ("unknown".to_owned(), 0, 0, None, None, false),
        },
        None => ("unknown".to_owned(), 0, 0, None, None, false),
    };

    let mut status_models = vec![config.generation.model.clone()];
    status_models.extend(config.generation.fallback_models.clone());
    let address_mask = if let Some(router) = state.nous_manager.router() {
        router.address_mask(&id).await
    } else {
        AddressMask::Public
    };

    Ok(Json(NousStatus {
        id: config.id.to_string(),
        model: config.generation.model.clone(),
        fallback_models: config.generation.fallback_models.clone(),
        retries_before_fallback: config.generation.retries_before_fallback,
        complexity_routing_enabled: config.generation.complexity.enabled,
        complexity_no_llm_threshold: config.generation.complexity.no_llm_threshold,
        complexity_low_threshold: config.generation.complexity.low_threshold,
        complexity_high_threshold: config.generation.complexity.high_threshold,
        provider_readiness: resolve_model_readiness(&state.provider_registry, &status_models),
        context_window: config.generation.context_window,
        max_output_tokens: config.generation.max_output_tokens,
        thinking_enabled: config.generation.thinking_enabled,
        thinking_budget: config.generation.thinking_budget,
        max_tool_iterations: config.limits.max_tool_iterations,
        status,
        background_failure_total_count,
        background_failure_recent_count,
        background_failure_latest_message,
        background_failure_latest_kind,
        background_health_degraded,
        address_mask: address_mask_status(&address_mask),
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
    let runtime = state
        .nous_manager
        .get_config(&id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?;
    require_visible_nous(&claims, runtime)?;

    let config = state.config.read().await;
    let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, &id));

    Ok(Json(ToolsResponse {
        tools,
        config_applied: None,
        live_applied: None,
        reload_required: None,
        restart_required: None,
    }))
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

    // WHY: Stage the toggle on a cloned config, persist it, then swap the
    // live config. This keeps runtime state and disk consistent if the write
    // fails (see #4582).
    let staged = {
        let config = state.config.read().await;
        let mut staged = config.clone();
        drop(config);

        let agent = ensure_agent_definition(&mut staged, &runtime)?;
        agent.enabled = body.enabled;
        staged
    };

    taxis::loader::write_config(&state.oikos, &staged).map_err(|e| ApiError::Internal {
        message: format!("failed to write config: {e}"),
        location: snafu::location!(),
    })?;

    {
        let mut config = state.config.write().await;
        *config = staged;
        if let Err(e) = state.config_tx.send(config.clone()) {
            tracing::warn!(error = %e, "config broadcast has no receivers");
        }
    }

    let live_handle = state.nous_manager.get(&id);
    let mut live_command_failed = live_handle.is_none();
    if let Some(handle) = live_handle.as_ref() {
        let live_result = if body.enabled {
            handle.wake().await
        } else {
            handle.sleep().await
        };
        if let Err(e) = live_result {
            live_command_failed = true;
            tracing::warn!(nous_id = %id, error = %e, "failed to update live agent lifecycle; config intent persisted");
        }
    }

    let config = state.config.read().await;
    let enabled = agent_definition(&config, &id).is_none_or(|agent| agent.enabled);
    let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, &id));
    drop(config);
    let status = match live_handle.as_ref() {
        Some(handle) => handle
            .status()
            .await
            .map_or_else(|_| "unknown".to_owned(), |s| s.lifecycle.to_string()),
        None => "unknown".to_owned(),
    };
    let live_applied = if body.enabled {
        matches!(status.as_str(), "idle" | "active")
    } else {
        status == "dormant"
    };
    let restart_required =
        !live_applied && (live_command_failed || matches!(status.as_str(), "unknown" | "degraded"));

    let mut summary_models = vec![runtime.generation.model.clone()];
    summary_models.extend(runtime.generation.fallback_models.clone());

    Ok(Json(NousSummary {
        id: runtime.id.to_string(),
        name: runtime
            .name
            .clone()
            .unwrap_or_else(|| runtime.id.to_string()),
        enabled,
        model: runtime.generation.model.clone(),
        fallback_models: runtime.generation.fallback_models.clone(),
        provider_readiness: resolve_model_readiness(&state.provider_registry, &summary_models),
        status,
        tools,
        config_applied: Some(true),
        live_applied: Some(live_applied),
        reload_required: Some(false),
        restart_required: Some(restart_required),
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

    // WHY: Stage the allowlist change on a cloned config, persist it, then
    // swap the live config. A failed write must not alter live tool policy
    // (see #4582).
    let staged = {
        let config = state.config.read().await;
        let mut staged = config.clone();
        drop(config);

        let agent = ensure_agent_definition(&mut staged, &runtime)?;
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

        staged
    };

    taxis::loader::write_config(&state.oikos, &staged).map_err(|e| ApiError::Internal {
        message: format!("failed to write config: {e}"),
        location: snafu::location!(),
    })?;

    {
        let mut config = state.config.write().await;
        *config = staged;
        if let Err(e) = state.config_tx.send(config.clone()) {
            tracing::warn!(error = %e, "config broadcast has no receivers");
        }
    }

    let config = state.config.read().await;
    let tools = tool_summaries_for_agent(&state, allowlist_for_agent(&config, &id));

    Ok(Json(ToolsResponse {
        tools,
        config_applied: Some(true),
        live_applied: Some(false),
        reload_required: Some(true),
        restart_required: Some(false),
    }))
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
) -> Result<(StatusCode, Json<CreateAgentResponse>), ApiError> {
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

    state
        .event_bus
        .publish(crate::event_bus::DomainEvent::new(
            state.event_bus.next_id(),
            "nous.lifecycle",
            serde_json::json!({
                "nous_id": id_for_response,
                "event": "created",
                "restart_required": true,
            }),
        ))
        .await;

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
#[path = "nous_tests.rs"]
mod tests;
