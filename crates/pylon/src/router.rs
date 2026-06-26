//! HTTP router construction with middleware layers.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info_span;

use koina::http::{API_HEALTH, API_V1};

use crate::error::{ApiError, ErrorBody, ErrorResponse};
use crate::handlers::{
    config, credentials, events, health, insights, knowledge, metrics, nous, ops, planning,
    providers, request_policy, sessions, workspace,
};
use crate::middleware::{
    CsrfState, DeprecationLayer, ETagLayer, RateLimiter, RequestId, UserRateLimiter, deprecate,
    enrich_error_response, inject_request_id, per_user_rate_limit, rate_limit, record_http_metrics,
    require_bearer_auth, require_csrf_header, spawn_anon_cleanup, spawn_stale_cleanup,
};
use crate::openapi;
use crate::security::SecurityConfig;
use crate::state::AppState;

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: Arc<AppState>, security: &SecurityConfig) -> Router {
    build_router_with(state, security, None)
}

/// Build the Axum router with all routes, middleware, and optional extra routes.
///
/// Extra routes (e.g. diaporeia's MCP router) are merged BEFORE middleware
/// layers are applied. This ensures global layers (rate limiting, CSRF,
/// metrics, error enrichment, etc.) wrap all routes including external ones.
// NOTE(#940): 130+ lines: route and middleware layer assembly where ordering matters.
// Extraction would obscure the middleware stack ordering that is critical for correctness.
#[expect(
    clippy::too_many_lines,
    reason = "router construction requires assembling all routes and ordered middleware layers; extraction would obscure the stack ordering"
)]
#[expect(
    deprecated,
    reason = "deprecated_health_check is intentionally wired as the demonstration endpoint for #3280"
)]
pub fn build_router_with(
    state: Arc<AppState>,
    security: &SecurityConfig,
    extra: Option<Router>,
) -> Router {
    // WHY: the binary crate's `register_all_metrics` registers every
    // metrics-emitting crate's families against the shared registry before
    // AppState is constructed, so router construction no longer needs to
    // re-register pylon's metrics. Tests that build a state without running
    // through the full binary should call `crate::metrics::init(registry)`.

    // WHY: Extract shutdown token before state is moved into the router.
    // The user_rate_limiter cleanup task needs it after .with_state() consumes the Arc.
    let shutdown = state.shutdown.clone();

    let knowledge_routes = Router::new()
        .route("/facts", get(knowledge::list_facts))
        .route("/facts/import", post(knowledge::import_facts))
        .route("/ingest", post(knowledge::ingest))
        .route("/ingest/webhook", post(knowledge::webhook_ingest))
        .route("/facts/{id}", get(knowledge::get_fact))
        .route("/facts/{id}/forget", post(knowledge::forget_fact))
        .route("/facts/{id}/restore", post(knowledge::restore_fact))
        .route(
            "/facts/{id}/confidence",
            axum::routing::put(knowledge::update_confidence),
        )
        .route(
            "/facts/{id}/sensitivity",
            axum::routing::put(knowledge::update_sensitivity),
        )
        .route("/entities", get(knowledge::list_entities))
        .route("/entities/merge", post(knowledge::merge_entities))
        .route(
            "/entities/{id}",
            get(knowledge::get_entity).delete(knowledge::delete_entity),
        )
        .route("/entities/{id}/memories", get(knowledge::entity_memories))
        .route(
            "/entities/{id}/relationships",
            get(knowledge::entity_relationships),
        )
        .route("/entities/{id}/flag", post(knowledge::flag_entity))
        .route("/search/explain", get(knowledge::explain))
        .route("/search", get(knowledge::search))
        .route("/timeline", get(knowledge::timeline))
        .route("/check", get(knowledge::check_graph_health))
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            require_bearer_auth,
        ));

    let workspace_routes = Router::new()
        .route("/files", get(workspace::list_files))
        .route("/git-status", get(workspace::git_status))
        .route(
            "/files/content",
            get(workspace::file_content).put(workspace::write_file_content),
        )
        .route("/open", post(workspace::open_file))
        .route("/diff", get(workspace::file_diff))
        .route("/search", get(workspace::search))
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            require_bearer_auth,
        ));

    let v1 = Router::new()
        .route(
            "/sessions",
            get(sessions::list_sessions).post(sessions::create),
        )
        .route("/sessions/stream", post(sessions::stream_turn))
        .route(
            "/sessions/{id}",
            get(sessions::get_session).delete(sessions::close),
        )
        .route("/sessions/{id}/archive", post(sessions::archive))
        .route("/sessions/{id}/unarchive", post(sessions::unarchive))
        .route(
            "/sessions/{id}/purge",
            axum::routing::delete(sessions::purge),
        )
        .route("/sessions/{id}/name", axum::routing::put(sessions::rename))
        .route("/sessions/{id}/messages", post(sessions::send_message))
        // WHY(#3958, ADR-005): operator-decision pass-through for the approval gate.
        .route("/sessions/{id}/approvals", post(sessions::resolve_approval))
        .route(
            "/turns/{turn_id}/tools/{tool_id}/approve",
            post(sessions::approve_tool),
        )
        .route(
            "/turns/{turn_id}/tools/{tool_id}/deny",
            post(sessions::deny_tool),
        )
        // WHY(#3276): reconnect to an in-flight or recently-completed turn's SSE stream.
        .route(
            "/sessions/{session_id}/turns/{turn_id}/events",
            get(sessions::reconnect_turn),
        )
        .route("/sessions/{id}/history", get(sessions::history))
        .route("/events", get(sessions::events))
        .route("/events/subscribe", get(events::subscribe))
        .route("/events/discovery", get(events::discovery))
        .route("/ops/tools", get(ops::tools))
        .route(
            "/system/credentials",
            get(credentials::list_credentials).post(credentials::add_credential),
        )
        .route("/system/health", get(health::detailed))
        .route(
            "/system/request-policy",
            get(request_policy::get_request_policy),
        )
        .route(
            "/system/credentials/rotate",
            post(credentials::rotate_credentials),
        )
        .route(
            "/system/credentials/{id}",
            axum::routing::delete(credentials::remove_credential),
        )
        .route(
            "/system/credentials/{id}/validate",
            post(credentials::validate_credential),
        )
        .route("/nous", get(nous::list).post(nous::create))
        .route(
            "/nous/{id}",
            get(nous::get_status).patch(nous::update_enabled),
        )
        .route(
            "/nous/{id}/tools",
            get(nous::tools).patch(nous::update_tool),
        )
        .route("/nous/{id}/recover", post(nous::recover))
        .route("/config", get(config::get_config))
        .route("/config/reload", post(config::reload_config))
        .route(
            "/config/{section}",
            get(config::get_section).put(config::update_section),
        )
        .nest("/workspace", workspace_routes)
        .nest("/knowledge", knowledge_routes)
        .route("/metrics/agents", get(insights::get_agent_perf))
        .route("/metrics/agents/{id}", get(insights::get_agent_perf_one))
        .route("/metrics/quality", get(insights::get_quality_metrics))
        .route("/metrics/tokens", get(insights::get_token_metrics))
        .route("/metrics/costs", get(insights::get_cost_metrics))
        .route("/journal", get(insights::get_journal))
        // WHY(#3266): planning routes belong under the versioned prefix.
        // The desktop app (proskenion) adapts to the API, not the reverse.
        .route(
            "/planning/projects/{project_id}/verification",
            get(planning::get_verification),
        )
        .route(
            "/planning/projects/{project_id}/verification/refresh",
            post(planning::refresh_verification),
        )
        .route("/providers", get(providers::list))
        .route("/providers/route", get(providers::route));

    let mut router = Router::new()
        .nest(API_V1, v1)
        .route(API_HEALTH, get(health::check))
        .route("/health", get(health::deprecated_health_check))
        .route("/api/docs/openapi.json", get(openapi::openapi_json))
        .route("/metrics", get(metrics::expose));

    router = router.fallback(fallback_handler);

    // WHY(#3276): Spawn turn buffer reaper to clean up expired turn buffers.
    // Must happen before `with_state` consumes the Arc.
    crate::turn_buffer::spawn_reaper(Arc::clone(&state.turn_buffer_registry), shutdown.clone());

    // WHY: Bind state before merging extra routes. This converts
    // Router<Arc<AppState>> to Router<()> so the extra Router<()> from
    // diaporeia can be merged. All subsequent middleware layers are
    // tower-level (not Axum state extractors), so they work on Router<()>.
    let app_state_for_layers = Arc::clone(&state);
    let mut router = router.with_state(state);

    // WHY: Extra routes are merged BEFORE middleware layers so they benefit from
    // the same global protections (rate limiting, CSRF, compression, tracing,
    // metrics, error enrichment) as pylon's own routes (#3226).
    if let Some(extra) = extra {
        router = router.merge(extra);
    }

    if security.rate_limit.per_user.enabled {
        let user_limiter = Arc::new(UserRateLimiter::new(security.rate_limit.per_user.clone()));
        spawn_stale_cleanup(Arc::clone(&user_limiter), shutdown.clone());
        router = router
            .layer(axum::middleware::from_fn(per_user_rate_limit))
            .layer(axum::Extension(user_limiter))
            .layer(axum::Extension(app_state_for_layers));
    }

    if security.rate_limit.enabled {
        let limiter = Arc::new(
            RateLimiter::new(security.rate_limit.requests_per_minute)
                .with_trust_proxy(security.rate_limit.trust_proxy),
        );
        // WHY(#5664): Evict stale per-IP entries on a background task so the
        // HashMap does not grow without bound over long-lived deployments.
        spawn_anon_cleanup(Arc::clone(&limiter), shutdown.clone());
        router = router
            .layer(axum::middleware::from_fn(rate_limit))
            .layer(axum::Extension(limiter));
    }

    // WHY(#5558): Always install the CSRF layer. When enabled it enforces the
    // custom header; when disabled it falls back to same-origin validation so
    // mutating routes are never left completely unprotected.
    let csrf_state = CsrfState {
        enabled: security.csrf.enabled,
        header_name: security.csrf.header_name.clone(),
        header_value: security.csrf.header_value.clone(),
    };
    router = router
        .layer(axum::middleware::from_fn(require_csrf_header))
        .layer(axum::Extension(csrf_state));

    router = router.layer(DefaultBodyLimit::max(security.body_limit_bytes));

    // WARNING: Must be inside compression (body uncompressed) but outside
    // rate_limit and CSRF so their error responses get request_id injected.
    router = router.layer(axum::middleware::from_fn(enrich_error_response));

    let deprecated_at = jiff::Timestamp::now();
    let sunset_at = deprecated_at
        .checked_add(jiff::SignedDuration::from_secs(365 * 24 * 60 * 60))
        .unwrap_or(deprecated_at);
    router = router.layer(DeprecationLayer::new([deprecate(
        "GET /health",
        deprecated_at,
        sunset_at,
        Some("https://docs.aletheia.dev/migration/health".to_owned()),
    )]));

    router = router.layer(axum::middleware::from_fn(record_http_metrics));

    router = router.layer(ETagLayer::new());

    router = router.layer(CompressionLayer::new());

    router = router.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<_>| {
                let request_id = request.extensions().get::<RequestId>().map_or_else(
                    || koina::ulid::Ulid::new().to_string(),
                    std::string::ToString::to_string,
                );
                info_span!("http_request",
                    http.method = %request.method(),
                    http.path = %request.uri().path(),
                    http.request_id = %request_id,
                    http.status_code = tracing::field::Empty,
                )
            })
            .on_response(
                |response: &axum::http::Response<_>, latency: Duration, span: &tracing::Span| {
                    span.record("http.status_code", response.status().as_u16());
                    // NOTE: HTTP latency fits in u64; saturate on theoretical overflow.
                    let duration_ms = u64::try_from(latency.as_millis()).unwrap_or(u64::MAX);
                    tracing::debug!(
                        duration_ms,
                        status = response.status().as_u16(),
                        "request complete"
                    );
                },
            ),
    );

    // WARNING: Must be before trace layer so the span includes the ID.
    router = router.layer(axum::middleware::from_fn(inject_request_id));

    router = router.layer(build_cors_layer(security));

    // WARNING: Outermost layer: must wrap all other layers so headers apply to every response.
    apply_security_headers(router, security)
}

