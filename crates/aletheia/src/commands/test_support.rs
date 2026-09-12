//! Shared test-only helpers for exercising the `guard_knowledge_lock` family
//! of instance-lock checks (#7205) without depending on a real
//! `aletheia` server.

#![expect(clippy::expect_used, reason = "test fixture setup")]

/// Bind an ephemeral local listener that answers exactly one HTTP request
/// with `200 OK`, then exits.
///
/// WHY: `is_knowledge_server_running` only checks the response status of a
/// GET against the health route, so a stub this simple is enough to make a
/// guard's "server is running" branch fire in a test — no real `aletheia`
/// server, and no dependency on another crate's test harness, required.
///
/// Returns the listener's base URL and the task driving it; `.await` the
/// task after exercising the guarded command to confirm the health probe
/// was actually made.
pub(crate) async fn spawn_stub_running_server() -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stub server binds an ephemeral port");
    let addr = listener.local_addr().expect("listener has a local addr");
    let handle = tokio::spawn(async move {
        if let Ok(Ok((mut socket, _))) =
            tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept()).await
        {
            let mut buf = [0_u8; 1024];
            let _ = socket.read(&mut buf).await;
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
            let _ = socket.write_all(response).await;
        }
    });
    (format!("http://{addr}"), handle)
}
