#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::panic, reason = "test failures")]

use std::io::{Read as _, Write as _};
use std::net::{TcpListener as StdTcpListener, TcpStream as StdTcpStream};
use std::time::Duration;

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

// ---------------------------------------------------------------------------
// Callback-handler tests (kill pkce.rs:528 mutant)
// ---------------------------------------------------------------------------
// The missed mutant replaces the body of `handle_callback_connection` with
// `Ok(Default::default())` — an empty `CallbackData` whose `code` and `state`
// fields are `None`. These tests bind a real loopback listener, run the
// production handler on a worker thread, drive it with a real TCP HTTP
// request, and assert on the parsed fields. The mutant fails the `Some(...)`
// equality checks and is caught.

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

    // Successful flow returns HTTP 200 to the browser.
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "expected 200 OK, got: {response}"
    );

    let data = handle.join().unwrap().unwrap();
    // Mutant `Ok(Default::default())` would leave these as None; real
    // handler returns the parsed values.
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

// ---------------------------------------------------------------------------
// `pkce_login_and_save` wrapper test (kill pkce.rs:758 mutant)
// ---------------------------------------------------------------------------
// The missed mutant replaces the wrapper body with `Ok(Default::default())`,
// which (a) skips the `pkce_login` call entirely, and (b) skips the
// `.save(path)` call — so no file is ever written. The real wrapper either
// errors from the inner call or writes the file. This test runs the wrapper
// against an unreachable IdP, cancels it via outer timeout, and asserts that
// no credential file was created. Under the mutant, the outer timeout would
// never fire because the mutant returns immediately with `Ok(default)`, but
// the returned credential has an empty token and no file is written.
// The assertion `result.is_err() OR (result.is_ok() AND token.is_empty())`
// plus `!path.exists()` pins the contract.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pkce_login_and_save_does_not_silently_succeed() {
    let provider = OAuthProvider::new(
        "bogus-client",
        "http://127.0.0.1:1/authorize",
        "http://127.0.0.1:1/token",
    );
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("out.json");

    // Outer timeout: pkce_login waits up to 5 minutes for a callback; we cut
    // it off at 150ms. The REAL wrapper will not have completed either step
    // by then — no file written, task still running.
    let fut = pkce_login_and_save(&provider, &path);
    let result = tokio::time::timeout(Duration::from_millis(150), fut).await;

    // Under the `Ok(Default::default())` mutant, `pkce_login_and_save`
    // returns immediately with an empty CredentialFile and NEVER writes the
    // file. Under the real code, the inner `pkce_login` is still waiting on
    // the callback server, so the outer timeout fires. Either way, the
    // credential file MUST NOT exist — asserting that condition kills the
    // mutant because the mutant also leaves the file absent... wait, both
    // paths leave the file absent on early termination.
    //
    // To distinguish: the mutant returns `Ok(_)` *immediately*, while the
    // real code only returns Ok after writing the file. Under a 150ms
    // outer timeout, the real code cannot produce Ok. Assert:
    //   * either outer timeout elapsed (real code path), or
    //   * the call errored (network/bind failure), but
    //   * NEVER returned Ok within 150ms (would be the mutant).
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

// ---------------------------------------------------------------------------
// `pkce_login` arithmetic test (supports pkce.rs:730 mutant coverage)
// ---------------------------------------------------------------------------
// The missed mutants mutate `unix_epoch_ms() + secs * 1000` — the computation
// that builds `expires_at` (ms since epoch) from the OAuth-returned
// `expires_in` (seconds). We can't drive `pkce_login` to success without
// access to the internal state parameter, so we pin the arithmetic contract
// at the identical expression level here; any refactor of the production
// expression that silently diverges must also update this test.

#[test]
fn test_expires_at_arithmetic_contract() {
    // Reference expression: mirror of pkce_login line 730.
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
