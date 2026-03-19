#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use axum::http::StatusCode;

// ── ApiError status codes ───────────────────────────────────────────────────

#[test]
fn api_error_session_not_found_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::SessionNotFound {
        id: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_nous_not_found_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::NousNotFound {
        id: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_bad_request_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::BadRequest {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn api_error_internal_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Internal {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn api_error_unauthorized_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Unauthorized {
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn api_error_rate_limited_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::RateLimited {
        retry_after_secs: 1,
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn api_error_forbidden_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Forbidden {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[test]
fn api_error_service_unavailable_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ServiceUnavailable {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn api_error_validation_failed_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ValidationFailed {
        errors: vec!["field required".to_owned()],
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn api_error_rate_limited_includes_details() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::RateLimited {
        retry_after_secs: 5,
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let body = rt.block_on(async {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    });
    assert_eq!(body["error"]["details"]["retry_after_secs"], 5);
}

#[test]
fn api_error_validation_failed_includes_errors() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ValidationFailed {
        errors: vec!["field1 required".to_owned(), "field2 invalid".to_owned()],
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let body = rt.block_on(async {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    });
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
}

// ── deep_merge ──────────────────────────────────────────────────────────────

#[test]
fn deep_merge_overwrites_scalar() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"key": "old"});
    let patch = serde_json::json!({"key": "new"});
    deep_merge(&mut base, patch);
    assert_eq!(base["key"], "new");
}

#[test]
fn deep_merge_adds_missing_keys() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"existing": 1});
    let patch = serde_json::json!({"new_key": 2});
    deep_merge(&mut base, patch);
    assert_eq!(base["existing"], 1);
    assert_eq!(base["new_key"], 2);
}

#[test]
fn deep_merge_recurses_objects() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"nested": {"a": 1, "b": 2}});
    let patch = serde_json::json!({"nested": {"b": 3, "c": 4}});
    deep_merge(&mut base, patch);
    assert_eq!(base["nested"]["a"], 1);
    assert_eq!(base["nested"]["b"], 3);
    assert_eq!(base["nested"]["c"], 4);
}

#[test]
fn deep_merge_replaces_non_object_with_object() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"key": "string"});
    let patch = serde_json::json!({"key": {"nested": true}});
    deep_merge(&mut base, patch);
    assert_eq!(base["key"]["nested"], true);
}
