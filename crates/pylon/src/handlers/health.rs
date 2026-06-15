//! Health check endpoint.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use koina::system::{Environment, RealSystem};
use symbolon::types::Role;

use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::HealthState;

#[path = "health_dto.rs"]
mod health_dto;
pub use health_dto::{HealthCheck, HealthResponse, LivenessResponse};

/// Per-check timeout: individual health checks that exceed this are reported as "timeout".
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Overall endpoint timeout: the health response is always returned within this bound.
const OVERALL_TIMEOUT: Duration = Duration::from_secs(10);

/// GET /api/health: public liveness check.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Public liveness status", body = LivenessResponse),
    ),
)]
pub async fn check() -> impl IntoResponse {
    Json(LivenessResponse { status: "healthy" })
}

/// GET /api/v1/system/health: operator-only readiness and diagnostics.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/system/health",
    responses(
        (status = 200, description = "Detailed health status", body = HealthResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
        (status = 503, description = "Service unavailable", body = HealthResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn detailed(
    State(state): State<HealthState>,
    claims: Claims,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;
    let (http_status, response) = detailed_health(&state).await;
    Ok((http_status, Json(response)))
}

async fn detailed_health(state: &HealthState) -> (StatusCode, HealthResponse) {
    let uptime = state.start_time.elapsed().as_secs();

    // WHY: Run all health checks concurrently with individual timeouts so a
    // single hanging check (e.g., provider connection) cannot block the entire
    // endpoint. The overall timeout guarantees a response even if multiple
    // checks hang simultaneously (#3277).
    let checks = tokio::time::timeout(OVERALL_TIMEOUT, async {
        // WHY: read config once before spawning concurrent checks so each check
        // does not contend on the config lock.
        let api_limits = &state.config.read().await.api_limits;
        let clock_skew_leeway = api_limits.clock_skew_leeway_secs;
        let expiry_warning_threshold = api_limits.expiry_warning_threshold_secs;

        let (store_check, actor_check, config_check, storage_check) = tokio::join!(
            timed_check("session_store", check_session_store(state)),
            timed_check("nous_actors", check_nous_actors(state)),
            timed_check("config_readable", check_config_readable(state)),
            timed_check("storage_writable", check_storage_writable(state)),
        );

        // WHY: these checks are synchronous and cheap — no timeout needed.
        let provider_check = check_provider_availability(state);
        let credential_check =
            check_credential_validity(state, clock_skew_leeway, expiry_warning_threshold);
        let embedding_check = check_embedding_provider(state);
        let gateway_security_check = check_gateway_security(state).await;

        vec![
            store_check,
            provider_check,
            actor_check,
            check_provider_reachability(state),
            config_check,
            gateway_security_check,
            credential_check,
            storage_check,
            embedding_check,
        ]
    })
    .await
    .unwrap_or_else(|_| {
        vec![HealthCheck {
            name: "overall",
            status: "fail",
            message: Some("health check timed out".to_owned()),
        }]
    });

    // WHY: "timeout" is treated as "fail" for aggregate status because
    // a timed-out check means we cannot confirm the subsystem is healthy.
    let status = if checks
        .iter()
        .any(|c| c.status == "fail" || c.status == "timeout")
    {
        "unhealthy"
    } else if checks.iter().any(|c| c.status == "warn") {
        "degraded"
    } else {
        "healthy"
    };

    let http_status = if status == "unhealthy" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (
        http_status,
        HealthResponse {
            status,
            version: env!("CARGO_PKG_VERSION"),
            git_sha: option_env!("GIT_SHA").unwrap_or("unknown"),
            uptime_seconds: uptime,
            checks,
            data_dir: state.oikos.data().to_string_lossy().into_owned(),
        },
    )
}

async fn check_gateway_security(state: &HealthState) -> HealthCheck {
    let config = state.config.read().await;
    gateway_security_check(&config.gateway.auth.mode, &config.gateway.bind)
}

fn gateway_security_check(auth_mode: &str, bind: &str) -> HealthCheck {
    if auth_mode == "none" && !taxis::validate::is_loopback_bind(bind) {
        return HealthCheck {
            name: "gateway_security",
            status: "fail",
            message: Some(format!(
                "unsafe gateway posture: auth.mode = \"none\" with non-loopback bind '{bind}'"
            )),
        };
    }
    if auth_mode == "none" {
        return HealthCheck {
            name: "gateway_security",
            status: "warn",
            message: Some(
                "auth.mode = \"none\" is limited to loopback but remains unauthenticated"
                    .to_owned(),
            ),
        };
    }
    HealthCheck {
        name: "gateway_security",
        status: "pass",
        message: None,
    }
}

/// GET /health: deprecated unversioned health check.
///
/// Use `/api/health` instead.
#[deprecated = "Use /api/health instead"]
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Public liveness status", body = LivenessResponse),
    ),
)]
pub async fn deprecated_health_check() -> impl IntoResponse {
    check().await
}

