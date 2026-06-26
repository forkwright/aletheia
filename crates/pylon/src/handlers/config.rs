//! Configuration read/write endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::Value;
use tracing::instrument;

use symbolon::types::Role;

use crate::error::{ApiError, FieldError};
use crate::extract::{Claims, require_role};
use crate::state::ConfigState;

const VALID_SECTIONS: &[&str] = &[
    "agents",
    "gateway",
    "channels",
    "bindings",
    "feature_flags",
    "embedding",
    "data",
    "packs",
    "maintenance",
    "pricing",
];

#[path = "config_dto.rs"]
mod config_dto;
pub use config_dto::{
    AgentsConfig, ChannelBinding, ChannelsConfig, ConfigReloadResponse, ConfigUpdateResponse,
    DataConfig, EmbeddingSettings, FeatureFlagConfig, GatewayConfig, MaintenanceConfig,
    ModelPricing,
};

pub use section_schemas::*; // kanon:ignore RUST/barrel-reexport -- WHY: section_schemas is a private submodule; the pub use glob is the intended public API surface for config endpoint types

#[expect(missing_docs, reason = "OpenAPI schema mirrors taxis config shapes")]
mod section_schemas {
    use crate::error::{ApiError, FieldError};
    use crate::handlers::config::config_dto::{
        AgentsConfig, ChannelBinding, ChannelsConfig, DataConfig, EmbeddingSettings,
        FeatureFlagConfig, GatewayConfig, MaintenanceConfig, ModelPricing,
    };
    use std::collections::HashMap;

    use axum::body::Body;
    use axum::extract::FromRequest;
    use serde_json::Value;
    use utoipa::PartialSchema;

    /// Typed payload for `PUT /api/v1/config/{section}`.
    ///
    /// The variant is determined by the `{section}` path parameter at the API
    /// boundary. Each variant wraps the raw JSON value so the underlying deep-merge
    /// semantics are preserved unchanged.
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum ConfigSectionPayload {
        Agents(Value),
        Gateway(Value),
        Channels(Value),
        Bindings(Value),
        FeatureFlags(Value),
        Embedding(Value),
        Data(Value),
        Packs(Value),
        Maintenance(Value),
        Pricing(Value),
    }

