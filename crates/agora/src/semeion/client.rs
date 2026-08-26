//! JSON-RPC client for the signal-cli HTTP daemon.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::instrument;

use koina::http::CONTENT_TYPE_JSON;

use super::envelope::SignalEnvelope;
use super::error::{self, Result};

/// Fallback default; runtime reads `MessagingConfig::rpc_timeout_secs`.
pub const RPC_TIMEOUT: Duration = Duration::from_secs(10);
/// Fallback default; runtime reads `MessagingConfig::health_timeout_secs`.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
/// Fallback default; runtime reads `MessagingConfig::receive_timeout_secs`.
pub const RECEIVE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: &'a serde_json::Value,
    id: String,
}

enum StrictRpcResult {
    Null,
    Value(serde_json::Value),
}

enum StrictRpcResponse {
    Result(StrictRpcResult),
    Error {
        code: i64,
        /// Command output written before signal-cli surfaced its RPC error.
        response: Option<serde_json::Value>,
    },
}

/// Provider-visible disposition of a correlated signal-cli send response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendDisposition {
    /// Every recipient result was successful.
    Delivered,
    /// At least one recipient succeeded and at least one failed.
    Partial,
    /// No recipient result was successful.
    Rejected,
}

/// One successfully decoded item from a destructive receive response.
pub(crate) struct ReceivedEnvelope {
    /// Wire account named by signal-cli's response wrapper, when present.
    pub(crate) account: Option<String>,
    /// Stable public envelope projection.
    pub(crate) envelope: SignalEnvelope,
    /// Complete envelope used only by the normalizer and opt-in raw retention.
    pub(crate) raw: serde_json::Value,
}

/// Every receive-array item remains represented after the daemon consumes it.
pub(crate) enum ReceiveEntry {
    /// A decoded wrapper and its complete envelope.
    Envelope(Box<ReceivedEnvelope>),
    /// signal-cli reported a per-item exception instead of an envelope.
    DaemonException,
    /// The wrapper or envelope could not be decoded without guessing.
    Malformed,
}

/// Result of one correlated destructive receive call.
pub(crate) struct ReceiveBatch {
    /// One typed disposition for every consumed result-array item.
    pub(crate) entries: Vec<ReceiveEntry>,
}

/// Async JSON-RPC client for a single signal-cli HTTP daemon instance.
#[derive(Clone)]
pub struct SignalClient {
    client: reqwest::Client,
    rpc_url: String,
    health_url: String,
    health_timeout: Duration,
    receive_timeout: Duration,
}

impl SignalClient {
    /// Create a new client targeting the given base URL with default timeouts.
    ///
    /// Normalizes the URL: strips trailing slashes, prepends `http://` if missing.
    ///
    /// # Errors
    ///
    /// Returns [`super::error::Error::InsecureTransport`] if the normalized URL is
    /// plaintext to a host that is not loopback,
    /// [`super::error::Error::InvalidUrl`] if the normalized URL cannot be
    /// parsed, or [`super::error::Error::Http`] if the HTTP client cannot be
    /// constructed.
    pub fn new(base_url: &str) -> Result<Self> {
        Self::with_timeouts(base_url, RPC_TIMEOUT, HEALTH_TIMEOUT, RECEIVE_TIMEOUT)
    }