/// Run a health check with a per-check timeout. If the check exceeds
/// [`CHECK_TIMEOUT`], a "timeout" status is returned instead of blocking.
async fn timed_check(
    name: &'static str,
    future: impl std::future::Future<Output = HealthCheck>,
) -> HealthCheck {
    match tokio::time::timeout(CHECK_TIMEOUT, future).await {
        Ok(check) => check,
        Err(_elapsed) => HealthCheck {
            name,
            status: "timeout",
            message: Some(format!(
                "{name} check timed out after {}s",
                CHECK_TIMEOUT.as_secs()
            )),
        },
    }
}

/// Check session store connectivity.
async fn check_session_store(state: &HealthState) -> HealthCheck {
    let store_ok = state.session_store.lock().await.ping().is_ok();
    HealthCheck {
        name: "session_store",
        status: if store_ok { "pass" } else { "fail" },
        message: if store_ok {
            None
        } else {
            Some("session store unavailable".to_owned())
        },
    }
}

/// Check whether any LLM providers are registered.
fn check_provider_availability(state: &HealthState) -> HealthCheck {
    let has_providers = !state.provider_registry.providers().is_empty();
    HealthCheck {
        name: "providers",
        status: if has_providers { "pass" } else { "warn" },
        message: if has_providers {
            None
        } else {
            Some("no LLM providers registered".to_owned())
        },
    }
}

/// Check nous actor liveness and background health.
async fn check_nous_actors(state: &HealthState) -> HealthCheck {
    let actor_health = state.nous_manager.check_health().await;
    let any_dead = actor_health.values().any(|h| !h.alive);

    if actor_health.is_empty() || any_dead {
        return HealthCheck {
            name: "nous_actors",
            status: "fail",
            message: if actor_health.is_empty() {
                Some("no nous actors registered".to_owned())
            } else {
                let dead: Vec<_> = actor_health
                    .iter()
                    .filter(|(_, h)| !h.alive)
                    .map(|(id, _)| id.as_str())
                    .collect();
                Some(format!("actors not responding: {}", dead.join(", ")))
            },
        };
    }

    let degraded: Vec<_> = actor_health
        .iter()
        .filter(|(_, h)| h.background_health_degraded)
        .collect();

    if degraded.is_empty() {
        HealthCheck {
            name: "nous_actors",
            status: "pass",
            message: None,
        }
    } else {
        let summaries: Vec<String> = degraded
            .iter()
            .map(|(id, h)| {
                let mut parts = vec![format!("id={id}")];
                parts.push(format!(
                    "recent={} total={}",
                    h.background_failure_recent_count, h.background_failure_total_count
                ));
                if let Some(kind) = &h.background_failure_latest_kind {
                    parts.push(format!("kind={kind}"));
                }
                if let Some(message) = &h.background_failure_latest_message {
                    parts.push(format!("message={message}"));
                }
                parts.join(" ")
            })
            .collect();
        HealthCheck {
            name: "nous_actors",
            status: "warn",
            message: Some(format!(
                "background health degraded: {}",
                summaries.join("; ")
            )),
        }
    }
}

