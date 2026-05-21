//! Kimi subprocess management.
//!
//! Spawns `kimi --print --afk --yolo --thinking -w <cwd> -p <prompt>` and
//! manages the child process lifecycle: stdout reading, timeout, and cleanup.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::error::{self, Result};

use super::parse::{self, KimiUsage};

/// Maximum total bytes of collected subprocess output before aborting.
///
/// WHY: Unbounded output collection is an OOM risk if the CLI emits
/// unexpectedly large content. 10 MB is generous for legitimate completions.
const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum total number of stdout lines before aborting.
///
/// WHY: Secondary guard alongside byte limit for many-small-line output.
const MAX_OUTPUT_LINES: usize = 100_000;

/// Maximum length of a system prompt included in the Kimi prompt argument.
///
/// WHY: The final prompt is passed as a command-line argument. An excessively
/// large system prompt can exhaust argument space or make subprocess parsing
/// consume excessive memory.
const MAX_SYSTEM_PROMPT_BYTES: usize = 100 * 1024;

fn scrub_kimi_auth_env(cmd: &mut Command) {
    // WHY: The Kimi CLI owns its OAuth credential flow. If MOONSHOT_API_KEY is
    // inherited from the parent, the subprocess can accidentally switch to
    // API-token auth instead of the local CLI credentials.
    cmd.env_remove("MOONSHOT_API_KEY");
}

fn ensure_max_tokens_supported(max_tokens: u32) -> Result<()> {
    if max_tokens == 0 {
        return Ok(());
    }

    Err(error::ApiRequestSnafu {
        message: format!(
            "Kimi subprocess cannot enforce max_tokens={max_tokens}: kimi CLI does not expose a max output token flag"
        ),
    }
    .build())
}

fn compose_prompt(system_prompt: Option<&str>, prompt: &str) -> Result<String> {
    match system_prompt {
        Some(system) => {
            if system.len() > MAX_SYSTEM_PROMPT_BYTES {
                return Err(error::ApiRequestSnafu {
                    message: format!(
                        "system prompt exceeds maximum size ({} bytes > {MAX_SYSTEM_PROMPT_BYTES} byte limit)",
                        system.len(),
                    ),
                }
                .build());
            }
            Ok(format!("System:\n{system}\n\nUser:\n{prompt}"))
        }
        None => Ok(prompt.to_owned()),
    }
}

fn build_kimi_command(kimi_binary: &Path, cwd: &Path, prompt: &str) -> Command {
    let mut cmd = Command::new(kimi_binary);
    cmd.arg("--print")
        .arg("--afk")
        .arg("--yolo")
        .arg("--thinking")
        .arg("-w")
        .arg(cwd)
        .arg("-p")
        .arg(prompt)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    scrub_kimi_auth_env(&mut cmd);
    cmd
}

/// Outcome of a Kimi subprocess invocation.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "fields retained for diagnostics and future cost tracking callers"
    )
)]
pub(crate) struct KimiOutput {
    /// The final response text.
    pub result_text: String,
    /// Usage from the final status update, if Kimi reported it.
    pub usage: Option<KimiUsage>,
    /// Kimi message ID, if reported.
    pub message_id: Option<String>,
    /// All text deltas collected in order.
    pub stream_deltas: Vec<String>,
}

/// Spawn Kimi and run a completion, collecting all output.
///
/// # Errors
/// Returns errors on spawn failure, timeout, malformed output, or unsupported
/// request options.
#[tracing::instrument(skip_all)]
pub(crate) async fn run_completion(
    kimi_binary: &Path,
    cwd: &Path,
    system_prompt: Option<&str>,
    prompt: &str,
    max_tokens: u32,
    timeout: Duration,
) -> Result<KimiOutput> {
    ensure_max_tokens_supported(max_tokens)?;
    let prompt = compose_prompt(system_prompt, prompt)?;
    let mut child = spawn_kimi(kimi_binary, cwd, &prompt, "spawning Kimi subprocess")?;

    let stdout = child.stdout.take().ok_or_else(|| {
        error::ApiRequestSnafu {
            message: "Kimi subprocess stdout not captured".to_owned(),
        }
        .build()
    })?;

    let result = tokio::time::timeout(timeout, read_stream(stdout)).await;

    match result {
        Ok(Ok(output)) => {
            let status = child.wait().await.map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!("failed to wait for Kimi process: {e}"),
                }
                .build()
            })?;

            if !status.success() && output.result_text.is_empty() {
                let stderr_text = read_stderr(child.stderr.take()).await;
                return Err(error::ApiRequestSnafu {
                    message: format!(
                        "Kimi process exited with {status}: {}",
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
            kill_child(&mut child, "Kimi subprocess").await;
            Err(e)
        }
        Err(_elapsed) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "Kimi subprocess timed out, killing"
            );
            kill_child(&mut child, "Kimi subprocess").await;
            Err(error::ApiRequestSnafu {
                message: format!("Kimi subprocess timed out after {}s", timeout.as_secs()),
            }
            .build())
        }
    }
}

