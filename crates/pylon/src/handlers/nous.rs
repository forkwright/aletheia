//! Nous (agent) information endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use symbolon::types::Role;

use crate::error::{ApiError, ErrorResponse, FieldError, NousNotFoundSnafu, ValidationFailedSnafu};
use crate::extract::{Claims, require_nous_access, require_role};
use crate::state::NousState;

#[path = "nous_dto.rs"]
mod nous_dto;
pub use nous_dto::{
    AgentDefinition, CreateAgentResponse, NousListResponse, NousStatus, NousSummary,
    RecoverResponse, ToolSummary, ToolsResponse,
};

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
        .map(|c| NousSummary {
            id: c.id.to_string(),
            name: c.name.clone().unwrap_or_else(|| c.id.to_string()),
            model: c.generation.model.clone(),
            status: "active".to_owned(),
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
/// Returns only the tools that the requesting agent is permitted to use,
/// filtered by the agent's `tool_allowlist` from its `NousConfig`. When no
/// allowlist is configured, all registered tools are returned (#3229).
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
    let config = state
        .nous_manager
        .get_config(&id)
        .ok_or_else(|| NousNotFoundSnafu { id }.build())?;

    let defs = state.tool_registry.definitions();

    // WHY: Without filtering, the API returns the global tool registry
    // regardless of which agent is requesting. Agents with a `tool_allowlist`
    // (e.g. role-restricted sub-agents) should only see their permitted tools.
    // This matches the execution-layer enforcement in `execute/mod.rs` (#3229).
    let tools = defs
        .into_iter()
        .filter(|d| {
            config
                .tool_allowlist
                .as_ref()
                .is_none_or(|allowlist| allowlist.iter().any(|a| a == d.name.as_str()))
        })
        .map(|d| ToolSummary {
            name: d.name.as_str().to_owned(),
            description: d.description.clone(),
            category: format!("{:?}", d.category),
            auto_activate: d.auto_activate,
        })
        .collect();

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

    let recovered = handle.recover().await.map_err(|e| {
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

            let _: &Arc<NousManager> = &state.nous_manager;
            let _: &Arc<ToolRegistry> = &state.tool_registry;
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
                auto_activate: true,
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["tools"][0]["name"], "read_file");
        assert_eq!(json["tools"][0]["category"], "Builtin");
        assert_eq!(json["tools"][0]["auto_activate"], true);
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
}
