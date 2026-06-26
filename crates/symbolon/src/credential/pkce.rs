//! OAuth 2.0 PKCE Authorization Code Flow (RFC 7636 + RFC 8252).
//!
//! This module implements the Proof Key for Code Exchange (PKCE) extension
//! for the OAuth 2.0 Authorization Code flow, suitable for native applications.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use koina::secret::SecretString;
use rand::TryRng as _;
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use tracing::{debug, info};

use crate::util::base64url_encode;

use super::OAuthRequiredAction;
use super::file_ops::CredentialFile;
use super::oauth_types::{OAuthErrorResponse, OAuthTokenResponse};

/// Errors from PKCE authentication flow.
// kanon:ignore RUST/no-debug-derive-on-public-types -- WHY: error enum; Debug is required by std::error::Error and Result ergonomics. Every variant carries only OAuth error codes/descriptions, source errors, and a location — no secrets, tokens, or PKCE verifiers.
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

    /// Failed to set listener blocking mode.
    #[snafu(display("failed to set blocking mode on listener: {source}"))]
    SetBlocking {
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
// kanon:ignore RUST/no-debug-derive-on-public-types — OAuthProvider is a config struct with no secrets; Debug is safe for diagnostics and CLI output
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OAuthProvider {
    /// OAuth client identifier.
    // kanon:ignore RUST/primitive-for-domain-id -- WHY: OAuth client_id is a provider-assigned public identifier carried in config and on the wire; a newtype would break serde/config interop without adding safety.
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
        let mut rng = rand::rngs::SysRng;
        let mut verifier_bytes = vec![0u8; 128];
        rng.try_fill_bytes(&mut verifier_bytes)
            .map_err(|_e| PkceError::RandomGeneration {
                location: snafu::location!(),
            })?;
        let verifier = base64url_encode(&verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();
        let challenge = base64url_encode(&hash);

        Ok(Self {
            verifier: SecretString::from(verifier),
            challenge,
        })
    }
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
    let mut rng = rand::rngs::SysRng;
    let mut bytes = vec![0u8; 32];
    rng.try_fill_bytes(&mut bytes)
        .map_err(|_e| PkceError::RandomGeneration {
            location: snafu::location!(),
        })?;
    Ok(base64url_encode(&bytes))
}

/// Simple URL encoding for query parameters.
pub(crate) fn url_encode(s: &str) -> String {
    use std::fmt::Write;

    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(char::from(byte));
            }
            _ => {
                result.push('%');
                // INVARIANT: writing to a String via fmt::Write cannot fail.
                // kanon:ignore RUST/no-silent-result-swallow — fmt::Write to String is infallible; write! returns Result for trait consistency only
                let _ = write!(result, "{byte:02X}");
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
        .unwrap_or_else(|| format!("http://127.0.0.1:{redirect_port}/callback")); // kanon:ignore SECURITY/hardcoded-loopback-url -- RFC 8252 S7.3 OAuth2 loopback; 127.0.0.1 is mandatory for installed-app PKCE flows

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

    // NOTE: request line shape: GET /callback?code=...&state=... HTTP/1.1
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("-");
    let Some(path) = parts.next() else {
        return Ok(None);
    };

    // WHY: log method + bare path only; query string contains OAuth code/state secrets
    let bare_path = path.split('?').next().unwrap_or(path);
    debug!(method = %method, path = %bare_path, "OAuth callback received");

    let query_start = path.find('?');
    // INVARIANT: i is from find('?'), so i+1 is a valid byte boundary
    #[expect(
        clippy::string_slice,
        reason = "i from find('?'), i+1 is valid boundary"
    )]
    // kanon:ignore RUST/indexing-slicing — i from find('?') on the same string, i+1 is a valid UTF-8 boundary
    let query = query_start.map_or("", |i| &path[i + 1..]);

    let mut data = CallbackData::default();

    for pair in query.split('&') {
        if let Some(eq_pos) = pair.find('=') {
            // INVARIANT: eq_pos from find('='), always a valid byte boundary
            #[expect(clippy::string_slice, reason = "eq_pos from find('='), valid boundary")]
            // kanon:ignore RUST/indexing-slicing — eq_pos from find('=') on the same string, valid byte boundary
            let key = &pair[..eq_pos];
            #[expect(
                clippy::string_slice,
                reason = "eq_pos from find('='), eq_pos+1 is valid"
            )]
            // kanon:ignore RUST/indexing-slicing — eq_pos from find('=') on the same string, eq_pos+1 is a valid byte boundary
            // kanon:ignore RUST/no-result-unwrap-or-default — a url_decode failure on a callback query param degrades to an empty value, which fails downstream code/state validation (auth error), never a silent success; clippy::unwrap_or_default forbids the unwrap_or_else(String::new) form
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

    // WHY: drain the remaining headers so the request is fully consumed
    // before the response is written.
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
        // kanon:ignore RUST/no-silent-result-swallow — oneshot receiver may be dropped after timeout; send failure is harmless
        let _ = tx.send(result);
    });

    Ok((port, rx))
}

