//! Custom middleware layers for pylon.

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// CSRF protection state stored as a router extension.
#[derive(Debug, Clone)]
pub struct CsrfState {
    pub header_name: String,
    pub header_value: String,
}

/// Middleware that requires a custom header on state-changing requests.
///
/// GET, HEAD, and OPTIONS are exempt. POST, PUT, DELETE, and PATCH must
/// include the configured header with the expected value.
pub async fn require_csrf_header(request: Request, next: Next) -> Response {
    let is_safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );

    if is_safe {
        return next.run(request).await;
    }

    let csrf = request.extensions().get::<CsrfState>().cloned();

    if let Some(csrf) = csrf {
        let header_value = request
            .headers()
            .get(&csrf.header_name)
            .and_then(|v| v.to_str().ok());

        match header_value {
            Some(v) if v == csrf.header_value => next.run(request).await,
            _ => (
                StatusCode::FORBIDDEN,
                serde_json::json!({
                    "error": {
                        "code": "csrf_rejected",
                        "message": "missing or invalid CSRF header"
                    }
                })
                .to_string(),
            )
                .into_response(),
        }
    } else {
        next.run(request).await
    }
}