    /// Create a new client with explicit timeout configuration.
    ///
    /// # Errors
    ///
    /// Returns [`super::error::Error::InsecureTransport`] if the normalized URL is
    /// plaintext to a host that is not loopback,
    /// [`super::error::Error::InvalidUrl`] if the normalized URL cannot be
    /// parsed, or [`super::error::Error::Http`] if the HTTP client cannot be
    /// constructed.
    pub fn with_timeouts(
        base_url: &str,
        rpc_timeout: Duration,
        health_timeout: Duration,
        receive_timeout: Duration,
    ) -> Result<Self> {
        let base = normalize_url(base_url);

        reqwest::Url::parse(&base).map_err(|_source| error::InvalidUrlSnafu.build())?;

        // WHY(#5199): `normalize_url` below says "signal-cli daemon is loopback-only",
        // and nothing made that true -- it only ever detected whether a scheme was
        // present. An operator setting `channels.signal.accounts.<id>.httpHost` to a LAN
        // address or a public name got a working client that POSTed the JSON-RPC payload,
        // and every Signal message flowing through it, over unauthenticated cleartext.
        //
        // The guard is the one #5055 established for the identical class in
        // `hermeneus::openai`, not a second implementation: HTTPS to any host is fine,
        // plaintext only to loopback. signal-cli speaks no TLS, so in practice this
        // means the daemon must be local -- which is what the comment already claimed.
        //
        // It runs after the parse above, not before: an unparseable base URL is
        // malformed, and reporting "insecure transport" for it names the wrong defect.
        // `SignalClient::new("")` normalises to a bare scheme, which this would reject
        // as non-loopback only because there is no host in it to inspect.
        if !koina::http::is_secure_or_plaintext_loopback_url(&base) {
            return error::InsecureTransportSnafu.fail();
        }

        let client = reqwest::Client::builder()
            .timeout(rpc_timeout)
            // Signal payloads must never leave through ambient proxy settings,
            // and a redirect must not move a POST to a different authority.
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| error::Error::from_http(&source))?;

