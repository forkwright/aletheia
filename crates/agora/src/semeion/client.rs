//! JSON-RPC client for the signal-cli HTTP daemon.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::instrument;
use uuid::Uuid;

use super::error::{self, Result};

const RPC_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: serde_json::Value,
    id: String,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    #[expect(dead_code, reason = "present in wire format, validated implicitly")]
    jsonrpc: String,
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
    #[expect(dead_code, reason = "present in wire format, used for correlation")]
    id: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// Async JSON-RPC client for a single signal-cli HTTP daemon instance.
pub struct SignalClient {
    client: reqwest::Client,
    rpc_url: String,
    health_url: String,
}

impl SignalClient {
    /// Create a new client targeting the given base URL.
    ///
    /// Normalizes the URL: strips trailing slashes, prepends `http://` if missing.
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
        params: serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let id = Uuid::new_v4().to_string();
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
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .context(error::HttpSnafu)?;

        // 201 = accepted, no body (signal-cli convention for fire-and-forget)
        if response.status().as_u16() == 201 {
            return Ok(None);
        }

        let rpc_response: RpcResponse =
            response.json().await.context(error::HttpSnafu)?;

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
    pub async fn send_message(
        &self,
        params: &SendParams,
    ) -> Result<Option<serde_json::Value>> {
        let rpc_params = params.to_rpc_value();
        let backoffs = [500u64, 1000];
        let mut last_err = None;

        for attempt in 0..=backoffs.len() {
            match self.rpc("send", rpc_params.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if matches!(e, super::error::Error::Rpc { .. }) {
                        return Err(e);
                    }
                    last_err = Some(e);
                    if attempt < backoffs.len() {
                        tracing::warn!(
                            attempt = attempt + 1,
                            backoff_ms = backoffs[attempt],
                            "signal send attempt failed, retrying"
                        );
                        tokio::time::sleep(Duration::from_millis(
                            backoffs[attempt],
                        ))
                        .await;
                    }
                }
            }
        }

        Err(last_err.expect("at least one attempt was made"))
    }

    /// Health check — hits the signal-cli check endpoint.
    pub async fn health(&self) -> bool {
        let result = self
            .client
            .get(&self.health_url)
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await;
        matches!(result, Ok(r) if r.status().is_success())
    }

    /// The base RPC URL this client targets.
    pub fn rpc_url(&self) -> &str {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
}

impl SendParams {
    /// Convert to the JSON-RPC wire format expected by signal-cli.
    ///
    /// Key transformations:
    /// - `recipient` is wrapped in an array (signal-cli convention)
    /// - `group_id` becomes `groupId` (camelCase)
    pub fn to_rpc_value(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        if let Some(ref msg) = self.message {
            map.insert(
                "message".to_owned(),
                serde_json::Value::String(msg.clone()),
            );
        }
        if let Some(ref r) = self.recipient {
            // signal-cli expects recipient as an array
            map.insert(
                "recipient".to_owned(),
                serde_json::Value::Array(vec![serde_json::Value::String(
                    r.clone(),
                )]),
            );
        }
        if let Some(ref g) = self.group_id {
            map.insert(
                "groupId".to_owned(),
                serde_json::Value::String(g.clone()),
            );
        }
        if let Some(ref a) = self.account {
            map.insert(
                "account".to_owned(),
                serde_json::Value::String(a.clone()),
            );
        }
        if let Some(ref att) = self.attachments {
            map.insert(
                "attachments".to_owned(),
                serde_json::Value::Array(
                    att.iter()
                        .map(|a| serde_json::Value::String(a.clone()))
                        .collect(),
                ),
            );
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
mod tests {
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
        // recipient is wrapped in an array
        assert_eq!(
            value["recipient"],
            serde_json::json!(["+1234567890"])
        );
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
        // group_id mapped to camelCase groupId
        assert_eq!(value["groupId"], "YWJjMTIz");
        assert_eq!(
            value["attachments"],
            serde_json::json!(["/tmp/photo.jpg"])
        );
    }

    #[test]
    fn client_creation() {
        let client = SignalClient::new("localhost:8080").expect("create client");
        assert_eq!(client.rpc_url(), "http://localhost:8080/api/v1/rpc");
    }
}