    impl utoipa::PartialSchema for ConfigSectionPayload {
        fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
            utoipa::openapi::schema::OneOfBuilder::new()
                .item(AgentsConfig::schema())
                .item(GatewayConfig::schema())
                .item(ChannelsConfig::schema())
                .item(Vec::<ChannelBinding>::schema())
                .item(Vec::<FeatureFlagConfig>::schema())
                .item(EmbeddingSettings::schema())
                .item(DataConfig::schema())
                .item(Vec::<String>::schema())
                .item(MaintenanceConfig::schema())
                .item(std::collections::HashMap::<String, ModelPricing>::schema())
                .into()
        }
    }

    impl utoipa::ToSchema for ConfigSectionPayload {
        fn schemas(
            schemas: &mut Vec<(
                String,
                utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
            )>,
        ) {
            schemas.push((AgentsConfig::name().into(), AgentsConfig::schema()));
            schemas.push((GatewayConfig::name().into(), GatewayConfig::schema()));
            schemas.push((ChannelsConfig::name().into(), ChannelsConfig::schema()));
            schemas.push((ChannelBinding::name().into(), ChannelBinding::schema()));
            schemas.push((
                FeatureFlagConfig::name().into(),
                FeatureFlagConfig::schema(),
            ));
            schemas.push((
                EmbeddingSettings::name().into(),
                EmbeddingSettings::schema(),
            ));
            schemas.push((DataConfig::name().into(), DataConfig::schema()));
            schemas.push((
                MaintenanceConfig::name().into(),
                MaintenanceConfig::schema(),
            ));
            schemas.push((ModelPricing::name().into(), ModelPricing::schema()));
        }
    }

    impl<S> FromRequest<S> for ConfigSectionPayload
    where
        S: Send + Sync,
    {
        type Rejection = ApiError;

        async fn from_request(
            req: axum::http::Request<Body>,
            _state: &S,
        ) -> Result<Self, Self::Rejection> {
            let (parts, body) = req.into_parts();

            let content_type = parts
                .headers
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok());
            if !matches!(content_type, Some(ct) if ct == "application/json" || ct.starts_with("application/json;"))
            {
                return Err(ApiError::BadRequest {
                    message: "expected Content-Type: application/json".to_owned(),
                    location: snafu::location!(),
                });
            }

            let bytes = axum::body::to_bytes(body, 10 * 1024 * 1024)
                .await
                .map_err(|e| ApiError::BadRequest {
                    message: format!("failed to read request body: {e}"),
                    location: snafu::location!(),
                })?;

            let path = parts.uri.path();
            let section = path.rsplit('/').next().unwrap_or("");

            let value: Value =
                serde_json::from_slice(&bytes).map_err(|e| ApiError::ValidationFailed {
                    errors: vec![FieldError {
                        field: "_body".to_owned(),
                        code: "format".to_owned(),
                        message: e.to_string(),
                    }],
                    location: snafu::location!(),
                })?;

            // Validate structural shape at the boundary by deserializing into the
            // section's typed config. The original Value is preserved for the merge
            // so no downstream behaviour changes.
            let result = match section {
                "agents" => serde_json::from_value::<taxis::config::AgentsConfig>(value.clone())
                    .map(|_| Self::Agents(value)),
                "gateway" => serde_json::from_value::<taxis::config::GatewayConfig>(value.clone())
                    .map(|_| Self::Gateway(value)),
                "channels" => {
                    serde_json::from_value::<taxis::config::ChannelsConfig>(value.clone())
                        .map(|_| Self::Channels(value))
                }
                "bindings" => {
                    serde_json::from_value::<Vec<taxis::config::ChannelBinding>>(value.clone())
                        .map(|_| Self::Bindings(value))
                }
                "feature_flags" => {
                    serde_json::from_value::<Vec<taxis::config::FeatureFlagConfig>>(value.clone())
                        .map(|_| Self::FeatureFlags(value))
                }
                "embedding" => {
                    serde_json::from_value::<taxis::config::EmbeddingSettings>(value.clone())
                        .map(|_| Self::Embedding(value))
                }
                "data" => Ok(Self::Data(value)),
                "packs" => serde_json::from_value::<Vec<std::path::PathBuf>>(value.clone())
                    .map(|_| Self::Packs(value)),
                "maintenance" => {
                    serde_json::from_value::<taxis::config::MaintenanceSettings>(value.clone())
                        .map(|_| Self::Maintenance(value))
                }
                "pricing" => {
                    serde_json::from_value::<HashMap<String, taxis::config::ModelPricing>>(
                        value.clone(),
                    )
                    .map(|_| Self::Pricing(value))
                }
                _ => {
                    return Err(ApiError::NotFound {
                        path: path.to_owned(),
                        location: snafu::location!(),
                    });
                }
            };

            result.map_err(|e| ApiError::ValidationFailed {
                errors: vec![FieldError {
                    field: "_body".to_owned(),
                    code: "format".to_owned(),
                    message: e.to_string(),
                }],
                location: snafu::location!(),
            })
        }
    }
}

