//! Shared subprocess helpers for computer-use capture and action commands.

use std::path::Path;
use std::time::Duration;

use koina::defaults::MAX_OUTPUT_BYTES;

use crate::sandbox::SandboxPolicy;
use crate::subprocess::{SubprocessOutput, SubprocessRequest, SubprocessRunner};
use crate::types::{ToolContext, ToolDiagnostics};

const DISPLAY_ENV_VARS: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "ICEAUTHORITY",
    "XDG_RUNTIME_DIR",
];
const COMPUTER_USE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub(super) struct CommandDiagnostics {
    exit_code: Option<i32>,
    stderr: Vec<String>,
    duration_ms: u64,
}

impl CommandDiagnostics {
    pub(super) fn record(&mut self, output: &SubprocessOutput) {
        self.exit_code = Some(output.exit_code);
        let stderr = output.stderr.trim();
        if !stderr.is_empty() {
            self.stderr.push(stderr.to_owned());
        }
        self.duration_ms = self
            .duration_ms
            .saturating_add(u64::try_from(output.duration.as_millis()).unwrap_or(u64::MAX));
    }

    pub(super) fn into_tool_diagnostics(self) -> ToolDiagnostics {
        ToolDiagnostics {
            exit_code: self.exit_code,
            stderr: if self.stderr.is_empty() {
                None
            } else {
                Some(self.stderr.join("\n"))
            },
            sandbox_violations: Vec::new(),
            duration_ms: self.duration_ms,
        }
    }
}

pub(super) fn display_request(
    program: &'static str,
    current_dir: &Path,
    policy: &SandboxPolicy,
) -> SubprocessRequest {
    SubprocessRequest::new(program, current_dir.to_path_buf())
        .allow_env_vars(DISPLAY_ENV_VARS.iter().copied())
        .timeout(COMPUTER_USE_TIMEOUT)
        .max_output_bytes(MAX_OUTPUT_BYTES)
        .sandbox_policy(policy.clone())
}

pub(super) fn run_display_command(
    runner: &SubprocessRunner,
    ctx: &ToolContext,
    request: SubprocessRequest,
    label: &str,
) -> std::io::Result<SubprocessOutput> {
    let output = runner
        .run(request, ctx)
        .map_err(|e| std::io::Error::other(format!("{label} subprocess failed: {e}")))?;

    if output.exit_code != 0 {
        let stderr = output.stderr.trim();
        let detail = if stderr.is_empty() {
            "no stderr".to_owned()
        } else {
            stderr.to_owned()
        };
        return Err(std::io::Error::other(format!(
            "{label} exited with {}: {detail}",
            output.exit_code
        )));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::sandbox::{EgressPolicy, SandboxEnforcement};

    use super::*;

    fn test_policy() -> SandboxPolicy {
        SandboxPolicy {
            enabled: true,
            read_paths: vec![PathBuf::from("/usr")],
            write_paths: vec![PathBuf::from("/tmp")],
            exec_paths: vec![PathBuf::from("/usr/bin")],
            enforcement: SandboxEnforcement::Permissive,
            egress: EgressPolicy::Deny,
            egress_allowlist: Vec::new(),
        }
    }

    #[test]
    fn display_request_attaches_display_env_and_explicit_policy() {
        let policy = test_policy();
        let request = display_request("xdotool", Path::new("/tmp"), &policy);

        for var in DISPLAY_ENV_VARS {
            assert!(
                request.allowed_env_vars_for_test().contains(var),
                "display request should preserve {var}"
            );
        }
        assert!(
            request.explicit_sandbox_policy_for_test().is_some(),
            "display request should use the computer-use sandbox policy"
        );
        assert_eq!(request.timeout_for_test(), COMPUTER_USE_TIMEOUT);
        assert_eq!(request.max_output_bytes_for_test(), MAX_OUTPUT_BYTES);
    }
}
