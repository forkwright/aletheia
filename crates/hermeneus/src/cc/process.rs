//! Claude Code subprocess management.
//!
//! Spawns `claude -p --output-format stream-json` and manages the child
//! process lifecycle: stdin feeding, stdout reading, timeout, and cleanup.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, Command};
use tracing::{debug, warn};

use crate::error::{self, Result};

use super::parse::{self, CcEvent};

/// Maximum total bytes of collected stream deltas before aborting.
///
/// WHY: Unbounded delta collection is an OOM risk if CC outputs unexpectedly
/// large content (e.g., tool dumps a huge file, runaway LLM response).
/// 10 MB is generous for any legitimate completion while preventing OOM.
const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// Maximum total number of stream delta lines before aborting.
///
/// WHY: Secondary guard alongside byte limit. A runaway subprocess could
/// emit many small lines that individually pass no single-line check.
const MAX_OUTPUT_LINES: usize = 100_000;

/// Maximum length of a system prompt passed to the CC subprocess.
///
/// WHY: The system prompt is passed as a command-line argument. An
/// excessively large prompt could exhaust argument space or cause the
/// subprocess to consume excessive memory during parsing.
const MAX_SYSTEM_PROMPT_BYTES: usize = 100 * 1024; // 100 KB

/// Extract the OAuth access token from the raw JSON content of a CC credentials file.
///
/// Separated from I/O so it can be unit-tested without touching the real filesystem
/// or the process environment.
#[cfg(test)]
fn parse_oauth_token_from_json(content: &str) -> std::io::Result<String> {
    let parsed: serde_json::Value =
        serde_json::from_str(content).map_err(|e| std::io::Error::other(e.to_string()))?;
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

fn scrub_cc_auth_env(cmd: &mut Command) {
    // WHY: Clear auth-related env vars so the CLI uses its own credential store.
    // The parent process may have ANTHROPIC_AUTH_TOKEN or CLAUDE_CODE_OAUTH_TOKEN
    // set (e.g. from a systemd EnvironmentFile). If the CLI inherits these, it
    // can send raw OAuth tokens to the API, which rejects them with 401. The CLI
    // handles OAuth exchange correctly through its own credential management.
    cmd.env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN");
}

/// Warn (once) that a non-zero `max_tokens` cannot be honored by the CC
/// subprocess provider, then ignore it.
///
/// WHY: The `claude` CLI exposes no max-output-token flag, so the cap is
/// genuinely unenforceable here. Hard-erroring on any non-zero value (the prior
/// behavior) broke every turn on the default zero-config CC provider — both the
/// scaffolded `[agents.defaults] max_output_tokens` and the hardcoded
/// recall-rewrite `max_tokens = 512` feed a non-zero value into this path. The
/// turn should still run; degrade gracefully by ignoring the unenforceable cap
/// with a single warning rather than failing the request. See aletheia#4158.
fn warn_unenforceable_max_tokens(max_tokens: u32) {
    use std::sync::Once;
    static WARN_ONCE: Once = Once::new();
    if max_tokens != 0 {
        WARN_ONCE.call_once(|| {
            tracing::warn!(
                max_tokens,
                "claude CLI cannot enforce a max output token limit; ignoring max_tokens for CC subprocess completions"
            );
        });
    }
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
}

/// Spawn CC and run a completion, collecting all output.
///
/// # Arguments
/// - `cc_binary`: path to the `claude` executable
/// - `working_directory`: optional subprocess cwd
/// - `model`: model identifier to pass via `--model`
/// - `system_prompt`: optional system prompt (passed via `--system-prompt`)
/// - `prompt`: the user prompt text (piped via stdin)
/// - `max_tokens`: maximum output tokens
/// - `timeout`: maximum wall-clock time before killing the process
///
/// # Errors
/// Returns errors on spawn failure, timeout, or if CC reports an error result.
#[expect(
    clippy::too_many_lines,
    reason = "sequential process lifecycle: spawn, feed stdin, wait, parse — splitting obscures the flow"
)]
#[tracing::instrument(skip_all)]
pub(crate) async fn run_completion(
    cc_binary: &PathBuf,
    working_directory: Option<&Path>,
    model: &str,
    system_prompt: Option<&str>,
    prompt: &str,
    max_tokens: u32,
    timeout: Duration,
) -> Result<CcOutput> {
    warn_unenforceable_max_tokens(max_tokens);

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
        // WHY(#4884): killing the child on drop ensures caller cancellation
        // (timeout, actor shutdown, future drop) terminates the subprocess
        // rather than leaving it running outside Aletheia's lifecycle.
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    scrub_cc_auth_env(&mut cmd);

    if let Some(sys) = system_prompt {
        if sys.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err(error::ApiRequestSnafu {
                message: format!(
                    "system prompt exceeds maximum size ({} bytes > {MAX_SYSTEM_PROMPT_BYTES} byte limit)",
                    sys.len(),
                ),
            }
            .build());
        }
        cmd.arg("--system-prompt").arg(sys);
    }

    if let Some(cwd) = working_directory {
        cmd.current_dir(cwd);
    }

    debug!(
        binary = %cc_binary.display(),
        cwd = ?working_directory.map(|path| path.display().to_string()),
        model = %model,
        "spawning CC subprocess"
    );

    let mut child = cmd.spawn().map_err(|e| {
        error::SubprocessFailureSnafu {
            provider: "cc".to_owned(),
            kind: error::SubprocessFailureKind::Spawn,
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
                let stderr_text = read_stderr_text(child.stderr.take(), "cc").await;
                return Err(error::SubprocessFailureSnafu {
                    provider: "cc".to_owned(),
                    kind: error::SubprocessFailureKind::Exit,
                    message: format!(
                        "process exited with {status}: {}",
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
            Err(error::SubprocessFailureSnafu {
                provider: "cc".to_owned(),
                kind: error::SubprocessFailureKind::Timeout,
                message: format!("timed out after {}s", timeout.as_secs()),
            }
            .build())
        }
    }
}

async fn read_stderr_text(stderr: Option<ChildStderr>, stream: &'static str) -> String {
    let Some(mut stderr) = stderr else {
        return String::new();
    };

    let mut buf = String::new();
    if let Err(e) = stderr.read_to_string(&mut buf).await {
        debug!(error = %e, stream, "failed to read subprocess stderr");
    }
    buf
}

/// Synthesize a human-readable error message from an error-subtype result
/// event's `errors` array + `terminal_reason`.
///
/// WHY(#3717): when CC terminates with `subtype = "error_max_turns"` (or any
/// other error subtype) it omits the `result` field entirely. Rather than
/// bubble `None` up to `result_to_response` — which would need its own None
/// handling — we synthesize a message here. Downstream `is_error = true`
/// propagation turns this into an `ApiRequest` error with readable text.
fn synthesize_error_text(errors: &[String], terminal_reason: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(reason) = terminal_reason {
        parts.push(format!("terminal_reason={reason}"));
    }
    if errors.is_empty() {
        parts.push("(no error messages reported)".to_owned());
    } else {
        parts.push(errors.join("; "));
    }
    parts.join(": ")
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

    let mut line_count: usize = 0;
    let mut total_bytes: usize = 0;
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
                    total_bytes = total_bytes.saturating_add(message.text.len());
                    if total_bytes > MAX_OUTPUT_BYTES {
                        return Err(error::ApiRequestSnafu {
                            message: format!(
                                "CC subprocess output exceeds {MAX_OUTPUT_BYTES} byte limit (collected {total_bytes} bytes)"
                            ),
                        }
                        .build());
                    }
                    if line_count >= MAX_OUTPUT_LINES {
                        return Err(error::ApiRequestSnafu {
                            message: format!(
                                "CC subprocess output exceeds {MAX_OUTPUT_LINES} line limit"
                            ),
                        }
                        .build());
                    }
                    line_count = line_count.saturating_add(1);
                    result_text.push_str(&message.text);
                }
            }
            CcEvent::Result {
                result,
                is_error: err,
                usage: u,
                cost_usd: c,
                duration_ms: d,
                session_id: s,
                errors,
                terminal_reason,
                ..
            } => {
                // WHY(#3717): error-subtype result events omit `result` and
                // populate `errors` + `terminal_reason` instead. Synthesize
                // a human-readable `result_text` so downstream is_error
                // propagation keeps working without losing the reason.
                result_text = result
                    .unwrap_or_else(|| synthesize_error_text(&errors, terminal_reason.as_deref()));
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
        // CC exited without a result event -- the fallback result was built
        // incrementally from assistant deltas.
        if result_text.is_empty() {
            return Err(error::ApiRequestSnafu {
                message: "CC subprocess produced no result event and no text output".to_owned(),
            }
            .build());
        }
    }

    debug!(
        result_len = result_text.len(),
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
    })
}

