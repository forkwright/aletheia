#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::*;

#[test]
fn test_pkce_pair_generation() {
    let pair = PkcePair::generate().unwrap();
    // Verifier should be base64url encoded
    assert!(!pair.verifier.expose_secret().is_empty());
    assert!(!pair.challenge.is_empty());

    // Challenge should be base64url-encoded SHA256 hash (43 chars)
    assert_eq!(pair.challenge.len(), 43);

    // Verify challenge is correct
    let mut hasher = Sha256::new();
    hasher.update(pair.verifier.expose_secret().as_bytes());
    let expected = base64url_encode(&hasher.finalize());
    assert_eq!(pair.challenge, expected);
}

#[test]
fn test_generate_state() {
    let state1 = generate_state().unwrap();
    let state2 = generate_state().unwrap();

    // States should be unique
    assert_ne!(state1, state2);

    // Should be non-empty
    assert!(!state1.is_empty());
    assert!(!state2.is_empty());
}

#[test]
fn test_url_encode() {
    assert_eq!(url_encode("hello world"), "hello%20world");
    assert_eq!(url_encode("foo/bar"), "foo%2Fbar");
    assert_eq!(url_encode("test@example.com"), "test%40example.com");
    assert_eq!(url_encode("safe-_.~"), "safe-_.~");
}

#[test]
fn test_url_decode() {
    assert_eq!(url_decode("hello%20world"), Some("hello world".to_string()));
    assert_eq!(url_decode("foo%2Fbar"), Some("foo/bar".to_string()));
    assert_eq!(
        url_decode("test%40example.com"),
        Some("test@example.com".to_string())
    );
}

#[test]
fn test_html_escape() {
    assert_eq!(html_escape("<script>"), "&lt;script&gt;");
    assert_eq!(html_escape("foo & bar"), "foo &amp; bar");
    assert_eq!(html_escape("\"test\""), "&quot;test&quot;");
}

#[test]
fn test_build_authorization_url() {
    let provider = OAuthProvider::new(
        "test-client-id",
        "https://example.com/auth",
        "https://example.com/token",
    )
    .with_scope("read")
    .with_scope("write");

    let pkce = PkcePair::generate().unwrap();
    let state = "test-state";

    let url = build_authorization_url(&provider, &pkce, state, 8080);

    assert!(url.contains("response_type=code"));
    assert!(url.contains("client_id=test-client-id"));
    assert!(url.contains("code_challenge="));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=test-state"));
    assert!(url.contains("scope=read%20write"));
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fcallback"));
}

#[test]
fn test_build_authorization_url_custom_redirect() {
    let provider = OAuthProvider::new(
        "test-client-id",
        "https://example.com/auth",
        "https://example.com/token",
    )
    .with_redirect_uri("http://localhost:3000/callback");

    let pkce = PkcePair::generate().unwrap();
    let state = "test-state";

    let url = build_authorization_url(&provider, &pkce, state, 8080);

    assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A3000%2Fcallback"));
}

#[test]
fn test_build_form_body() {
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", "abc123");
    params.insert("client_id", "my client");

    let body = build_form_body(&params);

    // HashMap iteration order is not guaranteed, so check for presence of each param
    assert!(body.contains("grant_type=authorization_code"));
    assert!(body.contains("code=abc123"));
    assert!(body.contains("client_id=my%20client"));
    assert_eq!(body.matches('&').count(), 2);
}
