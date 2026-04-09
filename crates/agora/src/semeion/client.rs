//! JSON-RPC client for the signal-cli HTTP daemon.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::instrument;


use aletheia_koina::http::CONTENT_TYPE_JSON;

use super::envelope::SignalEnvelope;
use super::error::{self, Result};

const RPC_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: &'a serde_json::Value,
    id: String,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    #[expect(dead_code, reason = "deserialized from JSON-RPC wire format")]
    jsonrpc: String,
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
    #[expect(dead_code, reason = "deserialized from JSON-RPC wire format")]
    id: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// Async JSON-RPC client for a single signal-cli HTTP daemon instance.
#[derive(Clone)]
pub struct SignalClient {
    client: reqwest::Client,
    rpc_url: String,
    health_url: String,
}

impl SignalClient {
    /// Create a new client targeting the given base URL.
    ///
    /// Normalizes the URL: strips trailing slashes, prepends `http://` if missing.
    ///
    /// # Errors
    ///
    /// Returns [`super::error::Error::Http`] if the HTTP client cannot be constructed.
    pub fn new(base_url: &str) -> Result<Self> {
        let base = normalize_url(base_url);

        let client = reqwest::Client::builder()
            .timeout(RPC_TIMEOUT)
            .build()
            .context(error::HttpSnafu)?;

        Ok(Self {
            client,
            rpc_url: format!("{base}/api/v1/rpc"),
            health_url: format!("{base}/api/v1/check"),
        })
    }

    /// Low-level JSON-RPC call.
    #[instrument(skip(self, params), fields(method))]
    pub async fn rpc(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let id = aletheia_koina::uuid::uuid_v4();
        let request = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };

        let body = serde_json::to_string(&request).context(error::JsonSnafu)?;

        let response = self
            .client
            .post(&self.rpc_url)
            .header("content-type", CONTENT_TYPE_JSON)
            .body(body)
            .send()
            .await
            .context(error::HttpSnafu)?;

        // NOTE: 201 = accepted, no body (signal-cli convention for fire-and-forget)
        if response.status().as_u16() == 201 {
            return Ok(None);
        }

        let rpc_response: RpcResponse = response.json().await.context(error::HttpSnafu)?;

        if let Some(err) = rpc_response.error {
            return Err(error::RpcSnafu {
                code: err.code,
                message: err.message,
            }
            .build());
        }

        Ok(rpc_response.result)
    }

    /// Send a message with retry on transient failures.
    ///
    /// Retries up to 2 times with 500ms, 1000ms backoff.
    /// Does NOT retry JSON-RPC application errors (only transport failures).
    #[instrument(skip(self, params))]
    pub async fn send_message(&self, params: &SendParams) -> Result<Option<serde_json::Value>> {
        use aletheia_koina::retry::{BackoffStrategy, RetryConfig};

        let rpc_params = params.to_rpc_value();
        let config = RetryConfig {
            max_retries: 2,
            strategy: BackoffStrategy::Fixed {
                delays: vec![Duration::from_millis(500), Duration::from_millis(1000)],
            },
        };
        config
            .retry_async(
                || self.rpc("send", &rpc_params),
                |e| !matches!(e, super::error::Error::Rpc { .. }),
            )
            .await
    }

    /// Health check: hits the signal-cli check endpoint.
    pub async fn health(&self) -> bool {
        let result = self
            .client
            .get(&self.health_url)
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await;
        matches!(result, Ok(r) if r.status().is_success())
    }

    /// Poll for accumulated inbound messages.
    ///
    /// Calls the signal-cli `receive` RPC method, which returns all messages
    /// that have accumulated since the last call. Uses a longer timeout than
    /// standard RPC calls since receive may block briefly.
    #[instrument(skip(self))]
    pub async fn receive(&self, account: Option<&str>) -> Result<Vec<SignalEnvelope>> {
        let mut params = serde_json::Map::new();
        if let Some(acct) = account {
            params.insert(
                String::from("account"),
                serde_json::Value::String(acct.to_owned()),
            );
        }

        let id = aletheia_koina::uuid::uuid_v4();
        let params_value = serde_json::Value::Object(params);
        let request = RpcRequest {
            jsonrpc: "2.0",
            method: "receive",
            params: &params_value,
            id,
        };

        let body = serde_json::to_string(&request).context(error::JsonSnafu)?;

        let response = self
            .client
            .post(&self.rpc_url)
            .header("content-type", CONTENT_TYPE_JSON)
            .timeout(RECEIVE_TIMEOUT)
            .body(body)
            .send()
            .await
            .context(error::HttpSnafu)?;

        if response.status().as_u16() == 201 {
            return Ok(Vec::new());
        }

        let rpc_response: RpcResponse = response.json().await.context(error::HttpSnafu)?;

        if let Some(err) = rpc_response.error {
            return Err(error::RpcSnafu {
                code: err.code,
                message: err.message,
            }
            .build());
        }

        match rpc_response.result {
            Some(serde_json::Value::Array(items)) => {
                let envelopes = items
                    .into_iter()
                    .filter_map(|item| {
                        let env_value = item.get("envelope").cloned().unwrap_or(item);
                        serde_json::from_value::<SignalEnvelope>(env_value)
                            .inspect_err(|e| tracing::debug!(error = %e, "skipping unparseable envelope"))
                            .ok()
                    })
                    .collect();
                Ok(envelopes)
            }
            Some(_) | None => Ok(Vec::new()),
        }
    }

    /// The base RPC URL this client targets.
    #[must_use]
    #[expect(dead_code, reason = "Signal client RPC URL accessor for diagnostics")]
    pub(crate) fn rpc_url(&self) -> &str {
        &self.rpc_url
    }
}

impl std::fmt::Debug for SignalClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalClient")
            .field("rpc_url", &self.rpc_url)
            .finish_non_exhaustive()
    }
}

/// Parameters for the signal-cli `send` RPC method.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON key indexing on known-present keys"
)]
mod tests {
    use aletheia_organon::testing::install_crypto_provider;

    use super::*;

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
            ],
            "id": "test"
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&rpc_response))
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
            "result": [],
            "id": "test"
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&rpc_response))
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
            "error": {"code": -32601, "message": "method not found"},
            "id": "test"
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&rpc_response))
            .mount(&server)
            .await;

        let client = SignalClient::new(&server.uri()).expect("create client");
        let err = client.receive(None).await.expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("method not found"), "got: {msg}");
    }

    #[tokio::test]
    async fn receive_skips_malformed_envelopes() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;

        let rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "result": [
                {
                    "envelope": {
                        "sourceNumber": "+1111111111",
                        "dataMessage": {"message": "good"}
                    }
                },
                {
                    "envelope": "not-an-object"
                },
                {
                    "envelope": {
                        "sourceNumber": "+2222222222",
                        "dataMessage": {"message": "also good"}
                    }
                }
            ],
            "id": "test"
        });

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/v1/rpc"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&rpc_response))
            .mount(&server)
            .await;

        let client = SignalClient::new(&server.uri()).expect("create client");
        let envelopes = client.receive(None).await.expect("receive");

        assert_eq!(envelopes.len(), 2);
        assert_eq!(envelopes[0].source_number.as_deref(), Some("+1111111111"));
        assert_eq!(envelopes[1].source_number.as_deref(), Some("+2222222222"));
    }
}