/// Spawn Kimi for streaming, calling `on_delta` for each text part.
///
/// Returns the final `KimiOutput` after the stream completes.
#[tracing::instrument(skip_all)]
pub(crate) async fn run_streaming(
    kimi_binary: &Path,
    cwd: &Path,
    system_prompt: Option<&str>,
    prompt: &str,
    max_tokens: u32,
    timeout: Duration,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<KimiOutput> {
    ensure_max_tokens_supported(max_tokens)?;
    let prompt = compose_prompt(system_prompt, prompt)?;
    let mut child = spawn_kimi(
        kimi_binary,
        cwd,
        &prompt,
        "spawning Kimi subprocess (streaming)",
    )?;

    let stdout = child.stdout.take().ok_or_else(|| {
        error::ApiRequestSnafu {
            message: "Kimi subprocess stdout not captured".to_owned(),
        }
        .build()
    })?;

    let result = tokio::time::timeout(timeout, read_stream_with_callback(stdout, on_delta)).await;

    match result {
        Ok(Ok(output)) => {
            if let Err(e) = child.wait().await {
                warn!(error = %e, "failed to wait for Kimi streaming process");
            }
            Ok(output)
        }
        Ok(Err(e)) => {
            kill_child(&mut child, "Kimi streaming subprocess").await;
            Err(e)
        }
        Err(_elapsed) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "Kimi streaming subprocess timed out, killing"
            );
            kill_child(&mut child, "Kimi streaming subprocess").await;
            Err(error::ApiRequestSnafu {
                message: format!("Kimi subprocess timed out after {}s", timeout.as_secs()),
            }
            .build())
        }
    }
}

fn spawn_kimi(
    kimi_binary: &Path,
    cwd: &Path,
    prompt: &str,
    log_message: &'static str,
) -> Result<tokio::process::Child> {
    debug!(
        binary = %kimi_binary.display(),
        cwd = %cwd.display(),
        "{}",
        log_message
    );

    build_kimi_command(kimi_binary, cwd, prompt)
        .spawn()
        .map_err(|e| {
            error::ProviderInitSnafu {
                message: format!("failed to spawn kimi CLI at {}: {e}", kimi_binary.display()),
            }
            .build()
        })
}

async fn read_stderr(stderr: Option<tokio::process::ChildStderr>) -> String {
    let Some(mut stderr) = stderr else {
        return String::new();
    };
    let mut buf = String::new();
    if let Err(e) = stderr.read_to_string(&mut buf).await {
        warn!(error = %e, "failed to read Kimi stderr");
        return String::new();
    }
    buf
}

async fn kill_child(child: &mut tokio::process::Child, name: &str) {
    if let Err(e) = child.kill().await {
        warn!(error = %e, process = name, "failed to kill subprocess");
    }
}

async fn read_stream<R>(stdout: R) -> Result<KimiOutput>
where
    R: AsyncRead + Unpin,
{
    let mut ignore_delta = |_: &str| {};
    read_stream_with_callback(stdout, &mut ignore_delta).await
}

/// Read Kimi stdout with a callback for each parsed text part.
async fn read_stream_with_callback<R>(
    stdout: R,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<KimiOutput>
where
    R: AsyncRead + Unpin,
{
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let mut stream_deltas = Vec::new();
    let mut total_bytes: usize = 0;
    let mut total_lines: usize = 0;
    let mut usage = KimiUsage::default();
    let mut has_usage = false;
    let mut message_id = None;
    let mut in_text_part = false;

    while let Some(line) = lines.next_line().await.map_err(|e| {
        error::ApiRequestSnafu {
            message: format!("failed to read Kimi stdout: {e}"),
        }
        .build()
    })? {
        total_lines = total_lines.saturating_add(1);
        total_bytes = total_bytes.saturating_add(line.len()).saturating_add(1);
        if total_lines > MAX_OUTPUT_LINES {
            return Err(error::ApiRequestSnafu {
                message: format!("Kimi subprocess output exceeds {MAX_OUTPUT_LINES} line limit"),
            }
            .build());
        }
        if total_bytes > MAX_OUTPUT_BYTES {
            return Err(error::ApiRequestSnafu {
                message: format!(
                    "Kimi subprocess output exceeds {MAX_OUTPUT_BYTES} byte limit (collected {total_bytes} bytes)"
                ),
            }
            .build());
        }

        let trimmed = line.trim();
        if trimmed.starts_with("TextPart(") {
            in_text_part = true;
        }

        let text = parse::parse_text_part_line(&line).or_else(|| {
            if in_text_part {
                parse::parse_text_assignment_line(&line)
            } else {
                None
            }
        });
        if let Some(text) = text
            && !text.is_empty()
        {
            on_delta(&text);
            stream_deltas.push(text);
        }

        if trimmed == ")" && in_text_part {
            in_text_part = false;
        }

        if let Some(id) = parse::parse_message_id_line(&line) {
            message_id = Some(id);
        }

        let before = usage;
        parse::parse_usage_assignment(&line, &mut usage);
        if usage != before {
            has_usage = true;
        }
    }

    if stream_deltas.is_empty() {
        return Err(error::ApiRequestSnafu {
            message: "Kimi subprocess produced no text output".to_owned(),
        }
        .build());
    }

    let result_text = stream_deltas.join("");

    debug!(
        result_len = result_text.len(),
        deltas = stream_deltas.len(),
        "Kimi subprocess completed"
    );

    Ok(KimiOutput {
        result_text,
        usage: has_usage.then_some(usage),
        message_id,
        stream_deltas,
    })
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod process_tests;
