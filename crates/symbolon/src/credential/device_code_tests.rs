#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

#[test]
fn test_device_oauth_provider_builder() {
    let provider = DeviceOAuthProvider::new(
        "test-client",
        "https://example.com/auth",
        "https://example.com/token",
        "https://example.com/device",
    )
    .with_scope("read")
    .with_scope("write")
    .with_redirect_uri("http://localhost/callback");

    assert_eq!(provider.base.client_id, "test-client");
    assert_eq!(
        provider.device_authorization_url,
        "https://example.com/device"
    );
    assert_eq!(provider.base.scopes, vec!["read", "write"]);
    assert_eq!(
        provider.base.redirect_uri,
        Some("http://localhost/callback".to_string())
    );
}

// WHY: RFC 8628 §3.2 specifies `expires_in` MAY be omitted, and clients SHOULD
// fall back to a sensible default. 30 minutes matches common IdP behavior and
// is a contractual value — a mutation to 0 or 1 would collapse the device-code
// window to useless. Asserting the exact integer kills arithmetic mutants.
#[test]
fn test_default_expires_in_is_thirty_minutes() {
    assert_eq!(default_expires_in(), 1800);
    // Guard against 0 / 1 mutants being considered "close enough".
    assert!(default_expires_in() > 1);
    // Guard against off-by-one multiplication mutants (e.g. 1800*2).
    assert!(default_expires_in() < 3600);
}

// WHY: RFC 8628 §3.5 mandates a default polling interval of 5 seconds when the
// server does not specify one. Mutating this to 0 creates a tight loop that
// could be classified as a denial-of-service against the token endpoint —
// asserting the exact value kills `-> 0` and `-> 1` mutants.
#[test]
fn test_default_interval_is_five_seconds() {
    assert_eq!(default_interval(), 5);
    // Guard: 0 would be a tight poll loop; 1 would DoS the IdP.
    assert!(default_interval() >= 5);
    assert!(default_interval() < 60);
}

#[test]
fn test_build_form_body_str() {
    let mut params = HashMap::new();
    params.insert(
        "grant_type".to_string(),
        "urn:ietf:params:oauth:grant-type:device_code".to_string(),
    );
    params.insert("device_code".to_string(), "abc123".to_string());
    params.insert("client_id".to_string(), "my client".to_string());

    let body = build_form_body_str(&params);

    // HashMap iteration order is not guaranteed, so check for presence of each param
    assert!(body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"));
    assert!(body.contains("device_code=abc123"));
    assert!(body.contains("client_id=my%20client"));
    assert_eq!(body.matches('&').count(), 2);
}

#[test]
fn test_build_form_body_str_empty() {
    let params = HashMap::new();
    let body = build_form_body_str(&params);
    assert!(body.is_empty());
}

#[test]
fn test_build_form_body_str_single_param() {
    let mut params = HashMap::new();
    params.insert("key".to_string(), "value".to_string());
    let body = build_form_body_str(&params);
    assert_eq!(body, "key=value");
}

// WHY: the `DeviceAuthorizationResponse::expires_in` field uses `#[serde(default =
// "default_expires_in")]`. When an IdP omits the field the default is wired in
// via this path — asserting round-trip parsing kills mutants that replace the
// default wiring with `Default::default()` (which would yield 0).
#[test]
fn test_device_authorization_response_applies_default_expires_in() {
    let json = r#"{
        "device_code": "abc",
        "user_code": "USER-CODE",
        "verification_uri": "https://idp.example.com/device",
        "interval": 5
    }"#;
    let resp: DeviceAuthorizationResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.expires_in, 1800);
    assert_eq!(resp.interval, 5);
}

// WHY: mirror of the expires_in coverage — asserts the `default_interval`
// wiring in the serde attribute actually fires and returns 5, not 0.
#[test]
fn test_device_authorization_response_applies_default_interval() {
    let json = r#"{
        "device_code": "abc",
        "user_code": "USER-CODE",
        "verification_uri": "https://idp.example.com/device",
        "expires_in": 600
    }"#;
    let resp: DeviceAuthorizationResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.interval, 5);
    assert_eq!(resp.expires_in, 600);
}
