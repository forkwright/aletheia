//! Bounded subprocess helpers for render and probe commands.

use std::ffi::OsStr;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;
use std::process::{Command, Output, Stdio}; // kanon:ignore RUST/no-direct-process-command — this module is the subprocess substrate for poiesis-doc
use std::time::Duration;

use wait_timeout::ChildExt as _;

pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) enum CommandOutputError {
    Spawn {
        source: std::io::Error,
    },
    TempFile {
        source: std::io::Error,
    },
    Wait {
        source: std::io::Error,
    },
    Timeout {
        timeout: Duration,
        kill_error: Option<String>,
        wait_error: Option<String>,
    },
}

#[derive(Debug)]
pub(crate) enum ProbeCommandError {
    Failed(String),
    TimedOut { timeout_secs: u64 },
}

/// Construct a subprocess command inside the poiesis-doc process boundary.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program) // kanon:ignore RUST/no-direct-process-command — this module is the subprocess substrate for poiesis-doc
}

pub(crate) fn real_version_source(path: &Path) -> Result<String, ProbeCommandError> {
    let mut cmd = command(path);
    cmd.arg("--version");
    let output = output_with_timeout(&mut cmd, PROBE_TIMEOUT).map_err(|err| match err {
        CommandOutputError::Timeout { timeout, .. } => ProbeCommandError::TimedOut {
            timeout_secs: timeout.as_secs(),
        },
        CommandOutputError::Spawn { source }
        | CommandOutputError::TempFile { source }
        | CommandOutputError::Wait { source } => ProbeCommandError::Failed(source.to_string()),
    })?;

    if !output.status.success() {
        return Err(ProbeCommandError::Failed(format!(
            "exit status {}",
            output.status
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn parse_pandoc_version(output: &str) -> Option<(u32, u32, u32)> {
    let first = output.lines().next()?;
    let version = first.strip_prefix("pandoc ")?.trim();
    let mut parts = version.split('.');

    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);

    Some((major, minor, patch))
}

pub(crate) fn output_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
) -> Result<Output, CommandOutputError> {
    let mut stdout =
        tempfile::tempfile().map_err(|source| CommandOutputError::TempFile { source })?;
    let mut stderr =
        tempfile::tempfile().map_err(|source| CommandOutputError::TempFile { source })?;

    cmd.stdout(Stdio::from(
        stdout
            .try_clone()
            .map_err(|source| CommandOutputError::TempFile { source })?,
    ))
    .stderr(Stdio::from(
        stderr
            .try_clone()
            .map_err(|source| CommandOutputError::TempFile { source })?,
    ));

    let mut child = cmd
        .spawn()
        .map_err(|source| CommandOutputError::Spawn { source })?;

    // WHY: wait_timeout blocks on the OS child-exit notification instead of
    // polling with try_wait/sleep, eliminating the busy-wait loop.
    let status = child
        .wait_timeout(timeout)
        .map_err(|source| CommandOutputError::Wait { source })?;

    if let Some(status) = status {
        let stdout = read_temp_output(&mut stdout)?;
        let stderr = read_temp_output(&mut stderr)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    } else {
        let kill_error = child.kill().err().map(|err| err.to_string());
        let wait_error = child.wait().err().map(|err| err.to_string());
        Err(CommandOutputError::Timeout {
            timeout,
            kill_error,
            wait_error,
        })
    }
}

fn read_temp_output(file: &mut std::fs::File) -> Result<Vec<u8>, CommandOutputError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| CommandOutputError::TempFile { source })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| CommandOutputError::TempFile { source })?;
    Ok(bytes)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn output_with_timeout_kills_slow_process() {
        let mut cmd = command("sh");
        cmd.arg("-c").arg("sleep 5");

        let err = output_with_timeout(&mut cmd, Duration::from_millis(20))
            .expect_err("sleep must time out");

        assert!(
            matches!(err, CommandOutputError::Timeout { .. }),
            "expected timeout, got {err:?}"
        );
    }
}