/// Handle a single callback connection.
fn handle_callback_connection(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<CallbackData> {
    // WHY: the listener must be in blocking mode so accept() waits for the
    // browser redirect instead of returning WouldBlock.
    listener.set_nonblocking(false).context(SetBlockingSnafu)?;

    let (stream, addr) = listener.accept().context(AcceptConnectionSnafu)?;
    info!(addr = %addr, "received OAuth callback");

    let mut reader = BufReader::new(&stream);
    let callback_data = parse_callback_request(&mut reader)?;

    let mut stream = stream;

    if let Some(data) = callback_data {
        match &data.state {
            // WHY: state must match the value generated at login start; a
            // mismatch indicates a possible CSRF attack.
            Some(state) if state == expected_state => {}
            _ => {
                // kanon:ignore RUST/no-silent-result-swallow — error response is best-effort after state mismatch; client may have closed connection
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
            // kanon:ignore RUST/no-silent-result-swallow — error response is best-effort after OAuth error; client may have closed connection
            let _ = send_error_response(&mut stream, &message);
            return Err(PkceError::AuthorizationError {
                error: error.clone(),
                error_description: data.error_description.clone(),
                location: snafu::location!(),
            });
        }

        if data.code.is_none() {
            // kanon:ignore RUST/no-silent-result-swallow — error response is best-effort when code is missing; client may have closed connection
            let _ = send_error_response(&mut stream, "Missing authorization code.");
            return Err(PkceError::MissingCode {
                location: snafu::location!(),
            });
        }

        // kanon:ignore RUST/no-silent-result-swallow — success response is best-effort; client may have closed connection
        let _ = send_success_response(&mut stream);
        Ok(data)
    } else {
        // kanon:ignore RUST/no-silent-result-swallow — error response is best-effort for invalid request; client may have closed connection
        let _ = send_error_response(&mut stream, "Invalid request.");
        Err(PkceError::MissingCode {
            location: snafu::location!(),
        })
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
) -> Result<OAuthTokenResponse> {
    let redirect_uri = provider
        .redirect_uri
        .clone()
        .unwrap_or_else(|| format!("http://127.0.0.1:{redirect_port}/callback")); // kanon:ignore SECURITY/hardcoded-loopback-url -- RFC 8252 S7.3 OAuth2 loopback; 127.0.0.1 is mandatory for installed-app PKCE flows

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
        // WHY: prefer the structured OAuth error body when it parses; fall back
        // to the raw HTTP status + body.
        if let Ok(err) = serde_json::from_str::<OAuthErrorResponse>(&body_text) {
            return Err(PkceError::OAuthError {
                error: err.error,
                error_description: err.error_description,
                location: snafu::location!(),
            });
        }
        return Err(PkceError::OAuthError {
            error: format!("HTTP {status}: {body_text}"),
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
/// use symbolon::credential::pkce::{OAuthProvider, pkce_login};
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
#[tracing::instrument(skip_all)]
pub async fn pkce_login(provider: &OAuthProvider) -> Result<CredentialFile> {
    pkce_login_with_action(provider, |_| {}).await
}

/// Perform the complete PKCE OAuth login flow with typed caller-rendered actions.
///
/// # Errors
///
/// Returns an error if any step of the flow fails.
#[tracing::instrument(skip_all)]
pub async fn pkce_login_with_action<F>(
    provider: &OAuthProvider,
    mut action_callback: F,
) -> Result<CredentialFile>
where
    F: FnMut(OAuthRequiredAction),
{
    let pkce = PkcePair::generate()?;
    let state = generate_state()?;

    let (port, callback_rx) = start_callback_server(&state)?;

    let auth_url = build_authorization_url(provider, &pkce, &state, port);

    action_callback(OAuthRequiredAction::BrowserOpenUrl {
        url: auth_url.clone(),
    });
    action_callback(OAuthRequiredAction::WaitingForCallback { timeout_secs: 300 });

    let callback_result = tokio::time::timeout(Duration::from_mins(5), callback_rx).await;

    let callback_data = match callback_result {
        Ok(Ok(Ok(data))) => data,
        Ok(Ok(Err(e))) => return Err(e),
        Ok(Err(_)) => {
            return Err(PkceError::ServerBind {
                source: std::io::Error::other("channel closed"),
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

    let client = reqwest::Client::new();
    let token_response = exchange_code(&client, provider, &code, &pkce.verifier, port).await?;

    // SAFETY: logging success status, not the token value
    info!("successfully obtained access token"); // kanon:ignore SECURITY/credential-logging -- logs success status, not the token
    action_callback(OAuthRequiredAction::AuthorizationSucceeded);

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
#[tracing::instrument(skip_all)]
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
#[path = "pkce_tests.rs"]
mod pkce_tests;
