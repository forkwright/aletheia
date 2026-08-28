//! Matrix Client-Server API client.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::instrument;

use koina::http::CONTENT_TYPE_JSON;

use super::error::{self, Result};
use super::{MatrixRooms, MatrixSyncResponse, encode_path_segment};

/// Fallback default; runtime reads `MessagingConfig::rpc_timeout_secs`.
pub(crate) const RPC_TIMEOUT: Duration = Duration::from_secs(10);
/// Fallback default; runtime reads `MessagingConfig::receive_timeout_secs`.
pub(crate) const SYNC_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
struct SendMessageRequest<'a> {
    msgtype: &'static str,
    body: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "m.relates_to")]
    relates_to: Option<ThreadRelation<'a>>,
}

#[derive(Serialize)]
struct ThreadRelation<'a> {
    rel_type: &'static str,
    event_id: &'a str,
}

#[derive(Deserialize)]
struct MatrixErrorBody {
    errcode: Option<String>,
}

#[derive(Deserialize)]
struct MatrixSyncWire {
    next_batch: Option<String>,
    #[serde(default)]
    rooms: MatrixRooms,
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
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
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
    ///
    /// `txn_id` overrides the client-generated transaction ID; passing a
    /// stable key derived from the triggering inbound event makes a retried
    /// send idempotent at the homeserver (`PUT .../send/.../{txnId}` is
    /// the Matrix idempotency mechanism).
    #[instrument(
        skip(self, body, room_id),
        fields(room = %crate::redact::identifier(room_id))
    )]
    pub async fn send_text(
        &self,
        room_id: &str,
        body: &str,
        thread_id: Option<&str>,
        txn_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let txn_id = txn_id.map_or_else(koina::uuid::uuid_v4, ToOwned::to_owned);
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
            .map_err(reqwest::Error::without_url)
            .context(error::HttpSnafu)?;

        json_response(response).await
    }

    /// Perform a Matrix `/sync` request.
    #[instrument(skip(self, since))]
    pub(super) async fn sync(&self, since: Option<&str>) -> Result<MatrixSyncResponse> {
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
            .map_err(reqwest::Error::without_url)
            .context(error::HttpSnafu)?;

        let value = json_response(response).await?;
        let wire: MatrixSyncWire = serde_json::from_value(value).context(error::JsonSnafu)?;
        let Some(next_batch) = wire.next_batch else {
            return error::ProtocolSnafu {
                reason: "sync response omitted next_batch",
            }
            .fail();
        };
        if next_batch.trim().is_empty() {
            return error::ProtocolSnafu {
                reason: "sync response returned an empty next_batch",
            }
            .fail();
        }

        Ok(MatrixSyncResponse {
            next_batch,
            rooms: wire.rooms,
        })
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
        return response
            .json()
            .await
            .map_err(reqwest::Error::without_url)
            .context(error::HttpSnafu);
    }

    let status_code = status.as_u16();
    let body = response
        .text()
        .await
        .map_err(reqwest::Error::without_url)
        .context(error::HttpSnafu)?;
    let message = serde_json::from_str::<MatrixErrorBody>(&body)
        .ok()
        .and_then(|error_body| error_body.errcode)
        .unwrap_or_else(|| "Matrix request rejected".to_owned());

    error::ApiSnafu {
        status: status_code,
        message,
    }
    .fail()
}

fn normalize_homeserver(homeserver: &str) -> String {
    homeserver.trim_end_matches('/').to_owned()
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
            .send_text("!room:example.org", "hello", None, None)
            .await
            .expect("send");
        assert_eq!(
            result.get("event_id").and_then(serde_json::Value::as_str),
            Some("$event")
        );
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
        assert_eq!(response.next_batch, "s1");
    }

    #[tokio::test]
    async fn sync_rejects_missing_null_and_empty_next_batch() {
        install_crypto_provider();
        for body in [
            serde_json::json!({"rooms": {"join": {}}}),
            serde_json::json!({"next_batch": null, "rooms": {"join": {}}}),
            serde_json::json!({"next_batch": "", "rooms": {"join": {}}}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/_matrix/client/v3/sync"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;

            let client = MatrixClient::with_timeouts(
                &server.uri(),
                "token-123",
                Duration::from_secs(1),
                Duration::from_millis(10),
            )
            .expect("client");
            let Err(error) = client.sync(None).await else {
                panic!("invalid next_batch was accepted");
            };
            assert!(
                matches!(&error, error::Error::Protocol { .. }),
                "unexpected error: {error}"
            );
        }
    }

    #[tokio::test]
    async fn sync_does_not_follow_redirects() {
        install_crypto_provider();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/v3/sync"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "/redirected-with-credentials"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/redirected-with-credentials"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "next_batch": "s1"
            })))
            .expect(0)
            .mount(&server)
            .await;

        let client = MatrixClient::with_timeouts(
            &server.uri(),
            "token-123",
            Duration::from_secs(1),
            Duration::from_millis(10),
        )
        .expect("client");
        let Err(error) = client.sync(None).await else {
            panic!("redirect was followed");
        };
        assert!(
            matches!(&error, error::Error::Api { status: 302, .. }),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn send_text_uses_provided_txn_id() {
        install_crypto_provider();
        let server = MockServer::start().await;
        // NOTE: the exact path pins the percent-encoded idempotency key:
        // "reply:matrix:id:$event1" encodes ':' as %3A and '$' as %24.
        Mock::given(method("PUT"))
            .and(path(
                "/_matrix/client/v3/rooms/%21room%3Aexample.org/send/m.room.message/reply%3Amatrix%3Aid%3A%24event1",
            ))
            .and(header("authorization", "Bearer token-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "event_id": "$reply-event"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = MatrixClient::new(&server.uri(), "token-123").expect("client");
        let result = client
            .send_text(
                "!room:example.org",
                "hello",
                None,
                Some("reply:matrix:id:$event1"),
            )
            .await
            .expect("send");
        assert_eq!(
            result.get("event_id").and_then(serde_json::Value::as_str),
            Some("$reply-event")
        );
    }

    #[tokio::test]
    async fn api_error_surfaces_matrix_code_without_server_text() {
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
            .send_text("!room:example.org", "hello", None, None)
            .await
            .expect_err("forbidden");
        let rendered = err.to_string();
        assert!(rendered.contains("M_FORBIDDEN"));
        assert!(!rendered.contains("no access"));
    }
}