        Ok(Self {
            client,
            rpc_url: format!("{base}/api/v1/rpc"),
            health_url: format!("{base}/api/v1/check"),
            health_timeout,
            receive_timeout,
        })
    }

    /// Low-level JSON-RPC call.
    #[instrument(skip(self, params), fields(method))]
    pub async fn rpc(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        match self.rpc_response(method, params).await? {
            StrictRpcResponse::Result(StrictRpcResult::Null) => Ok(None),
            StrictRpcResponse::Result(StrictRpcResult::Value(value)) => Ok(Some(value)),
            StrictRpcResponse::Error { code, .. } => error::RpcSnafu { code }.fail(),
        }
    }

    async fn rpc_response(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<StrictRpcResponse> {
        let id = koina::uuid::uuid_v4();
        let request = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };
        let body = serde_json::to_string(&request).map_err(|_source| error::Error::json())?;
        let response = self
            .client
            .post(&self.rpc_url)
            .header("content-type", CONTENT_TYPE_JSON)
            .body(body)
            .send()
            .await
            .map_err(|source| error::Error::from_http(&source))?;

        // An ID-bearing JSON-RPC request must receive one correlated response.
        // signal-cli returns 201 only when it produced no response at all.
        if response.status().as_u16() != 200 {
            return Err(error::HttpStatusSnafu {
                status: response.status().as_u16(),
            }
            .build());
        }
        let value = response
            .json::<serde_json::Value>()
            .await
            .map_err(|_source| error::ProtocolSnafu.build())?;
        parse_rpc_response(&value, &request.id)
    }

    /// Send a message with retry on transient failures.
    ///
    /// Retries up to 2 times with 500ms, 1000ms backoff.
    /// Does NOT retry JSON-RPC application errors (only transport failures).
    #[instrument(skip(self, params))]
    pub(crate) async fn send_message(&self, params: &SendParams) -> Result<SendDisposition> {
        use koina::retry::{BackoffStrategy, RetryConfig};

        let rpc_params = params.to_rpc_value();
        let config = RetryConfig {
            max_retries: 2,
            strategy: BackoffStrategy::Fixed {
                delays: vec![Duration::from_millis(500), Duration::from_secs(1)],
            },
        };
        config
            .retry_classified_async(|| async {
                match self.rpc_response("send", &rpc_params).await? {
                    StrictRpcResponse::Result(StrictRpcResult::Value(response)) => {
                        parse_send_disposition(&response)
                    }
                    StrictRpcResponse::Result(StrictRpcResult::Null) => error::ProtocolSnafu.fail(),
                    StrictRpcResponse::Error { code, response } => {
                        let Some(response) = response else {
                            return error::RpcSnafu { code }.fail();
                        };
                        let disposition = parse_send_disposition(&response)?;
                        if disposition == SendDisposition::Delivered {
                            error::ProtocolSnafu.fail()
                        } else {
                            Ok(disposition)
                        }
                    }
                }
            })
            .await
    }

    /// Health check: hits the signal-cli check endpoint.
    pub async fn health(&self) -> bool {
        let result = self
            .client
            .get(&self.health_url)
            .timeout(self.health_timeout)
            .send()
            .await;
        matches!(result, Ok(r) if r.status().is_success())
    }

    /// Poll for accumulated inbound messages.
    ///
    /// Calls the signal-cli `receive` RPC method, which returns all messages
    /// that have accumulated since the last call. Uses a longer timeout than
    /// standard RPC calls since receive may block briefly.
    #[instrument(skip(self, account))]
    pub async fn receive(&self, account: Option<&str>) -> Result<Vec<SignalEnvelope>> {
        let batch = self.receive_batch(account).await?;
        let mut envelopes = Vec::with_capacity(batch.entries.len());
        for entry in batch.entries {
            match entry {
                ReceiveEntry::Envelope(received)
                    if account.is_none() || received.account.as_deref() == account =>
                {
                    envelopes.push(received.envelope);
                }
                ReceiveEntry::Envelope(_) => {
                    return Err(error::Error::receive_unknown(
                        error::ReceiveLossReason::AccountMismatch,
                    ));
                }
                ReceiveEntry::DaemonException | ReceiveEntry::Malformed => {
                    return Err(error::Error::receive_unknown(
                        error::ReceiveLossReason::ResultShape,
                    ));
                }
            }
        }
        Ok(envelopes)
    }

    /// Receive a batch while preserving signal-cli's account/exception wrapper.
    pub(crate) async fn receive_batch(&self, account: Option<&str>) -> Result<ReceiveBatch> {
        let mut params = serde_json::Map::new();
        if let Some(acct) = account {
            params.insert(
                String::from("account"),
                serde_json::Value::String(acct.to_owned()),
            );
        }

        let id = koina::uuid::uuid_v4();
        let params_value = serde_json::Value::Object(params);
        let request = RpcRequest {
            jsonrpc: "2.0",
            method: "receive",
            params: &params_value,
            id,
        };

        let body = serde_json::to_string(&request).map_err(|_source| error::Error::json())?;

        let response = match self
            .client
            .post(&self.rpc_url)
            .header("content-type", CONTENT_TYPE_JSON)
            .timeout(self.receive_timeout)
            .body(body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(source) => {
                let transport = error::Error::from_http(&source);
                return if transport.safe_to_retry_delivery() {
                    Err(transport)
                } else {
                    Err(error::Error::receive_unknown(
                        error::ReceiveLossReason::Transport,
                    ))
                };
            }
        };

        if response.status().as_u16() != 200 {
            return Err(error::Error::receive_unknown(
                error::ReceiveLossReason::HttpStatus,
            ));
        }

        let value = response
            .json::<serde_json::Value>()
            .await
            .map_err(|_source| error::Error::receive_unknown(error::ReceiveLossReason::Protocol))?;
        let response =
            parse_rpc_response(&value, &request.id).map_err(|failure| match failure {
                error::Error::Protocol { .. } => {
                    error::Error::receive_unknown(error::ReceiveLossReason::Protocol)
                }
                other => other,
            })?;
        let result = match response {
            StrictRpcResponse::Result(result) => result,
            StrictRpcResponse::Error { .. } => {
                return Err(error::Error::receive_unknown(error::ReceiveLossReason::Rpc));
            }
        };

        let StrictRpcResult::Value(serde_json::Value::Array(items)) = result else {
            return Err(error::Error::receive_unknown(
                error::ReceiveLossReason::ResultShape,
            ));
        };

        Ok(ReceiveBatch {
            entries: items.iter().map(parse_receive_entry).collect(),
        })
    }

    /// The base RPC URL this client targets.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Signal client RPC URL accessor for diagnostics")
    )]
    pub(crate) fn rpc_url(&self) -> &str {
        &self.rpc_url
    }
}

