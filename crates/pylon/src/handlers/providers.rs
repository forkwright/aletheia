//! Provider inventory and model-route decision endpoints.
//!
//! Exposes provider configuration, supported models, health, credential source
//! classes, and per-model route resolution so operators and control-plane
//! consumers can inspect LLM routing without log spelunking.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use symbolon::types::Role;
use utoipa::IntoParams;

use crate::error::{ApiError, BadRequestSnafu};
use crate::extract::{Claims, require_role};
use crate::state::ProvidersState;

#[path = "providers_dto.rs"]
mod providers_dto;
pub use providers_dto::{
    ModelProviderReadiness, ProviderInfo, ProviderListResponse, ProviderRouteResponse,
};

/// Query parameters for `GET /api/v1/providers/route`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct RouteQuery {
    /// Model identifier to resolve.
    model: String,
}

/// GET /api/v1/providers: list registered providers and their readiness.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/providers",
    responses(
        (status = 200, description = "Provider inventory", body = ProviderListResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list(
    State(state): State<ProvidersState>,
    claims: Claims,
) -> Result<Json<ProviderListResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;

    let config = state.config.read().await;
    let providers = state.provider_registry.providers();

    let mut infos = Vec::with_capacity(providers.len());
    for provider in providers {
        let provider_config = config.providers.iter().find(|p| p.name == provider.name());

        let health = state
            .provider_registry
            .provider_health(provider.name())
            .unwrap_or(hermeneus::health::ProviderHealth::Down {
                since: jiff::Timestamp::now(),
                reason: hermeneus::health::DownReason::ConsecutiveFailures,
            });

        infos.push(ProviderInfo {
            name: provider.name().to_owned(),
            kind: provider_config
                .map_or_else(|| "unknown".to_owned(), |p| provider_kind_wire(p.kind)),
            deployment_target: provider_config.map_or_else(
                || "cloud".to_owned(),
                |p| deployment_target_wire(p.deployment_target),
            ),
            base_url: provider_config.map_or_else(
                || "default".to_owned(),
                |p| redact_base_url(p.base_url.as_deref()),
            ),
            supported_models: provider
                .supported_models()
                .iter()
                .map(|&m| m.to_owned())
                .collect(),
            configured_models: provider_config.map_or_else(Vec::new, |p| p.models.clone()),
            health: health_status_wire(&health),
            health_reason: health_reason_wire(&health),
            auth_source: provider_config.map_or_else(|| "none".to_owned(), credential_source_class),
            available: matches!(health, hermeneus::health::ProviderHealth::Up),
        });
    }

    Ok(Json(ProviderListResponse { providers: infos }))
}

/// GET /api/v1/providers/route: resolve which provider handles a model.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/providers/route",
    params(RouteQuery),
    responses(
        (status = 200, description = "Resolved provider route", body = ProviderRouteResponse),
        (status = 400, description = "Missing model query parameter", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn route(
    State(state): State<ProvidersState>,
    claims: Claims,
    Query(query): Query<RouteQuery>,
) -> Result<Json<ProviderRouteResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;

    let model = query.model.trim();
    if model.is_empty() {
        return BadRequestSnafu {
            message: "model query parameter is required".to_owned(),
        }
        .fail();
    }

    let (provider, health) = state.provider_registry.find_provider(model).map_or_else(
        || (None, None),
        |p| {
            let health = state.provider_registry.provider_health(p.name()).unwrap_or(
                hermeneus::health::ProviderHealth::Down {
                    since: jiff::Timestamp::now(),
                    reason: hermeneus::health::DownReason::ConsecutiveFailures,
                },
            );
            (Some(p.name().to_owned()), Some(health))
        },
    );

    Ok(Json(ProviderRouteResponse {
        model: model.to_owned(),
        provider: provider.clone(),
        health: health.as_ref().map(health_status_wire),
        available: health
            .as_ref()
            .map(|h| matches!(h, hermeneus::health::ProviderHealth::Up)),
    }))
}

/// Resolve readiness for a list of model identifiers.
///
/// Used by the Nous endpoints to attach per-model provider readiness to agent
/// summaries and status responses.
pub fn resolve_model_readiness(
    registry: &hermeneus::provider::ProviderRegistry,
    models: &[String],
) -> Vec<ModelProviderReadiness> {
    models
        .iter()
        .map(|model| {
            let (provider, health) = registry.find_provider(model).map_or_else(
                || (None, None),
                |p| {
                    let health = registry.provider_health(p.name()).unwrap_or(
                        hermeneus::health::ProviderHealth::Down {
                            since: jiff::Timestamp::now(),
                            reason: hermeneus::health::DownReason::ConsecutiveFailures,
                        },
                    );
                    (Some(p.name().to_owned()), Some(health))
                },
            );

            ModelProviderReadiness {
                model: model.clone(),
                provider,
                health: health.as_ref().map(health_status_wire),
                available: health
                    .as_ref()
                    .is_some_and(|h| matches!(h, hermeneus::health::ProviderHealth::Up)),
            }
        })
        .collect()
}

fn provider_kind_wire(kind: taxis::config::ProviderKind) -> String {
    // WHY: keep wire values stable and human-readable; avoid relying on
    // debug formatting which may change.
    match kind {
        taxis::config::ProviderKind::Anthropic => "anthropic".to_owned(),
        taxis::config::ProviderKind::OpenAi => "openai".to_owned(),
        taxis::config::ProviderKind::OpenAiCompatible => "openai-compatible".to_owned(),
        taxis::config::ProviderKind::ClaudeCode => "claude-code".to_owned(),
        taxis::config::ProviderKind::CodexOauth => "codex-oauth".to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn deployment_target_wire(target: taxis::config::DeploymentTarget) -> String {
    match target {
        taxis::config::DeploymentTarget::LocalHosted => "local-hosted".to_owned(),
        taxis::config::DeploymentTarget::Embedded => "embedded".to_owned(),
        _ => "cloud".to_owned(),
    }
}

fn health_status_wire(health: &hermeneus::health::ProviderHealth) -> String {
    match health {
        hermeneus::health::ProviderHealth::Up => "up".to_owned(),
        hermeneus::health::ProviderHealth::Degraded { .. } => "degraded".to_owned(),
        hermeneus::health::ProviderHealth::Down { .. } => "down".to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn health_reason_wire(health: &hermeneus::health::ProviderHealth) -> Option<String> {
    match health {
        hermeneus::health::ProviderHealth::Degraded {
            consecutive_errors,
            last_error_at,
        } => Some(format!(
            "degraded: consecutive_errors={consecutive_errors}, last_error_at={last_error_at}"
        )),
        hermeneus::health::ProviderHealth::Down { reason, .. } => Some(match reason {
            hermeneus::health::DownReason::ConsecutiveFailures => {
                "down: consecutive failures".to_owned()
            }
            hermeneus::health::DownReason::RateLimited { retry_after_ms } => {
                format!("down: rate limited (retry_after_ms={retry_after_ms})")
            }
            hermeneus::health::DownReason::AuthFailure => "down: authentication failure".to_owned(),
            hermeneus::health::DownReason::Timeout => "down: timeout".to_owned(),
            _ => "down: unknown".to_owned(),
        }),
        _ => None,
    }
}

// WHY: emit class only ("env"), not the variable name — leaking the exact env
// var name narrows an attacker's search space for credential exfiltration.
fn credential_source_class(config: &taxis::config::LlmProviderConfig) -> String {
    config
        .api_key_env
        .as_ref()
        .filter(|s| !s.is_empty())
        .map_or_else(|| "none".to_owned(), |_| "env".to_owned())
}

fn redact_base_url(url: Option<&str>) -> String {
    let Some(url) = url else {
        return "default".to_owned();
    };
    if url.is_empty() {
        return "default".to_owned();
    }

    match url.parse::<axum::http::Uri>() {
        Ok(uri) => {
            let mut parts = axum::http::uri::Parts::from(uri);
            // WHY: strip userinfo (user:pass@), query, and fragment from the
            // exposed URL. Authority userinfo leaks credentials verbatim; query
            // params may carry tokens; fragment is client-side only.
            if let Some(authority) = &parts.authority {
                let auth_str = authority.as_str();
                // Split on '@' to isolate host:port from any userinfo prefix.
                let host_part = auth_str.split_once('@').map_or(auth_str, |(_, h)| h);
                match host_part.parse::<axum::http::uri::Authority>() {
                    Ok(clean_authority) => parts.authority = Some(clean_authority),
                    Err(_) => return "redacted".to_owned(),
                }
            }
            parts.path_and_query = parts.path_and_query.map(|pq| {
                let parsed = pq.path().parse();
                parsed.unwrap_or(pq)
            });
            match axum::http::Uri::from_parts(parts) {
                Ok(redacted) => redacted.to_string(),
                Err(_) => "redacted".to_owned(),
            }
        }
        Err(_) => "redacted".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_wire_values_are_stable() {
        assert_eq!(
            provider_kind_wire(taxis::config::ProviderKind::OpenAi),
            "openai"
        );
        assert_eq!(
            provider_kind_wire(taxis::config::ProviderKind::OpenAiCompatible),
            "openai-compatible"
        );
        assert_eq!(
            provider_kind_wire(taxis::config::ProviderKind::Anthropic),
            "anthropic"
        );
    }

    #[test]
    fn deployment_target_wire_values_are_stable() {
        assert_eq!(
            deployment_target_wire(taxis::config::DeploymentTarget::Cloud),
            "cloud"
        );
        assert_eq!(
            deployment_target_wire(taxis::config::DeploymentTarget::LocalHosted),
            "local-hosted"
        );
    }

    #[test]
    fn redact_base_url_strips_query_and_fragment() {
        assert_eq!(
            redact_base_url(Some("https://api.example.com/v1?key=secret#frag")),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn redact_base_url_strips_userinfo() {
        assert_eq!(
            redact_base_url(Some("https://user:pass@api.example.com/v1")),
            "https://api.example.com/v1"
        );
        assert_eq!(
            redact_base_url(Some("https://user:pass@api.example.com/v1?token=abc")),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn redact_base_url_returns_default_for_none_or_empty() {
        assert_eq!(redact_base_url(None), "default");
        assert_eq!(redact_base_url(Some("")), "default");
    }

    #[test]
    fn health_status_wire_maps_variants() {
        assert_eq!(
            health_status_wire(&hermeneus::health::ProviderHealth::Up),
            "up"
        );
        assert_eq!(
            health_status_wire(&hermeneus::health::ProviderHealth::Degraded {
                consecutive_errors: 1,
                last_error_at: jiff::Timestamp::now(),
            }),
            "degraded"
        );
    }

    #[test]
    fn credential_source_class_respects_env_and_none() {
        let with_env = taxis::config::LlmProviderConfig {
            name: "openai".to_owned(),
            kind: taxis::config::ProviderKind::OpenAi,
            base_url: None,
            api_key_env: Some("OPENAI_API_KEY".to_owned()),
            api_family: None,
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: taxis::config::DeploymentTarget::Cloud,
            models: Vec::new(),
        };
        assert_eq!(credential_source_class(&with_env), "env");

        let without_env = taxis::config::LlmProviderConfig {
            api_key_env: None,
            ..with_env
        };
        assert_eq!(credential_source_class(&without_env), "none");
    }
}
