//! Proves stdout stays pure MCP JSON-RPC once `main()`'s tracing init runs
//! through `koinon::telemetry::init_with_writer` — the property the
//! hand-rolled `EnvFilter`/`fmt().with_writer(stderr)` setup this replaced
//! (koinon#61) existed to guarantee, and the one thing an accidental swap
//! back to a stdout-default writer (`koinon::telemetry::init`, no
//! `_with_writer`) would silently break.
//!
//! Spawns the real compiled `xenodocheion` binary (not the in-process
//! `MemoryServer` the other integration tests drive) because the property
//! under test — which real OS stream a tracing event lands on — is a fact
//! about `main()`, not about the library API `tests/integration.rs`
//! exercises.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[expect(
    clippy::expect_used,
    reason = "test setup: panic on unexpected process-spawn/tempdir failure is acceptable"
)]
#[test]
fn startup_logs_go_to_stderr_and_stdout_stays_mcp_only() {
    let store_dir = tempfile::tempdir().expect("create temp store dir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_xenodocheion"))
        .env("ALETHEIA_MEMORY_MCP_STORE", store_dir.path())
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn xenodocheion");

    let mut stderr = child.stderr.take().expect("child stderr handle");

    // WHY a background reader thread rather than polling `stderr` directly:
    // `Read::read` on a pipe blocks until bytes arrive, and we need to keep
    // watching a deadline while it does. The thread streams each chunk
    // through a channel; the main thread accumulates and checks for the
    // "ready" line without ever blocking past the deadline itself.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader = std::thread::spawn(move || {
        let mut chunk = [0_u8; 512];
        loop {
            match stderr.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let Some(bytes) = chunk.get(..n) else {
                        break;
                    };
                    if tx.send(bytes.to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stderr_buf = String::new();
    while !stderr_buf.contains("memory MCP server ready on stdio") && Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => stderr_buf.push_str(&String::from_utf8_lossy(&bytes)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // `run()` never returns on its own here (it blocks in `serve_stdio()`
    // awaiting a peer over stdin, which this test never sends) — kill it
    // now that the startup log lines have been observed (or the deadline
    // passed), then drain whatever each stream produced.
    let _ = child.kill();
    let _ = child.wait();

    while let Ok(bytes) = rx.recv() {
        stderr_buf.push_str(&String::from_utf8_lossy(&bytes));
    }
    let _ = reader.join();

    let mut stdout_buf = Vec::new();
    let _ = child
        .stdout
        .take()
        .expect("child stdout handle")
        .read_to_end(&mut stdout_buf);

    assert!(
        stderr_buf.contains("opening knowledge store (fjall)"),
        "expected the startup log on stderr; got stderr={stderr_buf:?}"
    );
    assert!(
        stderr_buf.contains("memory MCP server ready on stdio"),
        "expected the ready log on stderr; got stderr={stderr_buf:?}"
    );
    assert!(
        stdout_buf.is_empty(),
        "stdout must carry only MCP JSON-RPC, never a log line; got stdout={:?}",
        String::from_utf8_lossy(&stdout_buf)
    );
}