// (read_stream's tests live in the bottom #[cfg(test)] module.)

/// Spawn CC for streaming, calling `on_event` for each assistant delta.
///
/// Returns the final `CcOutput` after the stream completes.
#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "WHY(#4889): streaming keeps subprocess setup, timeout, and output handling in protocol order"
)]
#[tracing::instrument(skip_all)]
pub(crate) async fn run_streaming(
    cc_binary: &PathBuf,
    working_directory: Option<&Path>,
    model: &str,
    system_prompt: Option<&str>,
    prompt: &str,
    max_tokens: u32,
    timeout: Duration,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<CcOutput> {
    warn_unenforceable_max_tokens(max_tokens);

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
        // WHY(#4884): same kill_on_drop contract as run_completion — drop
        // propagates SIGKILL so no subprocess survives actor cancellation.
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    scrub_cc_auth_env(&mut cmd);

    if let Some(sys) = system_prompt {
        if sys.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err(error::ApiRequestSnafu {
                message: format!(
                    "system prompt exceeds maximum size ({} bytes > {MAX_SYSTEM_PROMPT_BYTES} byte limit)",
                    sys.len(),
                ),
            }
            .build());
        }
        cmd.arg("--system-prompt").arg(sys);
    }

    if let Some(cwd) = working_directory {
        cmd.current_dir(cwd);
    }

    debug!(
        binary = %cc_binary.display(),
        cwd = ?working_directory.map(|path| path.display().to_string()),
        model = %model,
        "spawning CC subprocess (streaming)"
    );

    let mut child = cmd.spawn().map_err(|e| {
        error::SubprocessFailureSnafu {
            provider: "cc".to_owned(),
            kind: error::SubprocessFailureKind::Spawn,
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

    let result = tokio::time::timeout(timeout, read_stream_with_callback(stdout, on_delta)).await;

    match result {
        Ok(Ok(output)) => {
            let status = child.wait().await.map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!("failed to wait for CC streaming process: {e}"),
                }
                .build()
            })?;

            if !status.success() && output.result_text.is_empty() {
                let stderr_text = read_stderr_text(child.stderr.take(), "cc streaming").await;
                return Err(error::SubprocessFailureSnafu {
                    provider: "cc".to_owned(),
                    kind: error::SubprocessFailureKind::Exit,
                    message: format!(
                        "process exited with {status}: {}",
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
            let _ = child.kill().await;
            Err(e)
        }
        Err(_elapsed) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "CC streaming subprocess timed out, killing"
            );
            let _ = child.kill().await;
            Err(error::SubprocessFailureSnafu {
                provider: "cc".to_owned(),
                kind: error::SubprocessFailureKind::Timeout,
                message: format!("timed out after {}s", timeout.as_secs()),
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

    let mut line_count: usize = 0;
    let mut total_bytes: usize = 0;
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
                    total_bytes = total_bytes.saturating_add(message.text.len());
                    if total_bytes > MAX_OUTPUT_BYTES {
                        return Err(error::ApiRequestSnafu {
                            message: format!(
                                "CC subprocess output exceeds {MAX_OUTPUT_BYTES} byte limit (collected {total_bytes} bytes)"
                            ),
                        }
                        .build());
                    }
                    if line_count >= MAX_OUTPUT_LINES {
                        return Err(error::ApiRequestSnafu {
                            message: format!(
                                "CC subprocess output exceeds {MAX_OUTPUT_LINES} line limit"
                            ),
                        }
                        .build());
                    }
                    line_count = line_count.saturating_add(1);
                    on_delta(&message.text);
                    result_text.push_str(&message.text);
                }
            }
            CcEvent::Result {
                result,
                is_error: err,
                usage: u,
                cost_usd: c,
                duration_ms: d,
                session_id: s,
                errors,
                terminal_reason,
                ..
            } => {
                // WHY(#3717): error-subtype result events omit `result`.
                // See read_stream.
                result_text = result
                    .unwrap_or_else(|| synthesize_error_text(&errors, terminal_reason.as_deref()));
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

    if !got_result && result_text.is_empty() {
        return Err(error::ApiRequestSnafu {
            message: "CC subprocess produced no result event and no text output".to_owned(),
        }
        .build());
    }

    Ok(CcOutput {
        result_text,
        is_error,
        usage,
        cost_usd,
        duration_ms,
        session_id,
    })
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod process_tests;

#[cfg(test)]
#[path = "process_run_tests.rs"]
mod process_run_tests;
