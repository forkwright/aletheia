//! `aletheia health`: HTTP health check against a running instance.

use clap::Args;
use snafu::prelude::*;

use pylon::client::GatewayClient;

use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct HealthArgs {
    /// Server URL to check
    #[arg(long, default_value = crate::cli::DEFAULT_GATEWAY_URL)]
    pub url: String,
}

pub(crate) async fn run(args: &HealthArgs) -> Result<()> {
    validate_args(args)?;
    let client =
        GatewayClient::new(&args.url, None).whatever_context("failed to build HTTP client")?;
    match client.liveness().await {
        Ok(liveness) => {
            println!("OK — {}", liveness.status);
        }
        // WHY(review #5100): only a transport-level failure means the server
        // is unreachable. Auth/Server(e.g. 503)/Decode all mean a response
        // came back from something listening at the URL, so folding them
        // into "cannot reach" would misdiagnose a running-but-unhealthy
        // server as not running at all.
        Err(pylon::client::Error::Request { .. }) => {
            whatever!(
                "FAILED: cannot reach {}\n  \
                 Is the server running? Start it with: aletheia",
                args.url
            );
        }
        Err(e) => {
            whatever!("FAILED: health check failed: {e}");
        }
    }
    Ok(())
}

fn validate_args(args: &HealthArgs) -> Result<()> {
    if let Err(e) = reqwest::Url::parse(&args.url) {
        whatever!("--url is not a valid URL: {e} (got {:?})", args.url);
    }
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn args_with(url: &str) -> HealthArgs {
        HealthArgs {
            url: url.to_owned(),
        }
    }

    #[test]
    fn default_url_matches_the_shared_gateway_default() {
        use clap::Parser as _;

        #[derive(Debug, clap::Parser)]
        struct Wrapper {
            #[command(flatten)]
            health: HealthArgs,
        }

        // PROOF(#5100): `health` no longer restates its own gateway-URL
        // default — it resolves to the one constant every HTTP-backed
        // command shares.
        let wrapper = Wrapper::try_parse_from(["health"]).unwrap();
        assert_eq!(wrapper.health.url, crate::cli::DEFAULT_GATEWAY_URL);
    }

    #[test]
    fn validate_rejects_malformed_url() {
        let err = validate_args(&args_with("not a url")).unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_accepts_well_formed_url() {
        validate_args(&args_with("http://127.0.0.1:18789")).unwrap();
    }

    /// PROOF(#5100): `health` now goes through `pylon::client::GatewayClient`
    /// (shared URL resolution, headers, decode/error path) instead of a
    /// hand-rolled `reqwest::get` — this fails before the migration (no
    /// server-side change) because there is no shared client to reach the
    /// stub through, and passes after because `GatewayClient::liveness()`
    /// decodes exactly the body the stub returns.
    #[tokio::test]
    async fn run_reports_ok_through_the_shared_client() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        organon::testing::install_crypto_provider();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = socket.read(&mut buf).await;
            let body = r#"{"status":"healthy"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });

        let args = args_with(&format!("http://{addr}"));
        run(&args).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn run_fails_when_nothing_is_listening() {
        organon::testing::install_crypto_provider();
        let err = run(&args_with("http://127.0.0.1:1")).await.unwrap_err();
        assert!(err.to_string().contains("cannot reach"), "got: {err}");
    }

    /// PROOF(review #5100): a server that answers but reports itself
    /// unhealthy (HTTP 503) is not the same failure as no server being
    /// there at all — the message must say so (503/unhealthy) and must NOT
    /// claim the server is unreachable, which would send an operator
    /// chasing the wrong problem.
    #[tokio::test]
    async fn run_reports_the_status_when_the_server_answers_unhealthy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        organon::testing::install_crypto_provider();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = socket.read(&mut buf).await;
            let body = r#"{"status":"unhealthy"}"#;
            let response = format!(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });

        let err = run(&args_with(&format!("http://{addr}")))
            .await
            .unwrap_err();
        server.await.unwrap();

        let msg = err.to_string();
        assert!(msg.contains("503"), "got: {msg}");
        assert!(msg.contains("unhealthy"), "got: {msg}");
        assert!(
            !msg.contains("cannot reach"),
            "a running-but-unhealthy server must not read as unreachable: {msg}"
        );
    }
}