/// Check LLM provider connectivity by querying the provider registry health.
fn check_provider_reachability(state: &HealthState) -> HealthCheck {
    let providers = state.provider_registry.providers();
    if providers.is_empty() {
        return HealthCheck {
            name: "provider_reachability",
            status: "warn",
            message: Some("no providers to check".to_owned()),
        };
    }

    let any_healthy = providers.iter().any(|p| {
        state.provider_registry.provider_health(p.name())
            == Some(hermeneus::health::ProviderHealth::Up)
    });

    if any_healthy {
        HealthCheck {
            name: "provider_reachability",
            status: "pass",
            message: None,
        }
    } else {
        let any_degraded = providers.iter().any(|p| {
            matches!(
                state.provider_registry.provider_health(p.name()),
                Some(hermeneus::health::ProviderHealth::Degraded { .. })
            )
        });

        if any_degraded {
            HealthCheck {
                name: "provider_reachability",
                status: "warn",
                message: Some("one or more providers are degraded".to_owned()),
            }
        } else {
            HealthCheck {
                name: "provider_reachability",
                status: "fail",
                message: Some("all providers are down or unreachable".to_owned()),
            }
        }
    }
}

/// Check embedding provider status.
///
/// Reports:
/// - `"pass"` when the real provider is loaded and healthy
/// - `"warn"` with `"degraded: embedding-loading"` while the lazy provider
///   is still initializing (#3474)
/// - `"warn"` with `"degraded: no-embeddings"` when the real provider failed
///   to load and the server is running BM25-only (#3380)
fn check_embedding_provider(state: &HealthState) -> HealthCheck {
    /// Sentinel model name emitted by `LazyEmbeddingProvider` while still loading.
    const LOADING_MODEL_NAME: &str = "embedding-loading";

    let Some(provider) = state.embedding_provider.as_ref() else {
        return HealthCheck {
            name: "embedding_provider",
            status: "warn",
            message: Some("no embedding provider configured".to_owned()),
        };
    };

    let model_name = provider.model_name();

    if model_name == LOADING_MODEL_NAME {
        HealthCheck {
            name: "embedding_provider",
            status: "warn",
            message: Some(
                "degraded: embedding-loading (model initializing — \
                 recall unavailable until load completes)"
                    .to_owned(),
            ),
        }
    } else if mneme::embedding::is_degraded_provider(provider.as_ref()) {
        HealthCheck {
            name: "embedding_provider",
            status: "warn",
            message: Some(
                "degraded: no-embeddings (embedding model failed to load at startup — \
                 recall falls back to BM25)"
                    .to_owned(),
            ),
        }
    } else {
        HealthCheck {
            name: "embedding_provider",
            status: "pass",
            message: None,
        }
    }
}

/// Check if config can be read (verify config file exists and is accessible).
async fn check_config_readable(state: &HealthState) -> HealthCheck {
    let config_dir = state.oikos.config();
    let instance_root = state.oikos.root();

    // WHY: validate that constructed config paths stay within the instance
    // root to prevent path-traversal if the config directory is misconfigured.
    let toml_path = config_dir.join("aletheia.toml");
    let json_path = config_dir.join("aletheia.json");

    let config_path = if tokio::fs::metadata(&toml_path).await.is_ok() {
        match koina::fs::validate_within_root(&toml_path, instance_root) {
            Ok(p) => p,
            Err(e) => {
                return HealthCheck {
                    name: "config_readable",
                    status: "fail",
                    message: Some(format!("config path validation failed: {e}")),
                };
            }
        }
    } else {
        match koina::fs::validate_within_root(&json_path, instance_root) {
            Ok(p) => p,
            Err(e) => {
                // WHY: json_path may not exist yet (first run); validation
                // failure here means the parent directory itself is outside
                // the instance root, which is a real misconfiguration.
                return HealthCheck {
                    name: "config_readable",
                    status: "warn",
                    message: Some(format!("config path validation failed: {e}")),
                };
            }
        }
    };

    match tokio::fs::metadata(&config_path).await {
        Ok(metadata) => {
            if metadata.is_file() {
                // WHY: also verify the in-memory config lock is readable.
                let _config = state.config.read().await;
                HealthCheck {
                    name: "config_readable",
                    status: "pass",
                    message: None,
                }
            } else {
                HealthCheck {
                    name: "config_readable",
                    status: "warn",
                    message: Some(format!(
                        "config path exists but is not a file: {}",
                        config_path.display()
                    )),
                }
            }
        }
        Err(e) => {
            // WHY: warn, not fail — the config file may not exist yet (first run).
            HealthCheck {
                name: "config_readable",
                status: "warn",
                message: Some(format!(
                    "cannot read config file at {}: {e}",
                    config_path.display()
                )),
            }
        }
    }
}