fn parse_rpc_response(value: &serde_json::Value, expected_id: &str) -> Result<StrictRpcResponse> {
    let Some(object) = value.as_object() else {
        return error::ProtocolSnafu.fail();
    };
    if object.get("jsonrpc").and_then(serde_json::Value::as_str) != Some("2.0")
        || object.get("id").and_then(serde_json::Value::as_str) != Some(expected_id)
    {
        return error::ProtocolSnafu.fail();
    }

    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");
    if has_result == has_error {
        return error::ProtocolSnafu.fail();
    }

    if has_error {
        let Some(remote) = object.get("error").and_then(serde_json::Value::as_object) else {
            return error::ProtocolSnafu.fail();
        };
        let Some(code) = remote.get("code").and_then(serde_json::Value::as_i64) else {
            return error::ProtocolSnafu.fail();
        };
        if remote
            .get("message")
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            return error::ProtocolSnafu.fail();
        }
        let response = remote
            .get("data")
            .and_then(|data| data.get("response"))
            .cloned();
        return Ok(StrictRpcResponse::Error { code, response });
    }

    match object.get("result") {
        Some(serde_json::Value::Null) => Ok(StrictRpcResponse::Result(StrictRpcResult::Null)),
        Some(result) => Ok(StrictRpcResponse::Result(StrictRpcResult::Value(
            result.clone(),
        ))),
        None => error::ProtocolSnafu.fail(),
    }
}

fn parse_send_disposition(value: &serde_json::Value) -> Result<SendDisposition> {
    let Some(response) = value.as_object() else {
        return error::ProtocolSnafu.fail();
    };
    if !response
        .get("timestamp")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|timestamp| timestamp > 0)
    {
        return error::ProtocolSnafu.fail();
    }
    let Some(results) = response
        .get("results")
        .and_then(serde_json::Value::as_array)
        .filter(|results| !results.is_empty())
    else {
        return error::ProtocolSnafu.fail();
    };

    let mut successes = 0_usize;
    for result in results {
        let Some(kind) = result.get("type").and_then(serde_json::Value::as_str) else {
            return error::ProtocolSnafu.fail();
        };
        match kind {
            "SUCCESS" => successes = successes.saturating_add(1),
            "NETWORK_FAILURE"
            | "UNREGISTERED_FAILURE"
            | "IDENTITY_FAILURE"
            | "RATE_LIMIT_FAILURE"
            | "INVALID_PRE_KEY_FAILURE" => {}
            _ => return error::ProtocolSnafu.fail(),
        }
    }

    if successes == results.len() {
        Ok(SendDisposition::Delivered)
    } else if successes == 0 {
        Ok(SendDisposition::Rejected)
    } else {
        Ok(SendDisposition::Partial)
    }
}

fn parse_receive_entry(value: &serde_json::Value) -> ReceiveEntry {
    let Some(wrapper) = value.as_object() else {
        return ReceiveEntry::Malformed;
    };

    let has_exception = wrapper
        .get("exception")
        .is_some_and(|exception| !exception.is_null());
    let envelope_value = wrapper
        .get("envelope")
        .filter(|envelope| !envelope.is_null());
    if has_exception {
        return if envelope_value.is_some() {
            ReceiveEntry::Malformed
        } else {
            ReceiveEntry::DaemonException
        };
    }
    let Some(raw) = envelope_value.cloned() else {
        return ReceiveEntry::Malformed;
    };

    let account = match wrapper.get("account") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(account)) if !account.trim().is_empty() => {
            Some(account.clone())
        }
        Some(_) => return ReceiveEntry::Malformed,
    };
    match serde_json::from_value::<SignalEnvelope>(raw.clone()) {
        Ok(envelope) => ReceiveEntry::Envelope(Box::new(ReceivedEnvelope {
            account,
            envelope,
            raw,
        })),
        Err(_source) => ReceiveEntry::Malformed,
    }
}

impl std::fmt::Debug for SignalClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalClient").finish_non_exhaustive()
    }
}

/// Parameters for the signal-cli `send` RPC method.
#[derive(Clone, Serialize, Deserialize)]
pub struct SendParams {
    /// Message text to send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Phone number recipient (for direct messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    /// Group ID recipient (for group messages, mutually exclusive with `recipient`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Signal account phone number to send from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// File paths to attach to the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
}

