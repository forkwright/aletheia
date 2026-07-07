//! Matrix Client-Server API client.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::instrument;

use koina::http::CONTENT_TYPE_JSON;

use super::error::{self, Result};
use super::{MatrixSyncResponse, encode_path_segment};

/// Fallback default; runtime reads `MessagingConfig::rpc_timeout_secs`.
pub(crate) const RPC_TIMEOUT: Duration = Duration::from_secs(10);
/// Fallback default; runtime reads `MessagingConfig::receive_timeout_secs`.
pub(crate) const SYNC_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
struct SendMessageRequest<'a> {
    msgtype: &'static str,
    body: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "m.relates_to")]
    relates_to: Option<ThreadRelation<'a>>,
}

#[derive(Debug, Serialize)]
struct ThreadRelation<'a> {
    rel_type: &'static str,
    event_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct MatrixErrorBody {
    errcode: Option<String>,
    error: Option<String>,
}

/// Async client for a single Matrix account.
#[derive(Clone)]
pub struct MatrixClient {
    client: reqwest::Client,
    homeserver: String,
    access_token: String, // kanon:ignore RUST/plain-string-secret
    sync_timeout: Duration,
}

impl MatrixClient {
    /// Create a Matrix client with default timeouts.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(homeserver: &str, access_token: &str) -> Result<Self> {
        Self::with_timeouts(homeserver, access_token, RPC_TIMEOUT, SYNC_TIMEOUT)
    }

    /// Create a Matrix client with explicit timeout configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn with_timeouts(
        homeserver: &str,
        access_token: &str,
        rpc_timeout: Duration,
        sync_timeout: Duration,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(rpc_timeout)
            .build()
            .context(error::HttpSnafu)?;

        Ok(Self {
            client,
            homeserver: normalize_homeserver(homeserver),
            access_token: access_token.to_owned(),
            sync_timeout,
        })
    }

    /// Send an `m.room.message` text event to a room or room alias.
    #[instrument(skip(self, body), fields(room_id = %room_id))]
    pub async fn send_text(
        &self,
        room_id: &str,
        body: &str,
        thread_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let txn_id = thread_id.map_or_else(koina::uuid::uuid_v4, |event_id| {
            stable_reply_txn_id(event_id, body)
        });
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            self.homeserver,
            encode_path_segment(room_id),
            encode_path_segment(&txn_id)
        );
        let request = SendMessageRequest {
            msgtype: "m.text",
            body,
            relates_to: thread_id.map(|event_id| ThreadRelation {
                rel_type: "m.thread",
                event_id,
            }),
        };

        let response = self
            .client
            .put(url)
            .bearer_auth(&self.access_token)
            .header("content-type", CONTENT_TYPE_JSON)
            .json(&request)
            .send()
            .await
            .context(error::HttpSnafu)?;

        json_response(response).await
    }

    /// Perform a Matrix `/sync` request.
    #[instrument(skip(self, since))]
    pub async fn sync(&self, since: Option<&str>) -> Result<MatrixSyncResponse> {
        let mut query = vec![("timeout", self.sync_timeout.as_millis().to_string())];
        if let Some(since_token) = since {
            query.push(("since", since_token.to_owned()));
        }

        let url = format!("{}/_matrix/client/v3/sync", self.homeserver);
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.access_token)
            .query(&query)
            .timeout(self.sync_timeout + Duration::from_secs(2))
            .send()
            .await
            .context(error::HttpSnafu)?;

        let value = json_response(response).await?;
        serde_json::from_value(value).context(error::JsonSnafu)
    }

    /// Check whether the access token is accepted by the homeserver.
    pub async fn health(&self) -> bool {
        let url = format!("{}/_matrix/client/v3/account/whoami", self.homeserver);
        let result = self
            .client
            .get(url)
            .bearer_auth(&self.access_token)
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        matches!(result, Ok(response) if response.status().is_success())
    }
}

impl std::fmt::Debug for MatrixClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixClient")
            .field("homeserver", &self.homeserver)
            .finish_non_exhaustive()
    }
}

