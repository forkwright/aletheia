//! Configuration read/write endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::Value;
use tracing::instrument;
use utoipa::ToSchema;

use aletheia_symbolon::types::Role;

use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::ConfigState;

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

/// Response wrapper for config section update metadata.
#[derive(serde::Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdateResponse {
    /// Name of the config section that was updated.
    pub section: String,
    /// The updated config section value.
    pub config: Value,
    /// Field paths that require a restart to take effect.
    pub restart_required: Vec<String>,
}

/// Response wrapper for full config reload.
#[derive(serde::Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReloadResponse {
    /// Number of hot-reloadable values that were updated.
    pub hot_reloaded: usize,
    /// Field paths that changed but require a restart to take effect.
    pub restart_required: Vec<String>,
    /// All changed field paths (both hot and cold).
    pub changed: Vec<String>,
}

/// GET /api/v1/config: full redacted config.
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
    State(state): State<ConfigState>,
    _claims: Claims,
) -> Result<Json<Value>, ApiError> {
    let config = state.config.read().await;
    let redacted = aletheia_taxis::redact::redact(&config);
    Ok(Json(redacted))
}

/// GET /api/v1/config/{section}: redacted config section.
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
    State(state): State<ConfigState>,
    _claims: Claims,
    Path(section): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !VALID_SECTIONS.contains(&section.as_str()) {
        return Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::location!(),
        });
    }

    let config = state.config.read().await;
    let redacted = aletheia_taxis::redact::redact(&config);

    match redacted.get(&section) {
        Some(val) => Ok(Json(val.clone())),
        None => Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::location!(),
        }),
    }
}

/// POST /api/v1/config/reload: re-read config from disk and apply hot-reloadable values.
///
/// Re-reads `aletheia.toml` (and env overrides), validates the result, diffs
/// against the current in-memory config, applies hot-reloadable changes, and
/// logs what changed. Cold values (port, bind, TLS, auth mode, channels) are
/// reported but not applied until restart.
#[utoipa::path(
    post,
    path = "/api/v1/config/reload",
    responses(
        (status = 200, description = "Config reloaded", body = ConfigReloadResponse),
        (status = 422, description = "New config is invalid, old config preserved"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn reload_config(
    State(state): State<ConfigState>,
    claims: Claims,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;
    let current = state.config.read().await.clone();
    let outcome = aletheia_taxis::reload::prepare_reload(&state.oikos, &current).map_err(|e| {
        tracing::error!(error = %e, "config reload failed");
        match e {
            aletheia_taxis::reload::ReloadError::Validation { source, .. } => {
                ApiError::ValidationFailed {
                    errors: source.errors,
                    location: snafu::location!(),
                }
            }
            other => ApiError::Internal {
                message: other.to_string(),
                location: snafu::location!(),
            },
        }
    })?;

    let hot_reloaded = outcome.diff.hot_changes().len();
    let restart_required: Vec<String> = outcome
        .diff
        .cold_changes()
        .iter()
        .map(|c| c.path.clone())
        .collect();
    let changed: Vec<String> = outcome
        .diff
        .changes
        .iter()
        .map(|c| c.path.clone())
        .collect();

    crate::server::apply_reload(&state, outcome).await;

    Ok((
        StatusCode::OK,
        Json(ConfigReloadResponse {
            hot_reloaded,
            restart_required,
            changed,
        }),
    ))
}

/// PUT /api/v1/config/{section}: update and persist a config section.
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
#[instrument(skip(state, claims, body))]
pub async fn update_section(
    State(state): State<ConfigState>,
    claims: Claims,
    Path(section): Path<String>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;
    if !VALID_SECTIONS.contains(&section.as_str()) {
        return Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::location!(),
        });
    }

    if let Err(err) = aletheia_taxis::validate::validate_section(&section, &body) {
        return Err(ApiError::ValidationFailed {
            errors: err.errors,
            location: snafu::location!(),
        });
    }

    let mut config = state.config.write().await;
    let mut config_value = serde_json::to_value(&*config).map_err(|e| ApiError::Internal {
        message: format!("failed to serialize config: {e}"),
        location: snafu::location!(),
    })?;

    if let Value::Object(root) = &mut config_value {
        let existing = root
            .entry(section.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::default()));
        deep_merge(existing, body);
    }

    // WHY: Deserialize back to verify structural validity.
    // Log serde details internally; the error message exposed to the client must not
    // include field paths or internal type names from the serde error. (#845)
    let new_config: aletheia_taxis::config::AletheiaConfig =
        serde_json::from_value(config_value).map_err(|e| {
            tracing::error!(error = %e, section = %section, "config deserialization failed after merge");
            ApiError::ValidationFailed {
                errors: vec!["Invalid configuration format".to_owned()],
                location: snafu::location!(),
            }
        })?;

    aletheia_taxis::loader::write_config(&state.oikos, &new_config).map_err(|e| {
        ApiError::Internal {
            message: format!("failed to write config: {e}"),
            location: snafu::location!(),
        }
    })?;

    let restart_required: Vec<String> = aletheia_taxis::reload::restart_prefixes()
        .iter()
        .filter(|p| p.starts_with(&format!("{section}.")))
        .map(|p| (*p).to_owned())
        .collect();

    *config = new_config.clone();
    let redacted = aletheia_taxis::redact::redact(&config);
    drop(config);

    let _ = state.config_tx.send(new_config);

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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after len assertions"
)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn deep_merge_replaces_leaf_value() {
        let mut base = json!({"key": "old"});
        deep_merge(&mut base, json!({"key": "new"}));
        assert_eq!(base["key"], "new");
    }

    #[test]
    fn deep_merge_merges_nested_objects_without_replacing() {
        let mut base = json!({"outer": {"a": 1, "b": 2}});
        deep_merge(&mut base, json!({"outer": {"b": 99}}));
        assert_eq!(base["outer"]["a"], 1);
        assert_eq!(base["outer"]["b"], 99);
    }

    #[test]
    fn deep_merge_adds_new_key() {
        let mut base = json!({"existing": true});
        deep_merge(&mut base, json!({"added": "value"}));
        assert_eq!(base["existing"], true);
        assert_eq!(base["added"], "value");
    }

    #[test]
    fn deep_merge_does_not_remove_unpatched_keys() {
        let mut base = json!({"keep": "me", "also": "this"});
        deep_merge(&mut base, json!({"keep": "updated"}));
        assert_eq!(base["also"], "this");
    }

    #[test]
    fn deep_merge_replaces_array_wholesale() {
        let mut base = json!({"items": [1, 2, 3]});
        deep_merge(&mut base, json!({"items": [4, 5]}));
        let items = base["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], 4);
    }

    #[test]
    fn valid_sections_includes_expected_entries() {
        assert!(VALID_SECTIONS.contains(&"agents"));
        assert!(VALID_SECTIONS.contains(&"gateway"));
        assert!(VALID_SECTIONS.contains(&"maintenance"));
        assert!(!VALID_SECTIONS.contains(&"secrets"));
    }

    #[test]
    fn config_update_response_serializes_camel_case() {
        let resp = ConfigUpdateResponse {
            section: "agents".to_owned(),
            config: json!({"list": []}),
            restart_required: vec!["agents.model".to_owned()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["section"], "agents");
        assert!(json.get("restartRequired").is_some());
        assert!(json["restartRequired"].as_array().unwrap().len() == 1);
    }
}