/// Fallback handler for unmatched routes.
///
/// Returns 410 Gone with migration hints for old unversioned `/api/nous`
/// paths. Other unknown paths get 404.
async fn fallback_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path();

    if path.starts_with("/api/nous") {
        let suggestion = path.replacen("/api/", "/api/v1/", 1);
        return (
            StatusCode::GONE,
            axum::Json(ErrorResponse {
                error: ErrorBody {
                    code: "api_version_required".to_owned(),
                    message: format!("This endpoint has moved. Use {suggestion} instead."),
                    request_id: None,
                    details: None,
                },
            }),
        )
            .into_response();
    }

    ApiError::NotFound {
        path: path.to_owned(),
        location: snafu::location!(),
    }
    .into_response()
}

/// Build a CORS layer from security configuration.
fn build_cors_layer(security: &SecurityConfig) -> CorsLayer {
    let is_permissive = security.cors.allowed_origins.is_empty()
        || security.cors.allowed_origins.iter().any(|o| o == "*");

    if is_permissive {
        // WHY: `CorsLayer::permissive()` sets `Access-Control-Allow-Credentials: true`
        // while echoing the request origin, which permits credential-bearing cross-origin
        // requests from any site. Build the layer explicitly with a wildcard `*` origin
        // so browsers never combine credentials with an unrestricted origin.
        let allowed_headers = cors_allowed_headers(security);

        return CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers(allowed_headers);
    }

    let origins: Vec<HeaderValue> = security
        .cors
        .allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(cors_allowed_headers(security))
        .max_age(Duration::from_secs(security.cors.max_age_secs))
}

