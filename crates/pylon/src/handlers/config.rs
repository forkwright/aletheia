//! Configuration read/write endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::Value;
use tracing::instrument;
use utoipa::ToSchema;

use crate::error::ApiError;
use crate::extract::Claims;
use crate::state::AppState;

const VALID_SECTIONS: &[&str] = &[
    "agents",
    "gateway",
    "channels",
    "bindings",
    "embedding",
    "data",
    "packs",
    "maintenance",
    "pricing",
];

/// Response wrapper for config reload metadata.
#[derive(serde::Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdateResponse {
    pub section: String,
    pub config: Value,
    pub restart_required: Vec<String>,
}

/// GET /api/v1/config — full redacted config.
#[utoipa::path(
    get,
    path = "/api/v1/config",
    responses(
        (status = 200, description = "Redacted runtime config"),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims))]
pub async fn get_config(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
) -> Result<Json<Value>, ApiError> {
    let config = state.config.read().await;
    let redacted = aletheia_taxis::redact::redact(&config);
    Ok(Json(redacted))
}

/// GET /api/v1/config/{section} — redacted config section.
#[utoipa::path(
    get,
    path = "/api/v1/config/{section}",
    params(("section" = String, Path, description = "Config section name")),
    responses(
        (status = 200, description = "Redacted config section"),
        (status = 404, description = "Unknown section"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims))]
pub async fn get_section(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(section): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !VALID_SECTIONS.contains(&section.as_str()) {
        return Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::Location::default(),
        });
    }

    let config = state.config.read().await;
    let redacted = aletheia_taxis::redact::redact(&config);

    match redacted.get(&section) {
        Some(val) => Ok(Json(val.clone())),
        None => Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::Location::default(),
        }),
    }
}

/// PUT /api/v1/config/{section} — update and persist a config section.
#[utoipa::path(
    put,
    path = "/api/v1/config/{section}",
    params(("section" = String, Path, description = "Config section name")),
    request_body = Value,
    responses(
        (status = 200, description = "Updated config section", body = ConfigUpdateResponse),
        (status = 404, description = "Unknown section"),
        (status = 422, description = "Validation failed"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims, body))]
pub async fn update_section(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(section): Path<String>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    if !VALID_SECTIONS.contains(&section.as_str()) {
        return Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::Location::default(),
        });
    }

    // Validate
    if let Err(err) = aletheia_taxis::validate::validate_section(&section, &body) {
        return Err(ApiError::ValidationFailed {
            errors: err.errors,
            location: snafu::Location::default(),
        });
    }

    // Deep-merge into current config (body patches only changed fields)
    let mut config = state.config.write().await;
    let mut config_value = serde_json::to_value(&*config).map_err(|e| ApiError::Internal {
        message: format!("failed to serialize config: {e}"),
        location: snafu::Location::default(),
    })?;

    if let Value::Object(root) = &mut config_value {
        let existing = root
            .entry(section.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::default()));
        deep_merge(existing, body);
    }

    // Deserialize back to verify structural validity
    let new_config: aletheia_taxis::config::AletheiaConfig =
        serde_json::from_value(config_value).map_err(|e| ApiError::ValidationFailed {
            errors: vec![format!("invalid config structure: {e}")],
            location: snafu::Location::default(),
        })?;

    // Persist to disk
    aletheia_taxis::loader::write_config(&state.oikos, &new_config).map_err(|e| {
        ApiError::Internal {
            message: format!("failed to write config: {e}"),
            location: snafu::Location::default(),
        }
    })?;

    // Identify restart-required fields
    let restart_required: Vec<String> = aletheia_taxis::reload::restart_prefixes()
        .iter()
        .filter(|p| p.starts_with(&format!("{section}.")))
        .map(|p| (*p).to_owned())
        .collect();

    // Update in-memory config
    *config = new_config;
    let redacted = aletheia_taxis::redact::redact(&config);
    let section_value = redacted.get(&section).cloned().unwrap_or_else(|| {
        tracing::debug!(section = %section, "config section absent after update, returning null");
        Value::Null
    });

    Ok((
        StatusCode::OK,
        Json(ConfigUpdateResponse {
            section,
            config: section_value,
            restart_required,
        }),
    ))
}

/// Recursively merge `patch` into `base`. Patch values override base values;
/// nested objects are merged rather than replaced.
pub(crate) fn deep_merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (key, patch_val) in patch_map {
                let entry = base_map.entry(key).or_insert(Value::Null);
                deep_merge(entry, patch_val);
            }
        }
        (base, patch) => {
            *base = patch;
        }
    }
}
