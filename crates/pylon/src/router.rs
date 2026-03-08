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

use tower_http::services::{ServeDir, ServeFile};

use crate::error::ApiError;
use crate::handlers::{config, health, metrics, nous, sessions, webchat};
use crate::middleware::{
    CsrfState, RequestId, enrich_error_response, inject_request_id, record_http_metrics,
    require_csrf_header,
};
use crate::openapi;
use crate::security::SecurityConfig;
use crate::state::AppState;

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: Arc<AppState>, security: &SecurityConfig) -> Router {
    crate::metrics::init();

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
        .route("/sessions/{id}/messages", post(sessions::send_message))
        .route("/sessions/{id}/history", get(sessions::history))
        .route("/events", get(sessions::events))
        .route("/nous", get(nous::list))
        .route("/nous/{id}", get(nous::get_status))
        .route("/nous/{id}/tools", get(nous::tools))
        .route("/config", get(config::get_config))
        .route(
            "/config/{section}",
            get(config::get_section).put(config::update_section),
        );

    let mut router = Router::new()
        .nest("/api/v1", v1)
        // Webchat compatibility endpoints (unversioned)
        .route("/api/sessions/stream", post(webchat::stream))
        .route("/api/agents", get(webchat::agents_list))
        .route("/api/agents/{id}/identity", get(webchat::agent_identity))
        .route("/api/branding", get(webchat::branding))
        .route("/api/auth/mode", get(webchat::auth_mode))
        .route("/api/sessions", get(webchat::sessions_list))
        .route("/api/events", get(webchat::events_sse))
        // Infrastructure
        .route("/api/health", get(health::check))
        .route("/api/docs/openapi.json", get(openapi::openapi_json))
        .route("/metrics", get(metrics::expose));

    // Static file serving for the web UI
    let ui_dist = state
        .oikos
        .root()
        .parent()
        .map(|repo_root| repo_root.join("ui/dist"));

    if let Some(ref dist_path) = ui_dist {
        if dist_path.is_dir() {
            let index = dist_path.join("index.html");
            router = router.nest_service(
                "/ui",
                ServeDir::new(dist_path).fallback(ServeFile::new(index)),
            );
            tracing::info!(path = %dist_path.display(), "mounted web UI static files");
        } else {
            tracing::debug!(path = %dist_path.display(), "ui/dist not found, skipping static serving");
        }
    }

    router = router.fallback(fallback_handler);

    // CSRF protection — inject state and apply middleware
    if security.csrf_enabled {
        let csrf_state = CsrfState {
            header_name: security.csrf_header_name.clone(),
            header_value: security.csrf_header_value.clone(),
        };
        router = router
            .layer(axum::middleware::from_fn(require_csrf_header))
            .layer(axum::Extension(csrf_state));
    }

    // Body size limit
    router = router.layer(DefaultBodyLimit::max(security.body_limit_bytes));

    // Error response enrichment (innermost — body not yet compressed)
    router = router.layer(axum::middleware::from_fn(enrich_error_response));

    // HTTP metrics recording
    router = router.layer(axum::middleware::from_fn(record_http_metrics));

    // Compression
    router = router.layer(CompressionLayer::new());

    // Request tracing — reads RequestId from extensions
    router = router.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<_>| {
                let request_id = request
                    .extensions()
                    .get::<RequestId>()
                    .map_or_else(|| ulid::Ulid::new().to_string(), |r| r.0.clone());
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
                    #[expect(clippy::cast_possible_truncation, reason = "HTTP latency fits in u64")]
                    let duration_ms = latency.as_millis() as u64;
                    tracing::debug!(
                        duration_ms,
                        status = response.status().as_u16(),
                        "request complete"
                    );
                },
            ),
    );

    // Request ID injection (before trace layer so span includes the ID)
    router = router.layer(axum::middleware::from_fn(inject_request_id));

    // CORS
    router = router.layer(build_cors_layer(security));

    // Security response headers (outermost — applied to every response)
    router = apply_security_headers(router, security);

    router.with_state(state)
}

/// Fallback handler for unmatched routes.
///
/// Returns 410 Gone with migration hints for old unversioned `/api/nous`
/// paths. Other unknown paths get 404.
async fn fallback_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path();

    // `/api/nous/*` has no webchat equivalent; hint at v1
    if path.starts_with("/api/nous") {
        let suggestion = path.replacen("/api/", "/api/v1/", 1);
        return (
            StatusCode::GONE,
            axum::Json(serde_json::json!({
                "error": {
                    "code": "api_version_required",
                    "message": format!("This endpoint has moved. Use {suggestion} instead."),
                }
            })),
        )
            .into_response();
    }

    ApiError::NotFound {
        path: path.to_owned(),
        location: snafu::Location::default(),
    }
    .into_response()
}

/// Build a CORS layer from security configuration.
fn build_cors_layer(security: &SecurityConfig) -> CorsLayer {
    let is_permissive =
        security.allowed_origins.is_empty() || security.allowed_origins.iter().any(|o| o == "*");

    if is_permissive {
        return CorsLayer::permissive();
    }

    let origins: Vec<HeaderValue> = security
        .allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("authorization"),
        ])
        .max_age(Duration::from_secs(security.cors_max_age_secs))
}

/// Apply standard security response headers.
fn apply_security_headers(
    router: Router<Arc<AppState>>,
    security: &SecurityConfig,
) -> Router<Arc<AppState>> {
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

    if security.tls_enabled {
        r = r.layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ));
    }

    r
}
