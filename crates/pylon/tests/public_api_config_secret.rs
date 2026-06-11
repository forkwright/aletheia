#![expect(clippy::expect_used, reason = "test assertions use expect")]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use koina::http::API_V1;
use koina::secret::SecretString;
use pylon::router::build_router;

mod common;
use common::{TestEnv, bearer, issue_test_token, permissive_security};

#[tokio::test]
async fn config_put_and_nous_toggle_preserve_signing_key_on_disk() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let signing_key = "synthetic-signing-key-for-secret-write-preservation";

    {
        let mut config = env.state.config.write().await;
        config.gateway.auth.signing_key = Some(SecretString::from(signing_key));
        taxis::loader::write_config(&env.state.oikos, &config).expect("seed config");
    }

    let router = build_router(Arc::clone(&env.state), &permissive_security());
    let token = issue_test_token(&env.state);

    let put_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("{API_V1}/config/maintenance"))
                .header("authorization", bearer(&token))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("build config PUT request"),
        )
        .await
        .expect("config PUT response");
    assert_eq!(put_response.status(), StatusCode::OK);

    let toggle_response = router
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("{API_V1}/nous/syn"))
                .header("authorization", bearer(&token))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"enabled":false}"#))
                .expect("build nous toggle request"),
        )
        .await
        .expect("nous toggle response");
    assert_eq!(toggle_response.status(), StatusCode::OK);

    let config_path = env.state.oikos.config().join("aletheia.toml");
    let persisted = std::fs::read_to_string(&config_path).expect("read persisted config");
    assert!(
        persisted.contains(signing_key),
        "persisted config must contain the raw signing key"
    );
    assert!(
        !persisted.contains("[REDACTED]"),
        "persisted config must not contain SecretString redaction marker"
    );

    let reloaded = taxis::loader::load_config(&env.state.oikos).expect("reload config");
    assert_eq!(
        reloaded
            .gateway
            .auth
            .signing_key
            .as_ref()
            .map(SecretString::expose_secret),
        Some(signing_key)
    );
}
