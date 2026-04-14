//! Claude Code subprocess management.
//!
//! Spawns `claude -p --output-format stream-json` and manages the child
//! process lifecycle: stdin feeding, stdout reading, timeout, and cleanup.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use koina::system::{Environment, RealSystem};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::error::{self, Result};

use super::parse::{self, CcEvent};

/// Extract the OAuth access token from the raw JSON content of a CC credentials file.
///
/// Separated from I/O so it can be unit-tested without touching the real filesystem
/// or the process environment.
fn parse_oauth_token_from_json(content: &str) -> std::io::Result<String> {
    let parsed: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    // WHY: serde_json::Value::get returns None on missing keys (vs the
    // panic from indexing), so this is the safe form of
    // `parsed["claudeAiOauth"]["accessToken"]`.
    parsed
        .get("claudeAiOauth")
        .and_then(|v| v.get("accessToken"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("no accessToken in credentials"))
}

/// Read the OAuth access token from CC's credential file.
///
/// WHY: CC's `--bare` mode disables OAuth. Instead of `--bare`, we inject
/// the token via `CLAUDE_CODE_OAUTH_TOKEN` env var, which CC accepts as an
/// override even without bare mode.
fn read_oauth_token() -> std::io::Result<String> {
    let home = RealSystem
        .var("HOME")
        .ok_or_else(|| std::io::Error::other("HOME is not set"))?;
    let path = std::path::Path::new(&home).join(".claude/.credentials.json");
    let content = std::fs::read_to_string(&path)?;
    parse_oauth_token_from_json(&content)
}

/// Outcome of a CC subprocess invocation.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "fields retained for diagnostics and future cost tracking callers"
    )
)]
pub(crate) struct CcOutput {
    /// The final result text.
    pub result_text: String,
    /// Whether CC reported an error.
    pub is_error: bool,
    /// Usage from the result event.
    pub usage: Option<parse::CcUsage>,
    /// Cost in USD (if reported).
    pub cost_usd: Option<f64>,
    /// Duration in ms (if reported).
    pub duration_ms: Option<u64>,
    /// CC session ID (if reported).
    pub session_id: Option<String>,
    /// All streaming text deltas collected in order.
    pub stream_deltas: Vec<String>,
}