async fn json_response(response: reqwest::Response) -> Result<serde_json::Value> {
    let status = response.status();
    if status.is_success() {
        return response.json().await.context(error::HttpSnafu);
    }

    let status_code = status.as_u16();
    let body = response.text().await.context(error::HttpSnafu)?;
    let message = serde_json::from_str::<MatrixErrorBody>(&body)
        .ok()
        .and_then(|error_body| match (error_body.errcode, error_body.error) {
            (Some(code), Some(error)) => Some(format!("{code}: {error}")),
            (None, Some(error)) => Some(error),
            (Some(code), None) => Some(code),
            (None, None) => None,
        })
        .unwrap_or(body);

    error::ApiSnafu {
        status: status_code,
        message,
    }
    .fail()
}

fn normalize_homeserver(homeserver: &str) -> String {
    homeserver.trim_end_matches('/').to_owned()
}

fn stable_reply_txn_id(thread_id: &str, body: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for byte in b"matrix.reply\0"
        .iter()
        .copied()
        .chain(thread_id.bytes())
        .chain([0])
        .chain(body.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    format!("aletheia-reply-{hash:016x}")
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use organon::testing::install_crypto_provider;
    use wiremock::matchers::{body_json, header, method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn send_text_calls_room_send_endpoint() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path_regex(
                r"^/_matrix/client/v3/rooms/%21room%3Aexample\.org/send/m\.room\.message/[A-Za-z0-9_-]+$",
            ))
            .and(header("authorization", "Bearer token-123"))
            .and(body_json(serde_json::json!({
                "msgtype": "m.text",
                "body": "hello"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "event_id": "$event"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = MatrixClient::new(&server.uri(), "token-123").expect("client");
        let result = client
            .send_text("!room:example.org", "hello", None)
            .await
            .expect("send");
        assert_eq!(
            result.get("event_id").and_then(serde_json::Value::as_str),
            Some("$event")
        );
    }

    #[tokio::test]
    async fn send_text_uses_stable_transaction_id_for_thread_replies() {
        install_crypto_provider();
        let server = MockServer::start().await;
        let txn_id = stable_reply_txn_id("$event", "hello");
        Mock::given(method("PUT"))
            .and(path(format!(
                "/_matrix/client/v3/rooms/%21room%3Aexample.org/send/m.room.message/{txn_id}"
            )))
            .and(header("authorization", "Bearer token-123"))
            .and(body_json(serde_json::json!({
                "msgtype": "m.text",
                "body": "hello",
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$event"
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "event_id": "$reply"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = MatrixClient::new(&server.uri(), "token-123").expect("client");
        client
            .send_text("!room:example.org", "hello", Some("$event"))
            .await
            .expect("send");

        assert_eq!(txn_id, stable_reply_txn_id("$event", "hello"));
        assert_ne!(txn_id, stable_reply_txn_id("$event", "different"));
    }

    #[tokio::test]
    async fn sync_passes_since_token_and_timeout() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .and(header("authorization", "Bearer token-123"))
            .and(query_param("timeout", "50"))
            .and(query_param("since", "s0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s1",
                "rooms": {
                    "join": {
                        "!room:example.org": {
                            "timeline": {
                                "events": []
                            }
                        }
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = MatrixClient::with_timeouts(
            &server.uri(),
            "token-123",
            Duration::from_secs(1),
            Duration::from_millis(50),
        )
        .expect("client");
        let response = client.sync(Some("s0")).await.expect("sync");
        assert_eq!(response.next_batch.as_deref(), Some("s1"));
    }

    #[tokio::test]
    async fn api_error_surfaces_matrix_message() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "errcode": "M_FORBIDDEN",
                "error": "no access"
            })))
            .mount(&server)
            .await;

        let client = MatrixClient::new(&server.uri(), "token-123").expect("client");
        let err = client
            .send_text("!room:example.org", "hello", None)
            .await
            .expect_err("forbidden");
        assert!(err.to_string().contains("M_FORBIDDEN: no access"));
    }
}
