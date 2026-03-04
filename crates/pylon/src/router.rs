//! HTTP router construction with middleware layers.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::Router;
use axum::routing::{get, post};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info_span;

use crate::handlers::{health, nous, sessions};
use crate::middleware::{CsrfState, require_csrf_header};
use crate::security::SecurityConfig;
use crate::state::AppState;

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: Arc<AppState>, security: &SecurityConfig) -> Router {
    let mut router = Router::new()
        .route("/api/health", get(health::check))
        .route("/api/sessions", post(sessions::create))
        .route(
            "/api/sessions/{id}",
            get(sessions::get_session).delete(sessions::close),
        )
        .route("/api/sessions/{id}/messages", post(sessions::send_message))
        .route("/api/sessions/{id}/history", get(sessions::history))
        .route("/api/nous", get(nous::list))
        .route("/api/nous/{id}", get(nous::get_status))
        .route("/api/nous/{id}/tools", get(nous::tools));

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

    // Compression
    router = router.layer(CompressionLayer::new());

    // Request tracing
    router = router.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<_>| {
                let request_id = ulid::Ulid::new().to_string();
                info_span!("http_request",
                    http.method = %request.method(),
                    http.path = %request.uri().path(),
                    http.request_id = %request_id,
                    http.status_code = tracing::field::Empty,
                )
            })
            .on_response(
                |response: &axum::http::Response<_>,
                 latency: Duration,
                 span: &tracing::Span| {
                    span.record("http.status_code", response.status().as_u16());
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "HTTP latency fits in u64"
                    )]
                    let duration_ms = latency.as_millis() as u64;
                    tracing::debug!(
                        duration_ms,
                        status = response.status().as_u16(),
                        "request complete"
                    );
                },
            ),
    );

    // CORS
    router = router.layer(build_cors_layer(security));

    // Security response headers (outermost — applied to every response)
    router = apply_security_headers(router, security);

    router.with_state(state)
}

/// Build a CORS layer from security configuration.
fn build_cors_layer(security: &SecurityConfig) -> CorsLayer {
    let is_permissive = security.allowed_origins.is_empty()
        || security.allowed_origins.iter().any(|o| o == "*");

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
fn apply_security_headers(router: Router<Arc<AppState>>, security: &SecurityConfig) -> Router<Arc<AppState>> {
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