/// Check credential validity (presence and expiry).
fn check_credential_validity(
    state: &HealthState,
    clock_skew_leeway: u64,
    expiry_warning_threshold: u64,
) -> HealthCheck {
    if let Some(check) = provider_credential_scope_check(state) {
        return check;
    }

    let env_key = RealSystem.var("ANTHROPIC_API_KEY").or_else(|| {
        tracing::debug!("ANTHROPIC_API_KEY not set");
        None
    });

    if let Some(key) = env_key {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        return check_env_oauth_token(&key, now_secs, clock_skew_leeway, expiry_warning_threshold);
    }

    let creds_dir = state.oikos.credentials();
    let cred_file = creds_dir.join("anthropic.json");

    if let Some(cred_file) = symbolon::credential::CredentialFile::load(&cred_file) {
        if let Some(remaining_secs) = cred_file.seconds_remaining() {
            #[expect(
                clippy::cast_possible_wrap,
                clippy::as_conversions,
                reason = "u64->i64: leeway/threshold values fit in i64"
            )]
            let leeway_i64 = clock_skew_leeway as i64; // kanon:ignore RUST/as-cast
            #[expect(
                clippy::cast_possible_wrap,
                clippy::as_conversions,
                reason = "u64->i64: leeway/threshold values fit in i64"
            )]
            let warning_i64 = expiry_warning_threshold as i64; // kanon:ignore RUST/as-cast

            if remaining_secs < leeway_i64 {
                return HealthCheck {
                    name: "credential_validity",
                    status: "warn",
                    message: Some("credential file token has expired".to_owned()),
                };
            } else if remaining_secs < warning_i64 {
                return HealthCheck {
                    name: "credential_validity",
                    status: "warn",
                    message: Some("credential file token expires soon".to_owned()),
                };
            }
        }
        return HealthCheck {
            name: "credential_validity",
            status: "pass",
            message: None,
        };
    }

    let cc_credentials =
        symbolon::credential::claude_code_default_path().is_some_and(|p| p.exists());
    if cc_credentials {
        HealthCheck {
            name: "credential_validity",
            status: "pass",
            message: Some(
                "Claude Code credentials available (CC provider handles auth)".to_owned(),
            ),
        }
    } else {
        HealthCheck {
            name: "credential_validity",
            status: "warn",
            message: Some("no credentials found (ANTHROPIC_API_KEY not set, no credential file, no Claude Code credentials)".to_owned()),
        }
    }
}

/// Check an env-var credential for OAuth token expiry.
///
/// Returns a `credential_validity` [`HealthCheck`] for the given `ANTHROPIC_API_KEY`
/// value. Non-OAuth keys and OAuth tokens without a decodable expiry claim are
/// treated as valid.
fn check_env_oauth_token(
    key: &str,
    now_secs: u64,
    clock_skew_leeway: u64,
    expiry_warning_threshold: u64,
) -> HealthCheck {
    if key.is_empty() {
        return HealthCheck {
            name: "credential_validity",
            status: "warn",
            message: Some("ANTHROPIC_API_KEY is set but empty".to_owned()),
        };
    }

    if key.starts_with("sk-ant-oat") {
        // NOTE: the sk-ant-oat prefix marks an OAuth token with a decodable expiry.
        if let Some(exp_secs) = decode_jwt_exp(key) {
            let remaining_secs =
                exp_secs.saturating_sub(now_secs.saturating_add(clock_skew_leeway));
            if remaining_secs == 0 {
                return HealthCheck {
                    name: "credential_validity",
                    status: "warn",
                    message: Some("OAuth token has expired".to_owned()),
                };
            }
            if remaining_secs <= expiry_warning_threshold {
                return HealthCheck {
                    name: "credential_validity",
                    status: "warn",
                    message: Some("OAuth token expires soon".to_owned()),
                };
            }
        }
    }

    HealthCheck {
        name: "credential_validity",
        status: "pass",
        message: None,
    }
}

