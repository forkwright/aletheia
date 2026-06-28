//! Codex subprocess management.
//!
//! Spawns `codex exec --dangerously-bypass-approvals-and-sandbox
//! --skip-git-repo-check --color never -` and manages stdin feeding,
//! bounded output collection, timeout, and cleanup.

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::error::{self, Result};

/// Maximum total bytes of collected stdout before aborting.
///
/// WHY: Prevent a runaway subprocess from growing memory without bound.
pub(crate) const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum total number of stdout lines before aborting.
///
/// WHY: Secondary guard for output with many small lines.
pub(crate) const MAX_OUTPUT_LINES: usize = 100_000;

/// Maximum length of a system prompt passed to the Codex subprocess.
///
/// WHY: The system prompt is folded into stdin for Codex. Keep parity with
/// the CC adapter's prompt envelope and avoid excessive subprocess input.
pub(crate) const MAX_SYSTEM_PROMPT_BYTES: usize = 100 * 1024;

/// Outcome of a Codex subprocess invocation.
#[derive(Debug)]
pub(crate) struct CodexOutput {
    /// Buffered stdout text.
    pub stdout: String,
}

fn scrub_codex_auth_env(cmd: &mut Command) {
    // WHY: Force Codex CLI to use its local OAuth credential store. Inherited
    // API keys can switch the CLI onto the API-key auth path.
    cmd.env_remove("OPENAI_API_KEY");
}

fn compose_stdin(system_prompt: Option<&str>, prompt: &str) -> Result<String> {
    if let Some(system) = system_prompt {
        if system.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err(error::ApiRequestSnafu {
                message: format!(
                    "system prompt exceeds maximum size ({} bytes > {MAX_SYSTEM_PROMPT_BYTES} byte limit)",
                    system.len(),
                ),
            }
            .build());
        }
        return Ok(format!("System:\n{system}\n\nUser:\n{prompt}"));
    }

    Ok(prompt.to_owned())
}

/// Spawn Codex and run a completion, collecting bounded stdout.
///
/// # Errors
///
/// Returns errors on spawn failure, stdin write failure, timeout, output
/// limits, invalid UTF-8 output, or nonzero subprocess exit.
#[tracing::instrument(skip_all)]
pub(crate) async fn run_completion(
    codex_binary: &PathBuf,
    working_directory: Option<&Path>,
    system_prompt: Option<&str>,
    prompt: &str,
    timeout: Duration,
) -> Result<CodexOutput> {
    let stdin_payload = compose_stdin(system_prompt, prompt)?;

    let mut cmd = Command::new(codex_binary);
    cmd.arg("exec")
        .arg("--dangerously-bypass-approvals-and-sandbox")
        .arg("--skip-git-repo-check")
        .arg("--color")
        .arg("never")
        .arg("--json")
        .arg("-")
        // WHY(#4884): kill_on_drop ensures the subprocess is terminated if the
        // future is dropped (timeout, actor cancellation) rather than becoming
        // an orphan outside Aletheia's process lifecycle.
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    scrub_codex_auth_env(&mut cmd);

    if let Some(cwd) = working_directory {
        cmd.current_dir(cwd);
    }

    debug!(
        binary = %codex_binary.display(),
        cwd = ?working_directory.map(|path| path.display().to_string()),
        "spawning Codex subprocess"
    );

    let mut child = cmd.spawn().map_err(|e| {
        error::SubprocessFailureSnafu {
            provider: "codex".to_owned(),
            kind: error::SubprocessFailureKind::Spawn,
            message: format!(
                "failed to spawn codex CLI at {}: {e}",
                codex_binary.display()
            ),
        }
        .build()
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_payload.as_bytes())
            .await
            .map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!("failed to write to Codex stdin: {e}"),
                }
                .build()
            })?;
        drop(stdin);
    }

    let stdout = child.stdout.take().ok_or_else(|| {
        error::ApiRequestSnafu {
            message: "Codex subprocess stdout not captured".to_owned(),
        }
        .build()
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        error::ApiRequestSnafu {
            message: "Codex subprocess stderr not captured".to_owned(),
        }
        .build()
    })?;

    let result = Box::pin(tokio::time::timeout(timeout, async {
        let (stdout_result, stderr_result, status_result) = tokio::join!(
            read_bounded(stdout, "stdout"),
            read_bounded(stderr, "stderr"),
            child.wait()
        );

        let stdout = stdout_result?;
        let stderr = stderr_result?;
        let status = status_result.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to wait for Codex process: {e}"),
            }
            .build()
        })?;

        finish_output(status, stdout, &stderr)
    }))
    .await;

    match result {
        Ok(output) => output,
        Err(_elapsed) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "Codex subprocess timed out, killing"
            );
            let _ = child.kill().await; // kanon:ignore RUST/no-silent-result-swallow WHY: best-effort kill on timeout path; already returning an Err
            Err(error::SubprocessFailureSnafu {
                provider: "codex".to_owned(),
                kind: error::SubprocessFailureKind::Timeout,
                message: format!("timed out after {}s", timeout.as_secs()),
            }
            .build())
        }
    }
}

fn finish_output(status: ExitStatus, stdout: Vec<u8>, stderr: &[u8]) -> Result<CodexOutput> {
    let stdout_text = String::from_utf8(stdout).map_err(|e| {
        error::ApiRequestSnafu {
            message: format!("Codex stdout was not valid UTF-8: {e}"),
        }
        .build()
    })?;
    let stderr_text = String::from_utf8_lossy(stderr);

    if !status.success() {
        return Err(error::SubprocessFailureSnafu {
            provider: "codex".to_owned(),
            kind: error::SubprocessFailureKind::Exit,
            message: format!(
                "process exited with {status}: {}",
                if stderr_text.trim().is_empty() {
                    "(no stderr)"
                } else {
                    stderr_text.trim()
                }
            ),
        }
        .build());
    }

    debug!(stdout_len = stdout_text.len(), "Codex subprocess completed");

    Ok(CodexOutput {
        stdout: stdout_text,
    })
}

async fn read_bounded<R>(mut reader: R, stream_name: &str) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut out = Vec::new();
    let mut buf = [0_u8; 8192];
    let mut lines = 0_usize;

    loop {
        let read = reader.read(&mut buf).await.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to read Codex {stream_name}: {e}"),
            }
            .build()
        })?;
        if read == 0 {
            break;
        }

        let new_len = out.len().saturating_add(read);
        if new_len > MAX_OUTPUT_BYTES {
            return Err(error::ApiRequestSnafu {
                message: format!(
                    "Codex subprocess {stream_name} exceeds {MAX_OUTPUT_BYTES} byte limit (collected {new_len} bytes)"
                ),
            }
            .build());
        }

        let chunk = buf.get(..read).ok_or_else(|| {
            error::ApiRequestSnafu {
                message: format!("failed to read Codex {stream_name}: read beyond buffer"),
            }
            .build()
        })?;
        for byte in chunk {
            if *byte == b'\n' {
                lines = lines.saturating_add(1);
            }
        }
        if lines > MAX_OUTPUT_LINES {
            return Err(error::ApiRequestSnafu {
                message: format!(
                    "Codex subprocess {stream_name} exceeds {MAX_OUTPUT_LINES} line limit"
                ),
            }
            .build());
        }

        out.extend_from_slice(chunk);
    }

    Ok(out)
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod process_tests;
