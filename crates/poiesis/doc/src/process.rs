//! Bounded subprocess helpers for render and probe commands.

use std::io::{Read as _, Seek as _, SeekFrom};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    let started = Instant::now();

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|source| CommandOutputError::Wait { source })?
        {
            let stdout = read_temp_output(&mut stdout)?;
            let stderr = read_temp_output(&mut stderr)?;
            return Ok(Output {
                status,
                stdout,
                stderr,
            });
        }

        if started.elapsed() >= timeout {
            let kill_error = child.kill().err().map(|err| err.to_string());
            let wait_error = child.wait().err().map(|err| err.to_string());
            return Err(CommandOutputError::Timeout {
                timeout,
                kill_error,
                wait_error,
            });
        }

        std::thread::sleep(PROCESS_POLL_INTERVAL);
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
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 5");

        let err = output_with_timeout(&mut cmd, Duration::from_millis(20))
            .expect_err("sleep must time out");

        assert!(
            matches!(err, CommandOutputError::Timeout { .. }),
            "expected timeout, got {err:?}"
        );
    }
}