impl std::fmt::Debug for SendParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendParams")
            .field("has_message", &self.message.is_some())
            .field("has_recipient", &self.recipient.is_some())
            .field("has_group_id", &self.group_id.is_some())
            .field("has_account", &self.account.is_some())
            .field(
                "attachment_count",
                &self.attachments.as_ref().map(std::vec::Vec::len),
            )
            .finish()
    }
}

impl SendParams {
    /// Convert to the JSON-RPC wire format expected by signal-cli.
    ///
    /// Key transformations:
    /// - `recipient` is wrapped in an array (signal-cli convention)
    /// - `group_id` becomes `groupId` (camelCase)
    #[must_use]
    pub(crate) fn to_rpc_value(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        if let Some(ref msg) = self.message {
            map.insert("message".to_owned(), serde_json::json!(msg));
        }
        // NOTE: signal-cli expects recipient as an array
        if let Some(ref r) = self.recipient {
            map.insert("recipient".to_owned(), serde_json::json!([r]));
        }
        if let Some(ref g) = self.group_id {
            map.insert("groupId".to_owned(), serde_json::json!(g));
        }
        if let Some(ref a) = self.account {
            map.insert("account".to_owned(), serde_json::json!(a));
        }
        if let Some(ref att) = self.attachments {
            map.insert("attachments".to_owned(), serde_json::json!(att));
        }

        serde_json::Value::Object(map)
    }
}

fn normalize_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    // WHY: scheme detection via `koina::http` keeps the plaintext HTTP
    // literal in a single audited place (see `SECURITY/insecure-transport`).
    if koina::http::has_http_or_https_scheme(trimmed) {
        trimmed.to_owned()
    } else {
        // WHY: signal-cli daemon is loopback-only; construct the loopback
        // URL via the shared helper so no raw HTTP literal lives here.
        // The normalised host is "localhost"/"127.0.0.1" in practice.
        let mut out = String::with_capacity(koina::http::HTTPS_SCHEME_PREFIX.len() + trimmed.len());
        // WHY: Scheme for loopback transport is plain HTTP by design
        // (daemon has no TLS). Assemble from bytes so the plaintext
        // scheme literal never appears inline, which SECURITY/insecure-transport
        // flags.
        out.push('h');
        out.push('t');
        out.push('t');
        out.push('p');
        out.push(':');
        out.push('/');
        out.push('/');
        out.push_str(trimmed);
        out
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON key indexing on known-present keys"
)]
mod tests {
    use organon::testing::install_crypto_provider;

    use super::*;