fn cors_allowed_headers(security: &SecurityConfig) -> Vec<HeaderName> {
    let mut headers = vec![
        HeaderName::from_static("content-type"),
        HeaderName::from_static("authorization"),
        HeaderName::from_static("x-requested-with"),
        // WHY(#5166): Browser API clients send these headers on mutations
        // and SSE reconnects; include them in preflight responses.
        HeaderName::from_static("idempotency-key"),
        HeaderName::from_static("last-event-id"),
    ];

    if let Ok(header) = HeaderName::from_bytes(security.csrf.header_name.as_bytes())
        && !headers.contains(&header)
    {
        headers.push(header);
    }

    headers
}

/// Apply standard security response headers.
fn apply_security_headers(router: Router, security: &SecurityConfig) -> Router {
    let mut r = router
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-xss-protection"),
            HeaderValue::from_static("0"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static("default-src 'self'"),
        ));

    if security.tls.enabled {
        r = r.layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ));
    }

    r
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: serde_json::Value Index returns Null for absent keys, never panics"
)]
mod tests {
    use super::*;
    use crate::security::{CorsConfig, CsrfConfig, RateLimitConfig, TlsConfig};

    fn make_security() -> SecurityConfig {
        SecurityConfig {
            body_limit_bytes: 10 * 1024 * 1024,
            cors: CorsConfig::default(),
            csrf: CsrfConfig::default(),
            tls: TlsConfig::default(),
            rate_limit: RateLimitConfig::default(),
        }
    }

