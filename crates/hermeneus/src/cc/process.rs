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
                message: format!("CC subprocess timed out after {}s", timeout.as_secs()),
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
                    if stream_deltas.len() >= MAX_OUTPUT_LINES {
                        return Err(error::ApiRequestSnafu {
                            message: format!(
                                "CC subprocess output exceeds {MAX_OUTPUT_LINES} line limit"
                            ),
                        }
                        .build());
                    }
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

    let result = tokio::time::timeout(timeout, read_stream_with_callback(stdout, on_delta)).await;

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
                message: format!("CC subprocess timed out after {}s", timeout.as_secs()),
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
                    if stream_deltas.len() >= MAX_OUTPUT_LINES {
                        return Err(error::ApiRequestSnafu {
                            message: format!(
                                "CC subprocess output exceeds {MAX_OUTPUT_LINES} line limit"
                            ),
                        }
                        .build());
                    }
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
#[path = "process_tests.rs"]
mod process_tests;