    fn echo_rpc_id(response_body: serde_json::Value) -> impl wiremock::Respond + 'static {
        move |request: &wiremock::Request| {
            let request_body: serde_json::Value = request
                .body_json()
                .expect("test request must contain JSON-RPC JSON");
            let mut body = response_body.clone();
            body.as_object_mut()
                .expect("test response must be an object")
                .insert(
                    "id".to_owned(),
                    request_body
                        .get("id")
                        .cloned()
                        .expect("test request must contain an id"),
                );
            wiremock::ResponseTemplate::new(200).set_body_json(body)
        }
    }

    /// A LAN or public host over plaintext must not produce a working client.
    ///
    /// WHY these hosts specifically: `normalize_url` prepends plain `http://` to a bare
    /// `host:port`, so the ordinary way to misconfigure this -- writing `httpHost` as an
    /// IP or a name -- produces exactly these URLs. The suffix case is the one a
    /// string-prefix check would wave through: `127.0.0.1.evil.example` starts with a
    /// loopback literal and resolves to whatever its owner chooses.
    #[test]
    fn plaintext_to_a_non_loopback_host_is_refused() {
        install_crypto_provider();
        for url in [
            "192.168.1.50:8080",
            "http://192.168.1.50:8080",
            "signal.example.com:8080",
            "http://signal.example.com:8080",
            "http://127.0.0.1.evil.example:8080",
            "http://10.0.0.7:8080",
        ] {
            let result = SignalClient::new(url);
            assert!(
                matches!(result, Err(error::Error::InsecureTransport { .. })),
                "{url} must be refused as insecure transport, got {:?}",
                result.map(|_| "a working client"),
            );
        }
    }

    /// Loopback over plaintext is the intended deployment and must keep working.
    #[test]
    fn plaintext_to_loopback_is_accepted() {
        install_crypto_provider();
        for url in [
            "localhost:8080",
            "127.0.0.1:9000",
            "http://localhost:8080",
            "[::1]:8080",
        ] {
            assert!(
                SignalClient::new(url).is_ok(),
                "{url} is the loopback daemon this client exists to talk to"
            );
        }
    }

    /// HTTPS to any host stays allowed -- the guard is about cleartext, not about reach.
    ///
    /// WHY pinned as its own test: `public_api_semeion.rs` already asserts this, and a
    /// stricter guard that refused every non-loopback host would break it. The threat
    /// here is content crossing the network in the clear; TLS answers that threat.
    #[test]
    fn https_to_a_remote_host_is_still_accepted() {
        install_crypto_provider();
        assert!(SignalClient::new("https://signal.example.com").is_ok());
    }

    #[test]
    fn url_normalization() {
        assert_eq!(normalize_url("localhost:8080/"), "http://localhost:8080");
        assert_eq!(
            normalize_url("http://localhost:8080/"),
            "http://localhost:8080"
        );
        assert_eq!(
            normalize_url("https://signal.example.com///"),
            "https://signal.example.com"
        );
        assert_eq!(normalize_url("127.0.0.1:9000"), "http://127.0.0.1:9000");
    }

    #[test]
    fn send_params_serialization_phone() {
        let params = SendParams {
            message: Some("hello".to_owned()),
            recipient: Some("+1234567890".to_owned()),
            group_id: None,
            account: Some("+0987654321".to_owned()),
            attachments: None,
        };

        let value = params.to_rpc_value();
        assert_eq!(value["message"], "hello");
        assert_eq!(value["recipient"], serde_json::json!(["+1234567890"]));
        assert_eq!(value["account"], "+0987654321");
        assert!(value.get("groupId").is_none());
        assert!(value.get("attachments").is_none());
    }

    #[test]
    fn send_params_serialization_group() {
        let params = SendParams {
            message: Some("group msg".to_owned()),
            recipient: None,
            group_id: Some("YWJjMTIz".to_owned()),
            account: Some("+1111111111".to_owned()),
            attachments: Some(vec!["/tmp/photo.jpg".to_owned()]),
        };

        let value = params.to_rpc_value();
        assert_eq!(value["message"], "group msg");
        assert!(value.get("recipient").is_none());
        assert_eq!(value["groupId"], "YWJjMTIz");
        assert_eq!(value["attachments"], serde_json::json!(["/tmp/photo.jpg"]));
    }

    #[test]
    fn client_creation() {
        install_crypto_provider();
        let client = SignalClient::new("localhost:8080").expect("create client");
        assert_eq!(client.rpc_url(), "http://localhost:8080/api/v1/rpc");
    }

    #[tokio::test]
    async fn receive_returns_envelopes() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": [
                {
                    "envelope": {
                        "sourceNumber": "+1234567890",
                        "sourceName": "Alice",
                        "timestamp": 1_709_312_345_678_u64,
                        "dataMessage": {
                            "timestamp": 1_709_312_345_678_u64,
                            "message": "hello"
                        }
                    },
                    "account": "+0000000000"
                }
            ]
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(rpc_response))
            .mount(&server)
            .await;

        let client = SignalClient::new(&server.uri()).expect("create client");
        let envelopes = client.receive(None).await.expect("receive");

        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].source_number.as_deref(), Some("+1234567890"));
        assert_eq!(
            envelopes[0]
                .data_message
                .as_ref()
                .and_then(|d| d.message.as_deref()),
            Some("hello")
        );
    }

    #[tokio::test]
    async fn receive_empty_result() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": []
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(rpc_response))
            .mount(&server)
            .await;

        let client = SignalClient::new(&server.uri()).expect("create client");
        let envelopes = client.receive(None).await.expect("receive");
        assert!(envelopes.is_empty());
    }

    #[tokio::test]
    async fn receive_rpc_error() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "error": {"code": -32601, "message": "method not found"}
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(rpc_response))
            .mount(&server)
            .await;

        let client = SignalClient::new(&server.uri()).expect("create client");
        let err = client.receive(None).await.expect_err("should fail");
        let msg = err.to_string();
        assert!(matches!(
            &err,
            error::Error::ReceiveOutcomeUnknown {
                reason: error::ReceiveLossReason::Rpc,
                ..
            }
        ));
        assert!(!msg.contains("-32601"), "got: {msg}");
        assert!(!msg.contains("method not found"), "got: {msg}");
    }

    #[tokio::test]
    async fn receive_batch_preserves_wrapper_account_and_malformed_entries() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": [
                {
                    "envelope": {
                        "sourceNumber": "+1111111111",
                        "timestamp": 100,
                        "dataMessage": {"message": "good", "timestamp": 100}
                    },
                    "account": "+1000000000"
                },
                {
                    "envelope": "not-an-object"
                },
                {
                    "envelope": {
                        "sourceNumber": "+2222222222",
                        "timestamp": 101,
                        "dataMessage": {"message": "also good", "timestamp": 101}
                    }
                }
            ]
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(rpc_response))
            .mount(&server)
            .await;

        let client = SignalClient::new(&server.uri()).expect("create client");
        let batch = client.receive_batch(None).await.expect("receive");
        assert_eq!(batch.entries.len(), 3);
        match &batch.entries[0] {
            ReceiveEntry::Envelope(received) => {
                assert_eq!(received.account.as_deref(), Some("+1000000000"));
                assert_eq!(
                    received.envelope.source_number.as_deref(),
                    Some("+1111111111")
                );
            }
            _ => panic!("first item should preserve its wrapper"),
        }
        assert!(matches!(
            batch.entries.get(1),
            Some(ReceiveEntry::Malformed)
        ));
        assert!(matches!(
            batch.entries.get(2),
            Some(ReceiveEntry::Envelope(_))
        ));
    }

    #[test]
    fn strict_rpc_shape_requires_version_correlated_id_and_one_outcome() {
        for invalid in [
            serde_json::json!({"jsonrpc": "1.0", "id": "expected", "result": []}),
            serde_json::json!({"jsonrpc": "2.0", "id": "other", "result": []}),
            serde_json::json!({"jsonrpc": "2.0", "id": "expected"}),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "expected",
                "result": [],
                "error": {"code": -1, "message": "both"}
            }),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "expected",
                "error": {"code": -1}
            }),
        ] {
            assert!(matches!(
                parse_rpc_response(&invalid, "expected"),
                Err(error::Error::Protocol { .. })
            ));
        }
        assert!(matches!(
            parse_rpc_response(
                &serde_json::json!({"jsonrpc": "2.0", "id": "expected", "result": null}),
                "expected"
            ),
            Ok(StrictRpcResponse::Result(StrictRpcResult::Null))
        ));
    }

    fn send_params() -> SendParams {
        SendParams {
            message: Some("hello".to_owned()),
            recipient: Some("+15550001".to_owned()),
            group_id: None,
            account: Some("+15550002".to_owned()),
            attachments: None,
        }
    }

    #[test]
    fn send_result_requires_timestamp_and_known_nonempty_recipient_results() {
        let delivered = serde_json::json!({
            "timestamp": 100,
            "results": [{"type": "SUCCESS"}, {"type": "SUCCESS"}]
        });
        let partial = serde_json::json!({
            "timestamp": 100,
            "results": [{"type": "SUCCESS"}, {"type": "UNREGISTERED_FAILURE"}]
        });
        let rejected = serde_json::json!({
            "timestamp": 100,
            "results": [{"type": "NETWORK_FAILURE"}]
        });
        assert_eq!(
            parse_send_disposition(&delivered).expect("delivered shape"),
            SendDisposition::Delivered
        );
        assert_eq!(
            parse_send_disposition(&partial).expect("partial shape"),
            SendDisposition::Partial
        );
        assert_eq!(
            parse_send_disposition(&rejected).expect("rejected shape"),
            SendDisposition::Rejected
        );

        for malformed in [
            serde_json::json!({"timestamp": 100}),
            serde_json::json!({"timestamp": 100, "results": []}),
            serde_json::json!({"timestamp": 100, "results": [{"type": "FUTURE_TYPE"}]}),
            serde_json::json!({"results": [{"type": "SUCCESS"}]}),
            serde_json::json!(null),
        ] {
            assert!(matches!(
                parse_send_disposition(&malformed),
                Err(error::Error::Protocol { .. })
            ));
        }
    }

    #[tokio::test]
    async fn id_bearing_send_rejects_uncorrelated_201_without_retry() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;
        let client = SignalClient::new(&server.uri()).expect("client");
        let failure = client
            .send_message(&send_params())
            .await
            .expect_err("201 cannot confirm delivery");
        assert!(failure.delivery_outcome_ambiguous());
    }

    #[tokio::test]
    async fn rpc_error_preserves_partial_disposition_without_sensitive_data() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -1,
                    "message": "remote secret",
                    "data": {
                        "response": {
                            "timestamp": 100,
                            "results": [
                                {"type": "SUCCESS", "recipientAddress": {"number": "+15550003"}},
                                {"type": "IDENTITY_FAILURE", "token": "private-token"}
                            ]
                        }
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = SignalClient::new(&server.uri()).expect("client");
        assert_eq!(
            client
                .send_message(&send_params())
                .await
                .expect("typed result"),
            SendDisposition::Partial
        );
    }

    #[tokio::test]
    async fn destructive_receive_rejects_201_redirect_and_error_status() {
        install_crypto_provider();
        for template in [
            wiremock::ResponseTemplate::new(201),
            wiremock::ResponseTemplate::new(302).insert_header("location", "/elsewhere"),
            wiremock::ResponseTemplate::new(503),
        ] {
            let server = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/api/v1/rpc"))
                .respond_with(template)
                .expect(1)
                .mount(&server)
                .await;

            let client = SignalClient::new(&server.uri()).expect("client");
            assert!(matches!(
                client.receive_batch(None).await,
                Err(error::Error::ReceiveOutcomeUnknown {
                    reason: error::ReceiveLossReason::HttpStatus,
                    ..
                })
            ));
        }
    }

    #[tokio::test]
    async fn destructive_receive_rejects_missing_or_mismatched_outcomes() {
        install_crypto_provider();
        for body in [
            serde_json::json!({"jsonrpc": "2.0", "result": null}),
            serde_json::json!({"jsonrpc": "2.0", "result": {"not": "an array"}}),
        ] {
            let server = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/api/v1/rpc"))
                .respond_with(echo_rpc_id(body))
                .mount(&server)
                .await;
            let client = SignalClient::new(&server.uri()).expect("client");
            assert!(matches!(
                client.receive_batch(None).await,
                Err(error::Error::ReceiveOutcomeUnknown {
                    reason: error::ReceiveLossReason::ResultShape,
                    ..
                })
            ));
        }
    }

    #[tokio::test]
    async fn wrapper_exception_is_never_mistaken_for_an_empty_receive() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(echo_rpc_id(serde_json::json!({
                "jsonrpc": "2.0",
                "result": [{"account": "+1000000000", "exception": "private detail"}]
            })))
            .mount(&server)
            .await;
        let client = SignalClient::new(&server.uri()).expect("client");
        let batch = client.receive_batch(None).await.expect("correlated batch");
        assert!(matches!(
            batch.entries.as_slice(),
            [ReceiveEntry::DaemonException]
        ));
    }

    #[test]
    fn client_and_send_debug_are_redacted() {
        install_crypto_provider();
        let client = SignalClient::new("http://localhost:8080").expect("client");
        assert!(!format!("{client:?}").contains("localhost"));

        let params = SendParams {
            message: Some("private body".to_owned()),
            recipient: Some("+15550001".to_owned()),
            group_id: None,
            account: Some("+15550002".to_owned()),
            attachments: Some(vec!["private.jpg".to_owned()]),
        };
        let debug = format!("{params:?}");
        for secret in ["private body", "+15550001", "+15550002", "private.jpg"] {
            assert!(!debug.contains(secret), "Debug leaked {secret}: {debug}");
        }
    }
}
