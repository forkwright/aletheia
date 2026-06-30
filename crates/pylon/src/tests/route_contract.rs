//! Cross-crate route contracts for first-party clients.

use skene::api::routes::SKENE_CLIENT_ROUTE_CONTRACTS;

#[test]
fn skene_client_routes_exist_in_pylon_openapi() {
    let spec = crate::openapi::openapi_value_for_auth_mode("token");
    let paths = spec
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("OpenAPI spec must contain paths");

    for contract in SKENE_CLIENT_ROUTE_CONTRACTS {
        let Some(path_item) = paths.get(contract.path_template) else {
            panic!(
                "skene client route missing from pylon OpenAPI: {} {}",
                contract.method, contract.path_template
            );
        };
        let method = contract.method.to_ascii_lowercase();
        assert!(
            path_item.get(&method).is_some(),
            "skene client route method missing from pylon OpenAPI: {} {}",
            contract.method,
            contract.path_template
        );
    }
}

#[test]
fn gateway_client_session_replay_route_exists_in_pylon_openapi() {
    let spec = crate::openapi::openapi_value_for_auth_mode("token");
    let paths = spec
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("OpenAPI spec must contain paths");

    assert!(
        paths
            .get("/api/v1/sessions/{id}/replay")
            .and_then(|path| path.get("get"))
            .is_some(),
        "gateway client session replay route must be present in OpenAPI"
    );
    assert_eq!(
        crate::client::routes::session_replay("session/needs encoding"),
        "/api/v1/sessions/session%2Fneeds%20encoding/replay"
    );
}
