//! OAuth 2.0 PKCE Authorization Code Flow (RFC 7636 + RFC 8252).
//!
//! This module implements the Proof Key for Code Exchange (PKCE) extension
//! for the OAuth 2.0 Authorization Code flow, suitable for native applications.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use aletheia_koina::secret::SecretString;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRngCore as _;
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use tracing::{debug, info};

use super::file_ops::CredentialFile;

/// Errors from PKCE authentication flow.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum PkceError {
    /// Failed to generate cryptographic random data.
    #[snafu(display("random generation failed"))]
    RandomGeneration {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to bind local callback server.
    #[snafu(display("failed to bind callback server: {source}"))]
    ServerBind {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to accept incoming connection.
    #[snafu(display("failed to accept connection: {source}"))]
    AcceptConnection {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to read HTTP request.
    #[snafu(display("failed to read request: {source}"))]
    ReadRequest {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to send HTTP response.
    #[snafu(display("failed to send response: {source}"))]
    SendResponse {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HTTP request failed.
    #[snafu(display("HTTP request failed: {source}"))]
    HttpRequest {
        /// Underlying HTTP error.
        source: reqwest::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// OAuth error response from token endpoint.
    #[snafu(display("OAuth error: {error}"))]
    OAuthError {
        /// OAuth error code.
        error: String,
        /// OAuth error description.
        error_description: Option<String>,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid state parameter (CSRF protection).
    #[snafu(display("invalid state parameter - possible CSRF attack"))]
    InvalidState {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Missing authorization code in callback.
    #[snafu(display("missing authorization code in callback"))]
    MissingCode {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Callback received an error.
    #[snafu(display("authorization error: {error}"))]
    AuthorizationError {
        /// OAuth error code.
        error: String,
        /// OAuth error description.
        error_description: Option<String>,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Timeout waiting for callback.
    #[snafu(display("timeout waiting for authorization callback"))]
    Timeout {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse token response.
    #[snafu(display("failed to parse token response: {source}"))]
    ParseResponse {
        /// JSON parse error.
        source: serde_json::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to save credential file.
    #[snafu(display("failed to save credentials: {source}"))]
    SaveCredential {
        /// IO error from save operation.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result type for PKCE operations.
pub type Result<T> = std::result::Result<T, PkceError>;

/// OAuth 2.0 provider configuration for PKCE flow.
// kanon:ignore RUST/plain-string-secret
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OAuthProvider {
    /// OAuth client identifier.
    pub client_id: String,
    /// Authorization endpoint URL.
    pub authorization_url: String,
    /// Token endpoint URL.
    pub token_url: String, // kanon:ignore RUST/plain-string-secret
    /// Requested OAuth scopes.
    pub scopes: Vec<String>,
    /// Optional redirect URI (defaults to localhost).
    pub redirect_uri: Option<String>,
}

impl OAuthProvider {
    /// Create a new OAuth provider configuration.
    #[must_use]
    pub fn new(
        client_id: impl Into<String>,
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            authorization_url: authorization_url.into(),
            token_url: token_url.into(),
            scopes: Vec::new(),
            redirect_uri: None,
        }
    }

    /// Add a scope to the provider configuration.
    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Set the redirect URI.
    #[must_use]
    pub fn with_redirect_uri(mut self, uri: impl Into<String>) -> Self {
        self.redirect_uri = Some(uri.into());
        self
    }
}

/// PKCE code verifier (plaintext) and challenge (hashed).
#[derive(Debug)]
struct PkcePair {
    verifier: SecretString,
    challenge: String,
}

impl PkcePair {
    /// Generate a new PKCE pair with the recommended 128 bytes of randomness.
    ///
    /// The verifier is base64url-encoded random bytes (resulting in ~128-172 chars).
    /// The challenge is BASE64URL(SHA256(verifier)).
    fn generate() -> Result<Self> {
        // WHY: RFC 7636 recommends minimum 43 chars, max 128 chars for verifier.
        // We generate 128 bytes of randomness which becomes ~171 chars base64url.
        let mut rng = rand::rngs::OsRng;
        let mut verifier_bytes = vec![0u8; 128];
        rng.try_fill_bytes(&mut verifier_bytes)
            .map_err(|_| PkceError::RandomGeneration {
                location: snafu::location!(),
            })?;
        let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(hash);

        Ok(Self {
            verifier: SecretString::from(verifier),
            challenge,
        })
    }
}

/// OAuth token response from the token endpoint.
#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: SecretString,
    #[serde(default)]
    refresh_token: Option<SecretString>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    /// Token type (typically "Bearer").
    #[serde(default)]
    #[expect(dead_code, reason = "field provided by OAuth but not currently used")]
    token_type: String, // kanon:ignore RUST/plain-string-secret
}

/// OAuth error response from the token endpoint.
#[derive(Debug, serde::Deserialize)]
struct TokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Callback data received from the OAuth redirect.
#[derive(Debug, Default)]
struct CallbackData {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Generate a cryptographically random state parameter for CSRF protection.
fn generate_state() -> Result<String> {
    let mut rng = rand::rngs::OsRng;
    let mut bytes = vec![0u8; 32];
    rng.try_fill_bytes(&mut bytes)
        .map_err(|_| PkceError::RandomGeneration {
            location: snafu::location!(),
        })?;
    Ok(URL_SAFE_NO_PAD.encode(&bytes))
}

/// Simple URL encoding for query parameters.
pub(crate) fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(char::from(byte));
            }
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }
    result
}

/// Simple HTML escape for user-provided content.
fn html_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#x27;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

/// Simple URL decode for query parameters.
fn url_decode(s: &str) -> Option<String> {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next()?;
            let lo = chars.next()?;
            let hex = u8::from_str_radix(&format!("{hi}{lo}"), 16).ok()?;
            result.push(char::from(hex));
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }

    Some(result)
}

/// Build the authorization URL with PKCE parameters.
fn build_authorization_url(
    provider: &OAuthProvider,
    pkce: &PkcePair,
    state: &str,
    redirect_port: u16,
) -> String {
    let redirect_uri = provider
        .redirect_uri
        .clone()
        .unwrap_or_else(|| format!("http://127.0.0.1:{redirect_port}/callback"));

    let mut url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        provider.authorization_url,
        url_encode(&provider.client_id),
        url_encode(&redirect_uri),
        url_encode(&pkce.challenge),
        url_encode(state)
    );

    if !provider.scopes.is_empty() {
        url.push_str("&scope=");
        url.push_str(&url_encode(&provider.scopes.join(" ")));
    }

    url
}

/// Parse callback data from the HTTP request.
fn parse_callback_request(reader: &mut BufReader<&TcpStream>) -> Result<Option<CallbackData>> {
    let mut first_line = String::new();
    reader
        .read_line(&mut first_line)
        .context(ReadRequestSnafu)?;

    debug!("callback request: {}", first_line.trim());

    // Parse the request line: GET /callback?code=...&state=... HTTP/1.1
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(None);
    }

    let path = parts[1];

    // Extract query string
    let query_start = path.find('?');
    // SAFETY: i is from find('?'), so i+1 is a valid byte boundary
    #[expect(clippy::string_slice, reason = "i from find('?'), i+1 is valid boundary")]
    let query = query_start.map(|i| &path[i + 1..]).unwrap_or("");

    // Parse query parameters
    let mut data = CallbackData::default();

    for pair in query.split('&') {
        if let Some(eq_pos) = pair.find('=') {
            // SAFETY: eq_pos from find('='), always a valid byte boundary
            #[expect(clippy::string_slice, reason = "eq_pos from find('='), valid boundary")]
            let key = &pair[..eq_pos];
            #[expect(clippy::string_slice, reason = "eq_pos from find('='), eq_pos+1 is valid")]
            let value = url_decode(&pair[eq_pos + 1..]).unwrap_or_default();

            match key {
                "code" => data.code = Some(value),
                "state" => data.state = Some(value),
                "error" => data.error = Some(value),
                "error_description" => data.error_description = Some(value),
                _ => {}
            }
        }
    }

    // Read remaining headers (to consume the request)
    let mut header_line = String::new();
    loop {
        header_line.clear();
        if reader
            .read_line(&mut header_line)
            .context(ReadRequestSnafu)?
            == 0
        {
            break;
        }
        if header_line.trim().is_empty() {
            break;
        }
    }

    Ok(Some(data))
}

/// Send success response to the browser.
fn send_success_response(stream: &mut TcpStream) -> Result<()> {
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/html; charset=utf-8\r\n",
        "Connection: close\r\n",
        "\r\n",
        "<!DOCTYPE html>",
        "<html>",
        "<head><title>Authentication Successful</title></head>",
        "<body style='font-family:sans-serif;max-width:600px;margin:50px auto;text-align:center;'>",
        "<h1>Authentication Successful</h1>",
        "<p>You can close this window and return to the CLI.</p>",
        "</body>",
        "</html>"
    );

    stream
        .write_all(response.as_bytes())
        .context(SendResponseSnafu)?;
    Ok(())
}

/// Send error response to the browser.
fn send_error_response(stream: &mut TcpStream, message: &str) -> Result<()> {
    let escaped_message = html_escape(message);
    let body = format!(
        concat!(
            "<!DOCTYPE html>",
            "<html>",
            "<head><title>Authentication Failed</title></head>",
            "<body style='font-family:sans-serif;max-width:600px;margin:50px auto;text-align:center;color:#d32f2f;'>",
            "<h1>Authentication Failed</h1>",
            "<p>{}</p>",
            "</body>",
            "</html>"
        ),
        escaped_message
    );

    let response = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n{body}"
    );

    stream
        .write_all(response.as_bytes())
        .context(SendResponseSnafu)?;
    Ok(())
}

/// Start a temporary local HTTP server to receive the OAuth callback.
///
/// Returns the bound port number.
fn start_callback_server(
    expected_state: &str,
) -> Result<(u16, tokio::sync::oneshot::Receiver<Result<CallbackData>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").context(ServerBindSnafu)?;
    let port = listener
        .local_addr()
        .map_err(|e| PkceError::ServerBind {
            source: e,
            location: snafu::location!(),
        })?
        .port();

    let expected_state = expected_state.to_owned();
    let (tx, rx) = tokio::sync::oneshot::channel();

    std::thread::spawn(move || {
        let result = handle_callback_connection(&listener, &expected_state);
        let _ = tx.send(result);
    });

    Ok((port, rx))
}

/// Handle a single callback connection.
fn handle_callback_connection(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<CallbackData> {
    // Set a timeout for accepting connections
    listener
        .set_nonblocking(false)
        .expect("should be able to set blocking mode");

    let (stream, addr) = listener.accept().context(AcceptConnectionSnafu)?;
    info!(addr = %addr, "received OAuth callback");

    let mut reader = BufReader::new(&stream);
    let callback_data = parse_callback_request(&mut reader)?;

    let mut stream = stream;

    match callback_data {
        Some(data) => {
            // Validate state parameter
            match &data.state {
                Some(state) if state == expected_state => {
                    // State matches
                }
                _ => {
                    let _ = send_error_response(
                        &mut stream,
                        "Invalid state parameter. Possible CSRF attack.",
                    );
                    return Err(PkceError::InvalidState {
                        location: snafu::location!(),
                    });
                }
            }

            if let Some(ref error) = data.error {
                let message = data
                    .error_description
                    .clone()
                    .unwrap_or_else(|| error.clone());
                let _ = send_error_response(&mut stream, &message);
                return Err(PkceError::AuthorizationError {
                    error: error.clone(),
                    error_description: data.error_description.clone(),
                    location: snafu::location!(),
                });
            }

            if data.code.is_none() {
                let _ = send_error_response(&mut stream, "Missing authorization code.");
                return Err(PkceError::MissingCode {
                    location: snafu::location!(),
                });
            }

            let _ = send_success_response(&mut stream);
            Ok(data)
        }
        None => {
            let _ = send_error_response(&mut stream, "Invalid request.");
            Err(PkceError::MissingCode {
                location: snafu::location!(),
            })
        }
    }
}

/// Build form-urlencoded body from params.
fn build_form_body(params: &HashMap<&str, &str>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Exchange the authorization code for tokens.
async fn exchange_code(
    client: &reqwest::Client,
    provider: &OAuthProvider,
    code: &str,
    verifier: &SecretString,
    redirect_port: u16,
) -> Result<TokenResponse> {
    let redirect_uri = provider
        .redirect_uri
        .clone()
        .unwrap_or_else(|| format!("http://127.0.0.1:{redirect_port}/callback"));

    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", code);
    params.insert("redirect_uri", &redirect_uri);
    params.insert("client_id", &provider.client_id);
    params.insert("code_verifier", verifier.expose_secret());

    let body = build_form_body(&params);

    let response = client
        .post(&provider.token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context(HttpRequestSnafu)?;

    let status = response.status();
    let body_text = response.text().await.context(HttpRequestSnafu)?;

    if !status.is_success() {
        // Try to parse as OAuth error
        if let Ok(err) = serde_json::from_str::<TokenErrorResponse>(&body_text) {
            return Err(PkceError::OAuthError {
                error: err.error,
                error_description: err.error_description,
                location: snafu::location!(),
            });
        }
        return Err(PkceError::OAuthError {
            error: format!("HTTP {}: {}", status, body_text),
            error_description: None,
            location: snafu::location!(),
        });
    }

    serde_json::from_str(&body_text).context(ParseResponseSnafu)
}

/// Perform the complete PKCE authorization flow.
///
/// This function:
/// 1. Generates a PKCE code verifier and challenge
/// 2. Starts a local HTTP server to receive the callback
/// 3. Prints the authorization URL for the user to visit
/// 4. Waits for the callback and validates the state parameter
/// 5. Exchanges the authorization code for tokens
/// 6. Returns the credential file data
///
/// # Errors
///
/// Returns an error if any step of the flow fails.
///
/// # Example
///
/// ```no_run
/// use aletheia_symbolon::credential::pkce::{OAuthProvider, pkce_login};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = OAuthProvider::new(
///     "my-client-id",
///     "https://auth.example.com/authorize",
///     "https://auth.example.com/token",
/// )
/// .with_scope("read")
/// .with_scope("write");
///
/// let credential = pkce_login(&provider).await?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub async fn pkce_login(provider: &OAuthProvider) -> Result<CredentialFile> {
    // Generate PKCE parameters
    let pkce = PkcePair::generate()?;
    let state = generate_state()?;

    // Start callback server
    let (port, callback_rx) = start_callback_server(&state)?;

    // Build and display authorization URL
    let auth_url = build_authorization_url(provider, &pkce, &state, port);

    eprintln!("\n🔐 Authentication required");
    eprintln!("Please visit this URL to authorize:\n");
    eprintln!("  {auth_url}\n");

    // Wait for callback with timeout
    let callback_result = tokio::time::timeout(Duration::from_secs(300), callback_rx).await;

    let callback_data = match callback_result {
        Ok(Ok(Ok(data))) => data,
        Ok(Ok(Err(e))) => return Err(e),
        Ok(Err(_)) => {
            return Err(PkceError::ServerBind {
                source: std::io::Error::new(std::io::ErrorKind::Other, "channel closed"),
                location: snafu::location!(),
            });
        }
        Err(_) => {
            return Err(PkceError::Timeout {
                location: snafu::location!(),
            });
        }
    };

    let code = callback_data.code.ok_or(PkceError::MissingCode {
        location: snafu::location!(),
    })?;

    info!("received authorization code, exchanging for tokens");

    // Exchange code for tokens
    let client = reqwest::Client::new();
    let token_response = exchange_code(&client, provider, &code, &pkce.verifier, port).await?;

    info!("successfully obtained access token");

    // Build credential file
    let expires_at = token_response
        .expires_in
        .map(|secs| super::unix_epoch_ms() + secs * 1000);

    let scopes = token_response
        .scope
        .map(|s| s.split_whitespace().map(String::from).collect());

    Ok(CredentialFile {
        token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at,
        scopes,
        subscription_type: None,
    })
}

/// Perform PKCE flow and save credentials to a file.
///
/// This is a convenience wrapper around [`pkce_login`] that saves
/// the resulting credentials to the specified path.
///
/// # Errors
///
/// Returns an error if the flow fails or if the credential file cannot be saved.
pub async fn pkce_login_and_save(
    provider: &OAuthProvider,
    path: &std::path::Path,
) -> Result<CredentialFile> {
    let cred = pkce_login(provider).await?;
    cred.save(path).context(SaveCredentialSnafu)?;
    info!(path = %path.display(), "credentials saved");
    Ok(cred)
}

#[cfg(test)]
mod tests {
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
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
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
}