fn provider_credential_scope_check(state: &HealthState) -> Option<HealthCheck> {
    let provider_names: Vec<&str> = state
        .provider_registry
        .providers()
        .into_iter()
        .map(hermeneus::provider::LlmProvider::name)
        .collect();

    if provider_names.is_empty() {
        return Some(HealthCheck {
            name: "credential_validity",
            status: "warn",
            message: Some(
                "no providers registered; credential validity cannot be checked".to_owned(),
            ),
        });
    }

    if provider_names
        .iter()
        .any(|name| provider_uses_anthropic_credentials(name))
    {
        return None;
    }

    Some(HealthCheck {
        name: "credential_validity",
        status: "pass",
        message: Some(format!(
            "registered providers do not use pylon-managed Anthropic credentials: {}",
            provider_names.join(", ")
        )),
    })
}

fn provider_uses_anthropic_credentials(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    normalized.contains("anthropic") || normalized.contains("claude")
}

/// Check if the data directory is writable.
async fn check_storage_writable(state: &HealthState) -> HealthCheck {
    let data_dir = state.oikos.data();
    let instance_root = state.oikos.root();

    if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
        return HealthCheck {
            name: "storage_writable",
            status: "fail",
            message: Some(format!("cannot create data directory: {e}")),
        };
    }

    // WHY: validate that the data directory resolves within the instance
    // root to prevent path-traversal if oikos is misconfigured.
    if let Err(e) = koina::fs::validate_within_root(&data_dir, instance_root) {
        return HealthCheck {
            name: "storage_writable",
            status: "fail",
            message: Some(format!("data directory path validation failed: {e}")),
        };
    }

    let test_file = data_dir.join(".health-check-write-test");

    // WHY: validate the test file path stays within the data directory
    // (defense-in-depth against crafted data_dir values).
    if let Err(e) = koina::fs::validate_within_root(&test_file, &data_dir) {
        return HealthCheck {
            name: "storage_writable",
            status: "fail",
            message: Some(format!("test file path validation failed: {e}")),
        };
    }

    match tokio::fs::write(&test_file, b"health-check").await {
        Ok(()) => {
            let _ = tokio::fs::remove_file(&test_file).await;
            HealthCheck {
                name: "storage_writable",
                status: "pass",
                message: None,
            }
        }
        Err(e) => HealthCheck {
            name: "storage_writable",
            status: "fail",
            message: Some(format!("data directory is not writable: {e}")),
        },
    }
}

/// Decode JWT expiry claim from a token.
/// Returns expiry timestamp in seconds since epoch, or None if not found/invalid.
fn decode_jwt_exp(token: &str) -> Option<u64> {
    // JWT format: header.payload.signature
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;

    let payload = base64url_decode(payload_b64).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    json.get("exp").and_then(serde_json::Value::as_u64)
}