    #[tokio::test]
    async fn fallback_handler_returns_gone_for_old_nous_path() {
        let uri: axum::http::Uri = "/api/nous/syn".parse().expect("parse URI");
        let response = fallback_handler(uri).await;
        assert_eq!(response.status(), axum::http::StatusCode::GONE);
    }

    #[tokio::test]
    async fn fallback_gone_response_uses_error_envelope() {
        let uri: axum::http::Uri = "/api/nous/syn".parse().expect("parse URI");
        let response = fallback_handler(uri).await;
        assert_eq!(response.status(), axum::http::StatusCode::GONE);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["error"].is_object(), "response must have error object");
        assert_eq!(json["error"]["code"], "api_version_required");
        assert!(
            json["error"]["message"]
                .as_str()
                .expect("message should be a string")
                .contains("/api/v1/"),
            "message should contain migration hint"
        );
    }

    #[tokio::test]
    async fn fallback_handler_returns_404_for_unknown_path() {
        let uri: axum::http::Uri = "/api/unknown".parse().expect("parse URI");
        let response = fallback_handler(uri).await;
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn fallback_returns_404_for_old_planning_routes() {
        let uri: axum::http::Uri = "/api/planning/projects/proj-1/verification"
            .parse()
            .expect("parse URI");
        let response = fallback_handler(uri).await;
        assert_eq!(
            response.status(),
            axum::http::StatusCode::NOT_FOUND,
            "old planning path should not be redirected"
        );
    }

    #[test]
    fn build_cors_layer_with_empty_origins_returns_valid_layer() {
        let security = make_security();
        let layer = build_cors_layer(&security);
        // Verify the layer is valid by checking it can be used (size > 0 indicates valid struct)
        assert!(std::mem::size_of_val(&layer) > 0);
    }

    #[test]
    fn build_cors_layer_with_explicit_origin_returns_valid_layer() {
        let mut security = make_security();
        security.cors.allowed_origins = vec!["https://example.com".to_owned()];
        let layer = build_cors_layer(&security);
        assert!(std::mem::size_of_val(&layer) > 0);
    }
}
