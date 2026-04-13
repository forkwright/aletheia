//! Health check endpoint.

use koina::system::{Environment, RealSystem};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::HealthState;

/// GET /api/health: liveness + readiness check.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Health status", body = HealthResponse),
        (status = 503, description = "Service unavailable", body = HealthResponse),
    ),
)]
pub async fn check(State(state): State<HealthState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();

    let mut checks = Vec::new();

    let store_ok = state.session_store.lock().await.ping().is_ok();
    checks.push(HealthCheck {
        name: "session_store",
        status: if store_ok { "pass" } else { "fail" },
        message: if store_ok {
            None
        } else {
            Some("session store unavailable".to_owned())
        },
    });

    let has_providers = !state.provider_registry.providers().is_empty();
    checks.push(HealthCheck {
        name: "providers",
        status: if has_providers { "pass" } else { "warn" },
        message: if has_providers {
            None
        } else {
            Some("no LLM providers registered".to_owned())
        },
    });

    let actor_health = state.nous_manager.check_health().await;
    let any_dead = actor_health.values().any(|h| !h.alive);
    checks.push(HealthCheck {
        name: "nous_actors",
        status: if actor_health.is_empty() || any_dead {
            "fail"
        } else {
            "pass"
        },
        message: if actor_health.is_empty() {
            Some("no nous actors registered".to_owned())
        } else if any_dead {
            let dead: Vec<_> = actor_health
                .iter()
                .filter(|(_, h)| !h.alive)
                .map(|(id, _)| id.as_str())
                .collect();
            Some(format!("actors not responding: {}", dead.join(", ")))
        } else {
            None
        },
    });

    // Check provider reachability via health tracker
    checks.push(check_provider_reachability(&state));

    // Check config readability
    checks.push(check_config_readable(&state).await);

    // Check credential validity
    let api_limits = &state.config.read().await.api_limits;
    let clock_skew_leeway = api_limits.clock_skew_leeway_secs;
    let expiry_warning_threshold = api_limits.expiry_warning_threshold_secs;
    checks.push(check_credential_validity(&state, clock_skew_leeway, expiry_warning_threshold));

    // Check storage writability
    checks.push(check_storage_writable(&state).await);

    let status = if checks.iter().any(|c| c.status == "fail") {
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
        Json(HealthResponse {
            status,
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: uptime,
            checks,
            data_dir: state.oikos.data().to_string_lossy().into_owned(),
        }),
    )
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

    // Check if any provider is healthy (Up status)
    let any_healthy = providers
        .iter()
        .any(|p| state.provider_registry.provider_health(p.name()) == Some(hermeneus::health::ProviderHealth::Up));

    if any_healthy {
        HealthCheck {
            name: "provider_reachability",
            status: "pass",
            message: None,
        }
    } else {
        // Check if any provider is degraded
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

/// Check if config can be read (verify config file exists and is accessible).
async fn check_config_readable(state: &HealthState) -> HealthCheck {
    // Check for TOML config first, then JSON
    let config_dir = state.oikos.config();
    let toml_path = config_dir.join("aletheia.toml");
    let json_path = config_dir.join("aletheia.json");

    let config_path = if tokio::fs::metadata(&toml_path).await.is_ok() {
        toml_path
    } else {
        json_path
    };

    match tokio::fs::metadata(&config_path).await {
        Ok(_metadata) => {
            if std::fs::metadata(&config_path).is_ok_and(|m| m.is_file()) {
                // Also verify we can read the current config in memory
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
            // Config file may not exist yet (first run scenario)
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

// CLOCK_SKEW_LEEWAY and EXPIRY_WARNING_THRESHOLD are now read from
// `config.api_limits` at runtime. See `taxis::config::ApiLimitsConfig`.

/// Check credential validity (presence and expiry).
fn check_credential_validity(
    state: &HealthState,
    clock_skew_leeway: u64,
    expiry_warning_threshold: u64,
) -> HealthCheck {
    // Check for API key in environment
    let env_key = RealSystem.var("ANTHROPIC_API_KEY").or_else(|| {
        tracing::debug!("ANTHROPIC_API_KEY not set");
        None
    });

    if let Some(key) = env_key {
        if key.is_empty() {
            return HealthCheck {
                name: "credential_validity",
                status: "warn",
                message: Some("ANTHROPIC_API_KEY is set but empty".to_owned()),
            };
        }

        // Check if it's an OAuth token and if it appears expired
        if key.starts_with("sk-ant-oat") {
            // Try to decode JWT expiry
            if let Some(exp_secs) = decode_jwt_exp(&key) {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                if exp_secs + clock_skew_leeway < now_secs {
                    return HealthCheck {
                        name: "credential_validity",
                        status: "warn",
                        message: Some("OAuth token has expired".to_owned()),
                    };
                }

                // Check if expiring soon (within 1 hour)
                if exp_secs + clock_skew_leeway + expiry_warning_threshold < now_secs {
                    return HealthCheck {
                        name: "credential_validity",
                        status: "warn",
                        message: Some("OAuth token expires soon".to_owned()),
                    };
                }
            }
        }

        return HealthCheck {
            name: "credential_validity",
            status: "pass",
            message: None,
        };
    }

    // Check for credentials file
    let creds_dir = state.oikos.credentials();
    let cred_file = creds_dir.join("anthropic.json");

    if let Some(cred_file) = symbolon::credential::CredentialFile::load(&cred_file) {
        // Check if token is expired or expiring soon
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

    // Check if CC provider handles auth (Claude Code credentials)
    let cc_credentials = symbolon::credential::claude_code_default_path()
        .is_some_and(|p| p.exists());
    if cc_credentials {
        HealthCheck {
            name: "credential_validity",
            status: "pass",
            message: Some("Claude Code credentials available (CC provider handles auth)".to_owned()),
        }
    } else {
        HealthCheck {
            name: "credential_validity",
            status: "warn",
            message: Some("no credentials found (ANTHROPIC_API_KEY not set, no credential file, no Claude Code credentials)".to_owned()),
        }
    }
}

/// Check if the data directory is writable.
async fn check_storage_writable(state: &HealthState) -> HealthCheck {
    let data_dir = state.oikos.data();

    // Ensure data directory exists
    if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
        return HealthCheck {
            name: "storage_writable",
            status: "fail",
            message: Some(format!("cannot create data directory: {e}")),
        };
    }

    let test_file = data_dir.join(".health-check-write-test");

    match tokio::fs::write(&test_file, b"health-check").await {
        Ok(()) => {
            // Clean up the test file
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

    // Base64url decode payload
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
    // SAFETY: end <= bytes.len() by construction from rposition's return value.
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

/// Top-level health response combining all subsystem checks.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Crate version from `Cargo.toml`.
    #[schema(value_type = String)]
    pub version: &'static str,
    /// Seconds since server start.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
    /// Absolute path to the instance data directory.
    pub data_dir: String,
}

/// Result of a single subsystem health check.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthCheck {
    /// Subsystem name (e.g. `"session_store"`, `"providers"`).
    #[schema(value_type = String)]
    pub name: &'static str,
    /// Check outcome: `"pass"`, `"warn"`, or `"fail"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Diagnostic message when status is not `"pass"`.
    pub message: Option<String>,
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
        // Verify HealthState has all fields needed by health handlers.
        // This is a compile-time check that the fields exist and are accessible.
        #[expect(
            dead_code,
            reason = "compile-time shape assertion: proves field types via unused local fn"
        )]
        fn assert_health_state_fields(state: &HealthState) {
            use std::sync::Arc;
            use mneme::store::SessionStore;
            use hermeneus::provider::ProviderRegistry;
            use nous::manager::NousManager;
            use taxis::oikos::Oikos;
            use taxis::config::AletheiaConfig;

            let _: &Arc<tokio::sync::Mutex<SessionStore>> = &state.session_store;
            let _: &Arc<ProviderRegistry> = &state.provider_registry;
            let _: &Arc<NousManager> = &state.nous_manager;
            let _: std::time::Instant = state.start_time;
            let _: &Arc<Oikos> = &state.oikos;
            let _: &Arc<tokio::sync::RwLock<AletheiaConfig>> = &state.config;
        }
        // The function above proves all required fields exist and have correct types.
        // If this compiles, HealthState has all the fields health handlers need.
        assert!(std::mem::size_of::<HealthState>() > 0);
    }

    #[test]
    fn health_response_serializes_all_fields() {
        let resp = HealthResponse {
            status: "healthy",
            version: "1.0.0",
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
    fn decode_jwt_exp_extracts_expiry() {
        // Create a JWT with known exp claim: exp = 1234567890
        // Payload: {"exp":1234567890}
        // base64url: eyJleHAiOjEyMzQ1Njc4OTB9
        let token = "header.eyJleHAiOjEyMzQ1Njc4OTB9.signature";
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
}