/// Spawn CC and run a completion, collecting all output.
///
/// # Arguments
/// - `cc_binary`: path to the `claude` executable
/// - `model`: model identifier to pass via `--model`
/// - `system_prompt`: optional system prompt (passed via `--system-prompt`)
/// - `prompt`: the user prompt text (piped via stdin)
/// - `max_tokens`: maximum output tokens
/// - `timeout`: maximum wall-clock time before killing the process
///
/// # Errors
/// Returns errors on spawn failure, timeout, or if CC reports an error result.
#[tracing::instrument(skip_all)]
pub(crate) async fn run_completion(
    cc_binary: &PathBuf,
    model: &str,
    system_prompt: Option<&str>,
    prompt: &str,
    _max_tokens: u32,
    timeout: Duration,
) -> Result<CcOutput> {
    let mut cmd = Command::new(cc_binary);
    cmd.arg("-p")
        .arg("--verbose")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--model")
        .arg(model)
        .arg("--max-budget-usd")
        // WHY: max-budget-usd is a safety bound, not a billing limit.
        // Set generously to avoid premature abort on long completions.
        .arg("10.00")
        // WHY: --bare disables OAuth (isBareMode() → null). Instead, skip
        // CC's agent context via --no-session-persistence and override its
        // system prompt with aletheia's assembled prompt.
        .arg("--no-session-persistence")
        .arg("--dangerously-skip-permissions")
        .arg("--max-turns")
        .arg("1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // WHY: Clear auth-related env vars so the CLI uses its own credential store.
    // The parent process may have ANTHROPIC_AUTH_TOKEN or CLAUDE_CODE_OAUTH_TOKEN
    // set (e.g. from a systemd EnvironmentFile). If the CLI inherits these, it
    // sends the raw OAuth token to the API, which rejects it with 401. The CLI
    // handles OAuth exchange correctly through its own credential management.
    cmd.env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN");

    if let Some(sys) = system_prompt {
        cmd.arg("--system-prompt").arg(sys);
    }

    debug!(
        binary = %cc_binary.display(),
        model = %model,
        "spawning CC subprocess"
    );

    let mut child = cmd.spawn().map_err(|e| {
        error::ProviderInitSnafu {
            message: format!("failed to spawn claude CLI at {}: {e}", cc_binary.display()),
        }
        .build()
    })?;

    // Feed prompt via stdin, then close.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to write to CC stdin: {e}"),
            }
            .build()
        })?;
        // Drop closes the pipe, signaling EOF.
        drop(stdin);
    }

    // Read stdout line-by-line with timeout.
    let stdout = child.stdout.take().ok_or_else(|| {
        error::ApiRequestSnafu {
            message: "CC subprocess stdout not captured".to_owned(),
        }
        .build()
    })?;

    let result = tokio::time::timeout(timeout, read_stream(stdout)).await;

    match result {
        Ok(Ok(output)) => {
            // Wait for the process to exit (non-blocking since stdout is drained).
            let status = child.wait().await.map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!("failed to wait for CC process: {e}"),
                }
                .build()
            })?;

            if !status.success() && output.result_text.is_empty() {
                // Read stderr for diagnostics.
                let stderr_text = if let Some(mut stderr) = child.stderr.take() {
                    let mut buf = String::new();
                    let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
                    buf
                } else {
                    String::new()
                };
                return Err(error::ApiRequestSnafu {
                    message: format!(
                        "CC process exited with {status}: {}",
                        if stderr_text.is_empty() {
                            "(no stderr)"
                        } else {
                            stderr_text.trim()
                        }
                    ),
                }
                .build());
            }

            Ok(output)
        }
        Ok(Err(e)) => {
            // Stream read error. Kill the process.
            let _ = child.kill().await;
            Err(e)
        }
        Err(_elapsed) => {
            // Timeout. Kill the process.
            warn!(
                timeout_secs = timeout.as_secs(),
                "CC subprocess timed out, killing"
            );
            let _ = child.kill().await;
            Err(error::ApiRequestSnafu {
                message: format!(
                    "CC subprocess timed out after {}s",
                    timeout.as_secs()
                ),
            }
            .build())
        }
    }
}

/// Read CC's stdout stream, collecting assistant deltas and the final result.
///
/// Generic over the reader so unit tests can pass an in-memory buffer
/// (`tokio::io::Cursor` / `&[u8]`) without spawning a real subprocess.
async fn read_stream<R>(stdout: R) -> Result<CcOutput>
where
    R: AsyncRead + Unpin,
{
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let mut stream_deltas = Vec::new();
    let mut result_text = String::new();
    let mut is_error = false;
    let mut usage = None;
    let mut cost_usd = None;
    let mut duration_ms = None;
    let mut session_id = None;
    let mut got_result = false;

    while let Some(line) = lines.next_line().await.map_err(|e| {
        error::ApiRequestSnafu {
            message: format!("failed to read CC stdout: {e}"),
        }
        .build()
    })? {
        let Some(event) = parse::parse_event(&line) else {
            continue;
        };

        match event {
            CcEvent::Assistant { message } => {
                if !message.text.is_empty() {
                    stream_deltas.push(message.text);
                }
            }
            CcEvent::Result {
                result,
                is_error: err,
                usage: u,
                cost_usd: c,
                duration_ms: d,
                session_id: s,
                ..
            } => {
                result_text = result;
                is_error = err;
                usage = u;
                cost_usd = c;
                duration_ms = d;
                session_id = s;
                got_result = true;
            }
            CcEvent::System { .. } | CcEvent::RateLimit { .. } | CcEvent::User { .. } => {
                // Ignored — informational events that don't affect the response.
            }
        }
    }

    if !got_result {
        // CC exited without a result event -- synthesize from collected deltas.
        if stream_deltas.is_empty() {
            return Err(error::ApiRequestSnafu {
                message: "CC subprocess produced no result event and no text output".to_owned(),
            }
            .build());
        }
        result_text = stream_deltas.join("");
    }

    debug!(
        result_len = result_text.len(),
        deltas = stream_deltas.len(),
        cost = ?cost_usd,
        duration_ms = ?duration_ms,
        "CC subprocess completed"
    );

    Ok(CcOutput {
        result_text,
        is_error,
        usage,
        cost_usd,
        duration_ms,
        session_id,
        stream_deltas,
    })
}