/// GET /api/v1/config: full redacted config.
#[utoipa::path(
    get,
    path = "/api/v1/config",
    responses(
        (status = 200, description = "Redacted runtime config"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn get_config(
    State(state): State<ConfigState>,
    claims: Claims,
) -> Result<Json<Value>, ApiError> {
    // WHY(#5158): full config reads expose the same sensitive surface as
    // config mutations and reload; gate them at the Operator boundary.
    require_role(&claims, Role::Operator)?;
    let config = state.config.read().await;
    let redacted = taxis::redact::redact(&config);
    Ok(Json(redacted))
}

/// GET /api/v1/config/{section}: redacted config section.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/config/{section}",
    params(("section" = String, Path, description = "Config section name")),
    responses(
        (status = 200, description = "Redacted config section"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Unknown section"),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn get_section(
    State(state): State<ConfigState>,
    claims: Claims,
    Path(section): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // WHY(#5158): section reads are a subset of full config reads and must
    // share the Operator boundary so secret-containing sections cannot leak.
    require_role(&claims, Role::Operator)?;
    if !VALID_SECTIONS.contains(&section.as_str()) {
        return Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::location!(),
        });
    }

    let config = state.config.read().await;
    let redacted = taxis::redact::redact(&config);

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
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
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
    let outcome = taxis::reload::prepare_reload(&state.oikos, &current).map_err(|e| {
        tracing::error!(error = %e, "config reload failed");
        match e {
            taxis::reload::ReloadError::Validation { source, .. } => ApiError::ValidationFailed {
                errors: source
                    .errors
                    .into_iter()
                    .map(|msg| FieldError {
                        field: "_config".to_owned(),
                        code: "invalid".to_owned(),
                        message: msg,
                    })
                    .collect(),
                location: snafu::location!(),
            },
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

/// Run both structural and auth-policy validation for an incoming
/// config PUT body.
///
/// WHY(#3383, #4240): `validate_section` is structural-only — it accepts
/// `auth.mode = "none"` so file-load callers (`check-config`, server
/// startup) do not falsely reject configs operators write directly. The
/// API-level opt-in gate against silently disabling auth lives here and
/// fires *only* on the config PUT path. Keeping this split in a helper
/// also keeps `update_section` under the clippy too-many-lines limit.
fn validate_section_for_put(section: &str, body: &Value) -> Result<(), ApiError> {
    if let Err(err) = taxis::validate::validate_section(section, body) {
        return Err(ApiError::ValidationFailed {
            errors: err
                .errors
                .into_iter()
                .map(|msg| FieldError {
                    field: section.to_owned(),
                    code: "invalid".to_owned(),
                    message: msg,
                })
                .collect(),
            location: snafu::location!(),
        });
    }
    if section == "gateway"
        && let Err(err) = taxis::validate::validate_auth_mode_policy(body)
    {
        return Err(ApiError::ValidationFailed {
            errors: err
                .errors
                .into_iter()
                .map(|msg| FieldError {
                    field: section.to_owned(),
                    code: "auth_mode_none_requires_opt_in".to_owned(),
                    message: msg,
                })
                .collect(),
            location: snafu::location!(),
        });
    }
    Ok(())
}

/// Validate the fully merged config using the same section validators that
/// reload and startup use.
///
/// WHY(#4583): A partial PUT can make the merged config semantically invalid
/// even when the submitted section passes structural validation on its own.
/// Running the full validator before persisting keeps the API and disk states
/// consistent with what a reload would accept.
fn validate_full_config(config: &taxis::config::AletheiaConfig) -> Result<(), ApiError> {
    let value = serde_json::to_value(config).map_err(|e| ApiError::Internal {
        message: format!("failed to serialize config for validation: {e}"),
        location: snafu::location!(),
    })?;
    let Value::Object(sections) = value else {
        return Err(ApiError::Internal {
            message: "config did not serialize to a JSON object".to_owned(),
            location: snafu::location!(),
        });
    };

    let mut all_errors = Vec::new();
    for (section, val) in sections {
        if let Err(err) = taxis::validate::validate_section(&section, &val) {
            for msg in err.errors {
                all_errors.push(FieldError {
                    field: section.clone(),
                    code: "invalid".to_owned(),
                    message: msg,
                });
            }
        }
    }

    if all_errors.is_empty() {
        Ok(())
    } else {
        Err(ApiError::ValidationFailed {
            errors: all_errors,
            location: snafu::location!(),
        })
    }
}

/// PUT /api/v1/config/{section}: update and persist a config section.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    put,
    path = "/api/v1/config/{section}",
    params(("section" = String, Path, description = "Config section name")),
    request_body = ConfigSectionPayload,
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
    body: ConfigSectionPayload,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;
    if !VALID_SECTIONS.contains(&section.as_str()) {
        return Err(ApiError::NotFound {
            path: format!("/api/v1/config/{section}"),
            location: snafu::location!(),
        });
    }

    let body_value = match body {
        ConfigSectionPayload::Agents(v)
        | ConfigSectionPayload::Gateway(v)
        | ConfigSectionPayload::Channels(v)
        | ConfigSectionPayload::Bindings(v)
        | ConfigSectionPayload::FeatureFlags(v)
        | ConfigSectionPayload::Embedding(v)
        | ConfigSectionPayload::Data(v)
        | ConfigSectionPayload::Packs(v)
        | ConfigSectionPayload::Maintenance(v)
        | ConfigSectionPayload::Pricing(v) => v,
    };

    validate_section_for_put(&section, &body_value)?;

    let mut config = state.config.write().await;
    let mut config_value = serde_json::to_value(&*config).map_err(|e| ApiError::Internal {
        message: format!("failed to serialize config: {e}"),
        location: snafu::location!(),
    })?;

    if let Value::Object(root) = &mut config_value {
        let existing = root
            .entry(section.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::default()));
        deep_merge(existing, body_value);
    }
    taxis::redact::preserve_secret_leaves(&mut config_value, &config);

    // WHY: Deserialize back to verify structural validity.
    // Log serde details internally; the error message exposed to the client must not
    // include field paths or internal type names from the serde error. (#845)
    let new_config: taxis::config::AletheiaConfig =
        serde_json::from_value(config_value).map_err(|e| {
            tracing::error!(error = %e, section = %section, "config deserialization failed after merge");
            ApiError::ValidationFailed {
                errors: vec![FieldError {
                    field: section.clone(),
                    code: "format".to_owned(),
                    message: "Invalid configuration format".to_owned(),
                }],
                location: snafu::location!(),
            }
        })?;

    // WHY(#4583): Run the same semantic validation used by reload against the
    // fully merged candidate before any write or live swap can occur.
    validate_full_config(&new_config)?;

    let diff = taxis::reload::diff_configs(&config, &new_config).map_err(|e| {
        tracing::error!(error = %e, section = %section, "config diff failed after merge");
        ApiError::Internal {
            message: format!("failed to diff configs: {e}"),
            location: snafu::location!(),
        }
    })?;
    let restart_required: Vec<String> = diff
        .cold_changes()
        .iter()
        .map(|change| change.path.clone())
        .collect();

    let live_config = taxis::reload::preserve_restart_required_values(&config, &new_config, &diff)
        .map_err(|e| ApiError::Internal {
            message: format!("failed to preserve cold config values: {e}"),
            location: snafu::location!(),
        })?;
    let live_config =
        taxis::redact::preserve_config_secret_leaves(&live_config, &config).map_err(|e| {
            ApiError::Internal {
                message: format!("failed to preserve secret config values: {e}"),
                location: snafu::location!(),
            }
        })?;

    taxis::loader::write_config(&state.oikos, &new_config).map_err(|e| ApiError::Internal {
        message: format!("failed to write config: {e}"),
        location: snafu::location!(),
    })?;

    *config = live_config.clone();
    let redacted = taxis::redact::redact(&config);
    drop(config);

    let _ = state.config_tx.send(live_config);

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
    fn config_update_response_serializes_snake_case() {
        let resp = ConfigUpdateResponse {
            section: "agents".to_owned(),
            config: json!({"list": []}),
            restart_required: vec!["agents.model".to_owned()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["section"], "agents");
        assert!(json.get("restart_required").is_some());
        assert!(json["restart_required"].as_array().unwrap().len() == 1);
    }
}