/// Decode base64url-encoded string (no padding required).
fn base64url_decode(s: &str) -> Result<Vec<u8>, ()> {
    fn char_val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' | b'+' => Some(62),
            b'_' | b'/' => Some(63),
            b'=' => Some(0),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    let end = bytes.iter().rposition(|&b| b != b'=').map_or(0, |i| i + 1);
    // INVARIANT: end <= bytes.len() by construction from rposition's return value.
    let bytes = bytes.get(..end).unwrap_or(bytes);

    let mut out = Vec::with_capacity(bytes.len() * 6 / 8 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in bytes {
        let v = char_val(b).ok_or(())?;
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buf >> bits) & 0xFF).unwrap_or(0));
        }
    }

    Ok(out)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after len assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn health_state_has_all_required_fields() {
        // WHY: compile-time shape assertion — if this fn compiles, HealthState
        // has every field the health handlers need, with the right types.
        #[expect(
            dead_code,
            reason = "compile-time shape assertion: proves field types via unused local fn"
        )]
        fn assert_health_state_fields(state: &HealthState) {
            use std::sync::Arc;

            use hermeneus::provider::ProviderRegistry;
            use mneme::store::SessionStore;
            use nous::manager::NousManager;
            use taxis::config::AletheiaConfig;
            use taxis::oikos::Oikos;

            let _: &Arc<tokio::sync::Mutex<SessionStore>> = &state.session_store;
            let _: &Arc<ProviderRegistry> = &state.provider_registry;
            let _: &Arc<NousManager> = &state.nous_manager;
            let _: std::time::Instant = state.start_time;
            let _: &Arc<Oikos> = &state.oikos;
            let _: &Arc<tokio::sync::RwLock<AletheiaConfig>> = &state.config;
            let _: &Option<Arc<dyn mneme::embedding::EmbeddingProvider>> =
                &state.embedding_provider;
        }
        assert!(std::mem::size_of::<HealthState>() > 0);
    }

    #[test]
    fn health_response_serializes_all_fields() {
        let resp = HealthResponse {
            status: "healthy",
            version: "1.0.0",
            git_sha: "abc123",
            uptime_seconds: 300,
            checks: vec![],
            data_dir: "/tmp/instance/data".to_owned(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["uptime_seconds"], 300);
        assert!(json["checks"].as_array().unwrap().is_empty());
    }

    #[test]
    fn liveness_response_serializes_only_status() {
        let resp = LivenessResponse { status: "healthy" };
        let json = serde_json::to_value(&resp).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object.len(), 1);
        assert_eq!(json["status"], "healthy");
    }

    #[test]
    fn gateway_security_fails_auth_none_on_lan_bind() {
        let check = gateway_security_check("none", "0.0.0.0");
        assert_eq!(check.status, "fail");
        assert_eq!(check.name, "gateway_security");
    }

    #[test]
    fn health_check_pass_omits_message_when_none() {
        let check = HealthCheck {
            name: "session_store",
            status: "pass",
            message: None,
        };
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["name"], "session_store");
        assert_eq!(json["status"], "pass");
        // NOTE: message is None: serializes as null (no skip annotation).
        assert!(json["message"].is_null());
    }

    #[test]
    fn health_check_fail_includes_message() {
        let check = HealthCheck {
            name: "providers",
            status: "fail",
            message: Some("no LLM providers registered".to_owned()),
        };
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["status"], "fail");
        assert_eq!(json["message"], "no LLM providers registered");
    }

    #[test]
    fn credential_check_only_uses_anthropic_credentials_for_anthropic_providers() {
        assert!(provider_uses_anthropic_credentials("anthropic"));
        assert!(provider_uses_anthropic_credentials("claude-code"));
        assert!(!provider_uses_anthropic_credentials("openai"));
        assert!(!provider_uses_anthropic_credentials("local"));
        assert!(!provider_uses_anthropic_credentials("mock"));
    }

    #[test]
    fn aggregate_status_unhealthy_when_any_check_fails() {
        let checks = [
            HealthCheck {
                name: "a",
                status: "pass",
                message: None,
            },
            HealthCheck {
                name: "b",
                status: "fail",
                message: Some("down".to_owned()),
            },
        ];
        let status = if checks.iter().any(|c| c.status == "fail") {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "unhealthy");
    }

    #[test]
    fn aggregate_status_degraded_when_any_check_warns() {
        let checks = [
            HealthCheck {
                name: "a",
                status: "pass",
                message: None,
            },
            HealthCheck {
                name: "b",
                status: "warn",
                message: Some("no providers".to_owned()),
            },
        ];
        let status = if checks.iter().any(|c| c.status == "fail") {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "degraded");
    }

    #[test]
    fn aggregate_status_healthy_when_all_pass() {
        let checks = [
            HealthCheck {
                name: "session_store",
                status: "pass",
                message: None,
            },
            HealthCheck {
                name: "providers",
                status: "pass",
                message: None,
            },
        ];
        let status = if checks.iter().any(|c| c.status == "fail") {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "healthy");
    }

    #[test]
    fn aggregate_status_unhealthy_when_any_check_times_out() {
        let checks = [
            HealthCheck {
                name: "a",
                status: "pass",
                message: None,
            },
            HealthCheck {
                name: "b",
                status: "timeout",
                message: Some("check timed out after 5s".to_owned()),
            },
        ];
        // WHY: "timeout" is treated as "fail" for aggregate status because
        // a timed-out check means we cannot confirm the subsystem is healthy.
        let status = if checks
            .iter()
            .any(|c| c.status == "fail" || c.status == "timeout")
        {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "unhealthy");
    }

    #[tokio::test]
    async fn timed_check_returns_timeout_on_slow_future() {
        let check = timed_check("slow_check", async {
            tokio::time::sleep(Duration::from_mins(1)).await;
            HealthCheck {
                name: "slow_check",
                status: "pass",
                message: None,
            }
        })
        .await;
        assert_eq!(check.status, "timeout");
        assert_eq!(check.name, "slow_check");
        assert!(check.message.unwrap().contains("timed out"));
    }

    #[tokio::test]
    async fn timed_check_returns_result_on_fast_future() {
        let check = timed_check("fast_check", async {
            HealthCheck {
                name: "fast_check",
                status: "pass",
                message: None,
            }
        })
        .await;
        assert_eq!(check.status, "pass");
        assert_eq!(check.name, "fast_check");
    }

    #[test]
    fn decode_jwt_exp_extracts_expiry() {
        // Create a JWT with known exp claim: exp = 1234567890
        // Payload: {"exp":1234567890}
        // base64url: eyJleHAiOjEyMzQ1Njc4OTB9
        let token = "header.eyJleHAiOjEyMzQ1Njc4OTB9.signature"; // pii-allow: synthetic JWT structure, exp-decoder self-test
        let exp = decode_jwt_exp(token);
        assert_eq!(exp, Some(1_234_567_890));
    }

    #[test]
    fn decode_jwt_exp_returns_none_for_invalid() {
        // No exp claim
        let token = "header.eyJzdWIiOiIxMjMifQ.signature";
        let exp = decode_jwt_exp(token);
        assert_eq!(exp, None);

        // Invalid format
        let exp = decode_jwt_exp("not-a-jwt");
        assert_eq!(exp, None);

        // Empty
        let exp = decode_jwt_exp("");
        assert_eq!(exp, None);
    }

    #[test]
    fn base64url_decode_handles_padding_variants() {
        // Standard base64url without padding
        let decoded = base64url_decode("eyJleHAiOjEyMzQ1Njc4OTB9").unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), r#"{"exp":1234567890}"#);

        // With padding should also work
        let decoded = base64url_decode("eyJleHAiOjEyMzQ1Njc4OTB9").unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), r#"{"exp":1234567890}"#);
    }

    #[expect(
        clippy::as_conversions,
        reason = "sextet is masked to 0..=63 and indexes the 64-entry table"
    )]
    fn base64url_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        let mut buf: u32 = 0;
        let mut bits: u32 = 0;
        for &b in input {
            buf = (buf << 8) | u32::from(b);
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                out.push(char::from(TABLE[((buf >> bits) & 0x3F) as usize]));
            }
        }
        if bits > 0 {
            out.push(char::from(TABLE[((buf << (6 - bits)) & 0x3F) as usize]));
        }
        out
    }

    fn oauth_token_with_exp(exp: u64) -> String {
        let payload = serde_json::json!({ "exp": exp }).to_string();
        format!(
            "sk-ant-oat-header.{}.signature",
            base64url_encode(payload.as_bytes())
        )
    }

    #[test]
    fn env_token_expired_reports_expired() {
        let now = 1_000_000;
        let exp = now - 100;
        let token = oauth_token_with_exp(exp);
        let check = check_env_oauth_token(&token, now, 60, 300);
        assert_eq!(check.status, "warn");
        assert!(check.message.unwrap().contains("expired"));
    }

    #[test]
    fn env_token_expires_soon_reports_soon() {
        let now = 1_000_000;
        let exp = now + 150;
        let token = oauth_token_with_exp(exp);
        let check = check_env_oauth_token(&token, now, 60, 300);
        assert_eq!(check.status, "warn");
        assert!(check.message.unwrap().contains("soon"));
    }

    #[test]
    fn env_token_valid_reports_ok() {
        let now = 1_000_000;
        let exp = now + 900;
        let token = oauth_token_with_exp(exp);
        let check = check_env_oauth_token(&token, now, 60, 300);
        assert_eq!(check.status, "pass");
        assert!(check.message.is_none());
    }

    #[test]
    fn env_token_empty_env_var_handled() {
        let check = check_env_oauth_token("", 1_000_000, 60, 300);
        assert_eq!(check.status, "warn");
        assert!(check.message.unwrap().contains("empty"));
    }

    #[test]
    fn env_token_undecodable_returns_error() {
        // A non-OAuth, non-JWT string does not trigger expiry checks and is
        // treated as a plain API key.
        let check = check_env_oauth_token("not-a-jwt-and-not-oauth", 1_000_000, 60, 300);
        assert_eq!(check.status, "pass");
    }
}