// (read_stream's tests live in the bottom #[cfg(test)] module.)

/// Spawn CC for streaming, calling `on_event` for each assistant delta.
///
/// Returns the final `CcOutput` after the stream completes.
#[tracing::instrument(skip_all)]
pub(crate) async fn run_streaming(
    cc_binary: &PathBuf,
    model: &str,
    system_prompt: Option<&str>,
    prompt: &str,
    _max_tokens: u32,
    timeout: Duration,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<CcOutput> {
    let mut cmd = Command::new(cc_binary);
    cmd.arg("-p")
        .arg("--verbose")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--model")
        .arg(model)
        .arg("--max-budget-usd")
        .arg("10.00")
        .arg("--no-session-persistence")
        .arg("--dangerously-skip-permissions")
        .arg("--max-turns")
        .arg("1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // WHY: Same OAuth injection as run_completion.
    if let Ok(token) = read_oauth_token() {
        cmd.env("CLAUDE_CODE_OAUTH_TOKEN", &token);
    }

    if let Some(sys) = system_prompt {
        cmd.arg("--system-prompt").arg(sys);
    }

    debug!(
        binary = %cc_binary.display(),
        model = %model,
        "spawning CC subprocess (streaming)"
    );

    let mut child = cmd.spawn().map_err(|e| {
        error::ProviderInitSnafu {
            message: format!("failed to spawn claude CLI at {}: {e}", cc_binary.display()),
        }
        .build()
    })?;

    // Feed prompt.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to write to CC stdin: {e}"),
            }
            .build()
        })?;
        drop(stdin);
    }

    let stdout = child.stdout.take().ok_or_else(|| {
        error::ApiRequestSnafu {
            message: "CC subprocess stdout not captured".to_owned(),
        }
        .build()
    })?;

    let result =
        tokio::time::timeout(timeout, read_stream_with_callback(stdout, on_delta)).await;

    match result {
        Ok(Ok(output)) => {
            let _ = child.wait().await;
            Ok(output)
        }
        Ok(Err(e)) => {
            let _ = child.kill().await;
            Err(e)
        }
        Err(_elapsed) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "CC streaming subprocess timed out, killing"
            );
            let _ = child.kill().await;
            Err(error::ApiRequestSnafu {
                message: format!(
                    "CC subprocess timed out after {}s",
                    timeout.as_secs()
                ),
            }
            .build())
        }
    }
}

