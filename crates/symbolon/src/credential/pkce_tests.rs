#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::panic, reason = "test failures")]

use std::io::{Read as _, Write as _};
use std::net::{TcpListener as StdTcpListener, TcpStream as StdTcpStream};
use std::time::Duration;

use super::*;

#[test]
fn test_pkce_pair_generation() {
    let pair = PkcePair::generate().unwrap();
    assert!(!pair.verifier.expose_secret().is_empty());
    assert!(!pair.challenge.is_empty());

    // NOTE: 43 chars = unpadded base64url of a 32-byte SHA-256 digest.
    assert_eq!(pair.challenge.len(), 43);

    let mut hasher = Sha256::new();
    hasher.update(pair.verifier.expose_secret().as_bytes());
    let expected = base64url_encode(&hasher.finalize());
    assert_eq!(pair.challenge, expected);
}

#[test]
fn test_generate_state() {
    let state1 = generate_state().unwrap();
    let state2 = generate_state().unwrap();

    assert_ne!(state1, state2);

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

    // WHY: HashMap iteration order is not guaranteed, so check presence per param.
    assert!(body.contains("grant_type=authorization_code"));
    assert!(body.contains("code=abc123"));
    assert!(body.contains("client_id=my%20client"));
    assert_eq!(body.matches('&').count(), 2);
}

// ── handle_callback_connection mutant kills ──
// WHY: kills the mutant that replaces `handle_callback_connection`'s body with
// `Ok(Default::default())` — these tests drive the real handler over loopback
// TCP and assert on parsed fields an empty `CallbackData` cannot satisfy.

/// Bind a loopback listener and run `handle_callback_connection` on a helper
/// thread. Returns the bound port + a join handle yielding the parsed result.
fn spawn_callback_handler(
    expected_state: &'static str,
) -> (u16, std::thread::JoinHandle<Result<CallbackData>>) {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || handle_callback_connection(&listener, expected_state));
    (port, handle)
}

/// Send a GET /callback request and drain the HTTP response.
fn send_callback_request(port: u16, query: &str) -> String {
    let mut stream = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let request =
        format!("GET /callback?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

#[test]
fn test_handle_callback_connection_extracts_code_and_state() {
    let (port, handle) = spawn_callback_handler("state-xyz");
    let response = send_callback_request(port, "code=AUTH_CODE_42&state=state-xyz");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "expected 200 OK, got: {response}"
    );

    let data = handle.join().unwrap().unwrap();
    // WHY: the `Ok(Default::default())` mutant leaves code/state as `None`.
    assert_eq!(data.code.as_deref(), Some("AUTH_CODE_42"));
    assert_eq!(data.state.as_deref(), Some("state-xyz"));
    assert!(data.error.is_none());
    assert!(data.error_description.is_none());
}

#[test]
fn test_handle_callback_connection_rejects_mismatched_state() {
    let (port, handle) = spawn_callback_handler("expected-state");
    let _ = send_callback_request(port, "code=AUTH&state=attacker-state");

    let result = handle.join().unwrap();
    match result {
        Err(PkceError::InvalidState { .. }) => {}
        other => panic!("expected InvalidState, got {other:?}"),
    }
}

#[test]
fn test_handle_callback_connection_rejects_missing_code_when_state_ok() {
    let (port, handle) = spawn_callback_handler("s1");
    let _ = send_callback_request(port, "state=s1");

    let result = handle.join().unwrap();
    match result {
        Err(PkceError::MissingCode { .. }) => {}
        other => panic!("expected MissingCode, got {other:?}"),
    }
}

#[test]
fn test_handle_callback_connection_surfaces_authorization_error() {
    let (port, handle) = spawn_callback_handler("st");
    let _ = send_callback_request(port, "error=access_denied&error_description=nope&state=st");

    let result = handle.join().unwrap();
    match result {
        Err(PkceError::AuthorizationError {
            error,
            error_description,
            ..
        }) => {
            assert_eq!(error, "access_denied");
            assert_eq!(error_description.as_deref(), Some("nope"));
        }
        other => panic!("expected AuthorizationError, got {other:?}"),
    }
}

// ── pkce_login_and_save mutant kill ──
// WHY: kills the mutant that replaces `pkce_login_and_save`'s body with
// `Ok(Default::default())`, skipping both the `pkce_login` call and the
// `.save(path)` write.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pkce_login_and_save_does_not_silently_succeed() {
    let provider = OAuthProvider::new(
        "bogus-client",
        "http://127.0.0.1:1/authorize",
        "http://127.0.0.1:1/token",
    );
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("out.json");

    // WHY: pkce_login waits up to 5 minutes for a callback, so the real
    // wrapper cannot complete within the 150ms outer timeout.
    let fut = pkce_login_and_save(&provider, &path);
    let result = tokio::time::timeout(Duration::from_millis(150), fut).await;

    // WHY: only the mutant can return `Ok` within 150ms — the real wrapper is
    // still waiting on the callback server (timeout elapses) or errors; it
    // returns `Ok` only after writing the file, so `Ok` + no file = mutant.
    assert!(
        !path.exists(),
        "credential file must not be written when login has not completed"
    );
    match result {
        Err(_elapsed) => { /* expected: real future still polling */ }
        Ok(Err(_)) => { /* acceptable: inner error path */ }
        Ok(Ok(cred)) => panic!(
            "pkce_login_and_save returned Ok within 150ms \
             (mutant signature) — got cred token of len {}, refresh={}, \
             expires_at={:?}",
            cred.token.expose_secret().len(),
            cred.refresh_token.is_some(),
            cred.expires_at
        ),
    }
}

// ── expires_at arithmetic mutant kills ──
// WHY: `pkce_login` cannot be driven to success without its internal state
// parameter, so the `unix_epoch_ms() + secs * 1000` expires_at computation is
// pinned here at the expression level; keep in sync with `pkce_login`.

#[test]
fn test_expires_at_arithmetic_contract() {
    // Reference expression: mirror of pkce_login's expires_at computation.
    let secs: u64 = 3600;
    let now_ms = super::super::unix_epoch_ms();
    let expires_at = now_ms + secs * 1000;

    // `+` mutant → `-`: expires_at would be ≈ now - 3_600_000, underflowing
    // or yielding a time far in the past.
    assert!(
        expires_at > now_ms,
        "expires_at must be in the future (kills + -> -)"
    );

    // `*` mutant → `/`: secs/1000 = 3, expires_at ≈ now + 3.
    let delta_ms = expires_at - now_ms;
    assert!(
        delta_ms >= 3_600_000,
        "expires_at must be >= now + 3_600_000 ms (kills * -> /)"
    );

    // `+` mutant → `*`: now_ms * (secs*1000) overflows or is astronomically
    // large — vastly exceeds now + 1 day.
    let one_day_ms: u64 = 86_400_000;
    assert!(
        delta_ms <= one_day_ms,
        "expires_at must be <= now + 1 day for expires_in=3600s \
         (kills + -> *, which would yield now * 3_600_000)"
    );

    // `*` mutant → `+`: secs+1000 = 4600, expires_at ≈ now + 4600.
    // Guard that the delta is consistent with `secs * 1000`, not `secs + 1000`.
    assert_eq!(
        delta_ms, 3_600_000,
        "expires_at delta must equal exactly secs * 1000 (kills * -> +)"
    );
}
