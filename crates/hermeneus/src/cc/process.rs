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
    let parsed: serde_json::Value = serde_json::from_str(&content)
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

    // WHY: Inject OAuth token from the credential file so CC authenticates
    // without needing its own login state. CLAUDE_CODE_OAUTH_TOKEN bypasses
    // CC's secure storage check and works even when CC reports "not logged in".
    if let Ok(token) = read_oauth_token() {
        cmd.env("CLAUDE_CODE_OAUTH_TOKEN", &token);
    }

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
            CcEvent::System { .. } | CcEvent::RateLimit { .. } => {
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
            CcEvent::System { .. } | CcEvent::RateLimit { .. } => {}
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
    use super::*;

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
}