/// Read CC stdout with a callback for each text delta.
///
/// Generic over the reader for unit testing (see [`read_stream`]).
async fn read_stream_with_callback<R>(
    stdout: R,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<CcOutput>
where
    R: AsyncRead + Unpin,
{
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let mut stream_deltas = Vec::new();
    let mut result_text = String::new();
    let mut is_error = false;
    let mut usage = None;
    let mut cost_usd = None;
    let mut duration_ms = None;
    let mut session_id = None;
    let mut got_result = false;

    while let Some(line) = lines.next_line().await.map_err(|e| {
        error::ApiRequestSnafu {
            message: format!("failed to read CC stdout: {e}"),
        }
        .build()
    })? {
        let Some(event) = parse::parse_event(&line) else {
            continue;
        };

        match event {
            CcEvent::Assistant { message } => {
                if !message.text.is_empty() {
                    on_delta(&message.text);
                    stream_deltas.push(message.text);
                }
            }
            CcEvent::Result {
                result,
                is_error: err,
                usage: u,
                cost_usd: c,
                duration_ms: d,
                session_id: s,
                ..
            } => {
                result_text = result;
                is_error = err;
                usage = u;
                cost_usd = c;
                duration_ms = d;
                session_id = s;
                got_result = true;
            }
            CcEvent::System { .. } | CcEvent::RateLimit { .. } | CcEvent::User { .. } => {}
        }
    }

    if !got_result {
        if stream_deltas.is_empty() {
            return Err(error::ApiRequestSnafu {
                message: "CC subprocess produced no result event and no text output".to_owned(),
            }
            .build());
        }
        result_text = stream_deltas.join("");
    }

    Ok(CcOutput {
        result_text,
        is_error,
        usage,
        cost_usd,
        duration_ms,
        session_id,
        stream_deltas,
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    /// Write a shell script to a unique temp path and make it executable.
    ///
    /// Returns the script path. The caller is responsible for cleanup (or letting
    /// the OS reclaim the temp dir on process exit).
    fn write_script(name: &str, body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hermeneus_test_{name}_{}.sh", std::process::id()));
        let script = format!("#!/bin/sh\n{body}\n");
        // WHY: Open, write, fsync, close, then set permissions. Without fsync
        // the kernel may not have finished writing page cache to disk when we
        // exec the script, producing ETXTBSY (errno 26) on Linux.
        #[expect(clippy::disallowed_methods, reason = "test helper writes temp scripts, async not needed")]
        {
            use std::io::Write;
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(script.as_bytes()).unwrap();
            f.sync_all().unwrap();
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Build a multi-line stream-json buffer from individual event JSON strings.
    fn stream_buf(events: &[&str]) -> Vec<u8> {
        let mut out = String::new();
        for line in events {
            out.push_str(line);
            out.push('\n');
        }
        out.into_bytes()
    }

    #[tokio::test]
    async fn read_stream_assistant_then_result() {
        let buf = stream_buf(&[
            r#"{"type":"system","subtype":"init","session_id":"abc"}"#,
            r#"{"type":"assistant","message":{"type":"text","text":"Hello "}}"#,
            r#"{"type":"assistant","message":{"type":"text","text":"world"}}"#,
            r#"{"type":"result","subtype":"success","result":"Hello world","is_error":false,"session_id":"sess_42","cost_usd":0.002,"duration_ms":1500,"usage":{"input_tokens":12,"output_tokens":3}}"#,
        ]);
        let output = read_stream(buf.as_slice()).await.unwrap();
        assert_eq!(output.result_text, "Hello world");
        assert!(!output.is_error);
        assert_eq!(output.stream_deltas, vec!["Hello ", "world"]);
        assert_eq!(output.session_id.as_deref(), Some("sess_42"));
        assert_eq!(output.cost_usd, Some(0.002));
        assert_eq!(output.duration_ms, Some(1500));
        let usage = output.usage.unwrap();
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 3);
    }

    #[tokio::test]
    async fn read_stream_result_only() {
        // WHY: CC can emit a result event with no preceding assistant deltas
        // (e.g. when the response is short and CC batches it into the result).
        let buf = stream_buf(&[
            r#"{"type":"result","subtype":"success","result":"ok","is_error":false}"#,
        ]);
        let output = read_stream(buf.as_slice()).await.unwrap();
        assert_eq!(output.result_text, "ok");
        assert!(output.stream_deltas.is_empty());
    }

    #[tokio::test]
    async fn read_stream_no_result_synthesizes_from_deltas() {
        // WHY: If CC exits without emitting a result event we fall back to
        // joining the collected text deltas. This guards graceful degradation.
        let buf = stream_buf(&[
            r#"{"type":"assistant","message":{"type":"text","text":"part1 "}}"#,
            r#"{"type":"assistant","message":{"type":"text","text":"part2"}}"#,
        ]);
        let output = read_stream(buf.as_slice()).await.unwrap();
        assert_eq!(output.result_text, "part1 part2");
        assert_eq!(output.stream_deltas.len(), 2);
    }

    #[tokio::test]
    async fn read_stream_empty_input_errors() {
        // WHY: No result event AND no deltas is unrecoverable — caller needs to
        // know the subprocess produced nothing usable.
        let buf: Vec<u8> = Vec::new();
        let err = read_stream(buf.as_slice()).await.unwrap_err();
        assert!(
            err.to_string().contains("no result event"),
            "expected 'no result event' in error, got: {err}"
        );
    }

    #[tokio::test]
    async fn read_stream_blank_lines_skipped() {
        // WHY: parse_event treats blank lines as None — read_stream must not
        // crash on them and must not synthesize empty deltas.
        let buf = b"\n\n   \n{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"is_error\":false}\n";
        let output = read_stream(buf.as_slice()).await.unwrap();
        assert_eq!(output.result_text, "ok");
    }

    #[tokio::test]
    async fn read_stream_invalid_json_skipped() {
        // WHY: parse_event returns None on invalid JSON (logged as warning) —
        // read_stream should continue past it to subsequent valid events.
        let buf = stream_buf(&[
            "not json at all",
            r#"{"type":"result","subtype":"success","result":"recovered","is_error":false}"#,
        ]);
        let output = read_stream(buf.as_slice()).await.unwrap();
        assert_eq!(output.result_text, "recovered");
    }

    #[tokio::test]
    async fn read_stream_with_callback_invokes_for_each_delta() {
        let buf = stream_buf(&[
            r#"{"type":"assistant","message":{"type":"text","text":"a"}}"#,
            r#"{"type":"assistant","message":{"type":"text","text":"b"}}"#,
            r#"{"type":"assistant","message":{"type":"text","text":"c"}}"#,
            r#"{"type":"result","subtype":"success","result":"abc","is_error":false}"#,
        ]);
        let mut collected: Vec<String> = Vec::new();
        {
            let mut on_delta = |s: &str| collected.push(s.to_string());
            let output = read_stream_with_callback(buf.as_slice(), &mut on_delta)
                .await
                .unwrap();
            assert_eq!(output.result_text, "abc");
            assert_eq!(output.stream_deltas, vec!["a", "b", "c"]);
        }
        assert_eq!(collected, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn read_stream_with_callback_skips_empty_text() {
        // WHY: An assistant event with empty text should not invoke the
        // callback (matches read_stream behavior to avoid empty UI updates).
        let buf = stream_buf(&[
            r#"{"type":"assistant","message":{"type":"text","text":""}}"#,
            r#"{"type":"assistant","message":{"type":"text","text":"real"}}"#,
            r#"{"type":"result","subtype":"success","result":"real","is_error":false}"#,
        ]);
        let mut count = 0_u32;
        let mut on_delta = |_: &str| count += 1;
        let _ = read_stream_with_callback(buf.as_slice(), &mut on_delta)
            .await
            .unwrap();
        assert_eq!(count, 1, "callback should fire once for the non-empty delta");
    }

    #[tokio::test]
    async fn read_stream_propagates_is_error_flag() {
        // WHY: A `result` event with is_error=true is preserved on the output
        // so the provider layer can map it to a hermeneus::Error variant.
        let buf = stream_buf(&[
            r#"{"type":"result","subtype":"error","result":"rate limit","is_error":true}"#,
        ]);
        let output = read_stream(buf.as_slice()).await.unwrap();
        assert!(output.is_error);
        assert_eq!(output.result_text, "rate limit");
    }

    // ── parse_oauth_token_from_json ───────────────────────────────────────────
    // WHY: Tests target the JSON-parsing helper rather than read_oauth_token
    // directly. read_oauth_token wraps parse_oauth_token_from_json with I/O
    // and HOME resolution. Testing the parser in isolation avoids env var
    // manipulation (unsafe in Rust 2024) while covering all key branches.

    #[test]
    fn parse_oauth_token_succeeds_with_valid_credentials() {
        // WHY: Happy path — valid JSON with the expected key hierarchy returns
        // the access token string without error.
        let json = r#"{"claudeAiOauth":{"accessToken":"test-token-abc123"}}"#;
        let token = parse_oauth_token_from_json(json).unwrap();
        assert_eq!(token, "test-token-abc123");
    }

    #[test]
    fn parse_oauth_token_fails_when_access_token_key_absent() {
        // WHY: JSON exists but lacks the `accessToken` key — must return an
        // error rather than silently returning empty, so callers don't inject
        // a blank token into the subprocess environment.
        let json = r#"{"claudeAiOauth":{"someOtherKey":"value"}}"#;
        let err = parse_oauth_token_from_json(json).unwrap_err();
        assert!(
            err.to_string().contains("no accessToken"),
            "expected 'no accessToken' in error, got: {err}"
        );
    }

    #[test]
    fn parse_oauth_token_fails_when_top_level_key_absent() {
        // WHY: JSON without the `claudeAiOauth` wrapper must fail cleanly —
        // this covers flat credential formats that don't contain CC OAuth data.
        let json = r#"{"someOtherProvider":{"accessToken":"irrelevant"}}"#;
        let err = parse_oauth_token_from_json(json).unwrap_err();
        assert!(
            err.to_string().contains("no accessToken"),
            "expected 'no accessToken' in error, got: {err}"
        );
    }

    #[test]
    fn parse_oauth_token_fails_on_malformed_json() {
        // WHY: Malformed credentials (e.g. truncated write) must return an
        // error, not panic. The caller silently skips OAuth injection on error.
        let err = parse_oauth_token_from_json("not-json{{{").unwrap_err();
        assert!(!err.to_string().is_empty(), "error message must not be empty");
    }

    #[test]
    fn parse_oauth_token_fails_on_empty_input() {
        let err = parse_oauth_token_from_json("").unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    // ── run_completion ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_completion_spawn_failure_reports_binary_path() {
        // WHY: A missing or non-executable binary must produce a ProviderInit
        // error that names the bad path so the operator can diagnose it.
        let binary = PathBuf::from("/nonexistent/path/to/claude-binary");
        let err = run_completion(
            &binary,
            "claude-test-model",
            None,
            "hello",
            1024,
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("/nonexistent/path/to/claude-binary"),
            "error must include the binary path, got: {msg}"
        );
        assert!(
            msg.contains("provider init failed"),
            "error must be ProviderInit variant, got: {msg}"
        );
    }

    #[tokio::test]
    async fn run_completion_success_collects_output() {
        // WHY: End-to-end subprocess path with a real script. Verifies that
        // run_completion feeds stdin, reads stdout stream-json, and returns
        // a populated CcOutput with the result text and delta list intact.
        let script = write_script(
            "completion_ok",
            // Discard all args and stdin; emit a two-event stream.
            r#"cat > /dev/null
printf '{"type":"assistant","message":{"type":"text","text":"hello "}}\n'
printf '{"type":"assistant","message":{"type":"text","text":"world"}}\n'
printf '{"type":"result","subtype":"success","result":"hello world","is_error":false,"session_id":"s1","cost_usd":0.001,"duration_ms":200}\n'"#,
        );

        let output = run_completion(
            &script,
            "claude-test-model",
            None,
            "prompt text",
            1024,
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        assert_eq!(output.result_text, "hello world");
        assert!(!output.is_error);
        assert_eq!(output.stream_deltas, vec!["hello ", "world"]);
        assert_eq!(output.session_id.as_deref(), Some("s1"));
        assert_eq!(output.cost_usd, Some(0.001));
        assert_eq!(output.duration_ms, Some(200));

        let _ = fs::remove_file(&script);
    }

    #[tokio::test]
    async fn run_completion_with_system_prompt_succeeds() {
        // WHY: Verifies the --system-prompt branch executes without error.
        // The actual arg passing is structural (cmd.arg) and not visible from
        // outside the subprocess, but the round-trip proves the branch is taken.
        let script = write_script(
            "completion_sys",
            r#"cat > /dev/null
printf '{"type":"result","subtype":"success","result":"sys ok","is_error":false}\n'"#,
        );

        let output = run_completion(
            &script,
            "claude-test-model",
            Some("You are a helpful assistant."),
            "prompt",
            512,
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        assert_eq!(output.result_text, "sys ok");
        let _ = fs::remove_file(&script);
    }

    #[tokio::test]
    async fn run_completion_timeout_returns_error() {
        // WHY: Subprocess that sleeps past the deadline must be killed and
        // must surface a timeout error message that includes the duration.
        let script = write_script("completion_sleep", "sleep 30");

        let err = run_completion(
            &script,
            "claude-test-model",
            None,
            "prompt",
            1024,
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("timed out"),
            "error must mention timeout, got: {msg}"
        );
        let _ = fs::remove_file(&script);
    }

    #[tokio::test]
    async fn run_completion_nonzero_exit_with_stderr_captured() {
        // WHY: When CC emits a result event with empty result text and then exits
        // nonzero, run_completion falls through to the stderr-capture branch
        // (`!status.success() && result_text.is_empty()`). The captured stderr
        // must appear in the error so the operator can see the failure reason
        // (e.g. "not logged in", "invalid model").
        //
        // The script emits a result event with an empty result string so that
        // read_stream returns Ok(CcOutput { result_text: "", ... }), which
        // triggers the stderr-capture branch when the exit code is nonzero.
        let script = write_script(
            "completion_fail",
            r#"cat > /dev/null
printf '{"type":"result","subtype":"error","result":"","is_error":true}\n'
printf 'OAuth token rejected\n' >&2
exit 1"#,
        );

        let err = run_completion(
            &script,
            "claude-test-model",
            None,
            "prompt",
            1024,
            Duration::from_secs(10),
        )
        .await
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("OAuth token rejected"),
            "stderr must appear in error message, got: {msg}"
        );
        let _ = fs::remove_file(&script);
    }

    // ── run_streaming ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_streaming_spawn_failure_reports_binary_path() {
        // WHY: Mirrors run_completion spawn failure — streaming entry point must
        // also surface the bad binary path in a ProviderInit error.
        let binary = PathBuf::from("/nonexistent/path/to/claude-stream");
        let mut on_delta = |_: &str| {};
        let err = run_streaming(
            &binary,
            "claude-test-model",
            None,
            "hello",
            1024,
            Duration::from_secs(5),
            &mut on_delta,
        )
        .await
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("/nonexistent/path/to/claude-stream"),
            "error must include binary path, got: {msg}"
        );
        assert!(
            msg.contains("provider init failed"),
            "error must be ProviderInit variant, got: {msg}"
        );
    }

    #[tokio::test]
    async fn run_streaming_invokes_callback_for_each_delta() {
        // WHY: run_streaming must call on_delta once per assistant text event,
        // in order, with the exact text. This is the primary contract of the
        // streaming API — callers relay deltas to UI/SSE consumers in real time.
        let script = write_script(
            "streaming_deltas",
            r#"cat > /dev/null
printf '{"type":"assistant","message":{"type":"text","text":"chunk1"}}\n'
printf '{"type":"assistant","message":{"type":"text","text":"chunk2"}}\n'
printf '{"type":"assistant","message":{"type":"text","text":"chunk3"}}\n'
printf '{"type":"result","subtype":"success","result":"chunk1chunk2chunk3","is_error":false}\n'"#,
        );

        let mut collected: Vec<String> = Vec::new();
        let mut on_delta = |s: &str| collected.push(s.to_owned());

        let output = run_streaming(
            &script,
            "claude-test-model",
            None,
            "prompt",
            1024,
            Duration::from_secs(10),
            &mut on_delta,
        )
        .await
        .unwrap();

        assert_eq!(output.result_text, "chunk1chunk2chunk3");
        assert_eq!(output.stream_deltas, vec!["chunk1", "chunk2", "chunk3"]);
        assert_eq!(
            collected,
            vec!["chunk1", "chunk2", "chunk3"],
            "on_delta must be called in order with each text delta"
        );

        let _ = fs::remove_file(&script);
    }

    #[tokio::test]
    async fn run_streaming_timeout_returns_error() {
        // WHY: Same timeout contract as run_completion — streaming subprocess
        // that stalls must be killed and must return a timeout error.
        let script = write_script("streaming_sleep", "sleep 30");

        let mut on_delta = |_: &str| {};
        let err = run_streaming(
            &script,
            "claude-test-model",
            None,
            "prompt",
            1024,
            Duration::from_millis(100),
            &mut on_delta,
        )
        .await
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("timed out"),
            "error must mention timeout, got: {msg}"
        );
        let _ = fs::remove_file(&script);
    }
}
