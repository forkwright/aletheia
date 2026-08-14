//! Shared non-2xx response decoding for desktop views.
//!
//! Several views collapsed a failed fetch to `"server returned {status}"`,
//! discarding the canonical pylon error envelope (`{error: {code, message,
//! request_id, details}}`) that [`skene::api::error`] already knows how to
//! decode. This is that decode step, exposed once so every raw-fetch view
//! gets the same detail instead of re-deriving (or re-losing) it.

use skene::api::error::format_http_error_body;

/// Turn a non-2xx `reqwest::Response` into an operator-facing message.
///
/// Prefers the canonical pylon envelope (status + code + request ID +
/// structured details); falls back to the bare status and reason phrase
/// when the body is not that shape (e.g. a proxy-generated error page).
pub(crate) async fn decode_error_response(resp: reqwest::Response) -> String {
    let status = resp.status();
    let reason = status.canonical_reason().unwrap_or("HTTP error");
    // kanon:ignore RUST/no-result-unwrap-or-default — empty body on text() failure is acceptable; status code is the primary error signal
    let body = resp.text().await.unwrap_or_default();
    format_http_error_body(status.as_u16(), reason, &body)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helper failures should panic")]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;

    /// Serve one HTTP response on a local ephemeral port, then stop.
    fn serve_once(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let addr = listener.local_addr().expect("read local test server addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut buf = [0_u8; 2048];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write test response");
        });
        (format!("http://{addr}"), handle)
    }

    /// reqwest requires a TLS crypto provider installed before building any
    /// client, even for a plain `http://` URL. In production this happens
    /// at startup; tests install it explicitly (idempotent across tests in
    /// one process).
    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    /// Without this decode step, callers reduced any non-2xx response to
    /// `"server returned {status}"` -- discarding the pylon envelope's
    /// code, message, and request ID entirely. This asserts the real
    /// envelope survives the round trip.
    #[tokio::test]
    async fn decodes_canonical_pylon_envelope() {
        install_crypto();
        let body = r#"{"error":{"code":"validation_error","message":"path is required","request_id":"req-diff-1"}}"#;
        let (base_url, server) = serve_once("422 Unprocessable Entity", body);

        let client = reqwest::Client::new();
        let resp = client
            .get(&base_url)
            .send()
            .await
            .expect("request to local test server");
        let message = decode_error_response(resp).await;
        server.join().expect("test server thread should finish");

        assert!(message.contains("path is required"));
        assert!(message.contains("code validation_error"));
        assert!(message.contains("request_id req-diff-1"));
        // The bug this guards against: the message must not collapse to
        // just the status, which is all the old `format!("server returned
        // {status}")` call sites produced.
        assert_ne!(message, "422 Unprocessable Entity");
    }

    /// A body that is not the canonical envelope (e.g. a proxy error page)
    /// must still produce a readable message instead of panicking or
    /// silently emitting an empty string.
    #[tokio::test]
    async fn falls_back_to_status_and_reason_for_non_envelope_body() {
        install_crypto();
        let (base_url, server) = serve_once("503 Service Unavailable", "upstream timeout");

        let client = reqwest::Client::new();
        let resp = client
            .get(&base_url)
            .send()
            .await
            .expect("request to local test server");
        let message = decode_error_response(resp).await;
        server.join().expect("test server thread should finish");

        assert!(message.contains("503"));
    }
}
