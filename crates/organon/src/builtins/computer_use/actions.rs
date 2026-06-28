//! Action dispatch via xdotool.

use std::path::Path;

use super::process::{display_request, run_display_command};
use super::types::ComputerAction;
use crate::sandbox::SandboxPolicy;
use crate::subprocess::{SubprocessOutput, SubprocessRequest, SubprocessRunner};
use crate::types::ToolContext;

/// Captured output from one logical computer action.
pub(super) struct ActionCommandResult {
    pub(super) outputs: Vec<SubprocessOutput>,
}

/// Build the xdotool subprocess requests needed for a computer action.
pub(super) fn action_requests(
    action: &ComputerAction,
    current_dir: &Path,
    policy: &SandboxPolicy,
) -> Vec<SubprocessRequest> {
    match action {
        ComputerAction::Click { x, y, button } => {
            vec![display_request("xdotool", current_dir, policy).args([
                "mousemove".to_owned(),
                "--sync".to_owned(),
                x.to_string(),
                y.to_string(),
                "click".to_owned(),
                button.to_string(),
            ])]
        }
        ComputerAction::TypeText { text } => {
            // WHY: --clearmodifiers prevents modifier keys held by the user
            // from interfering with the typed text.
            vec![display_request("xdotool", current_dir, policy).args([
                "type".to_owned(),
                "--clearmodifiers".to_owned(),
                "--".to_owned(),
                text.clone(),
            ])]
        }
        ComputerAction::Key { combo } => {
            vec![display_request("xdotool", current_dir, policy).args([
                "key".to_owned(),
                "--clearmodifiers".to_owned(),
                combo.clone(),
            ])]
        }
        ComputerAction::Scroll { x, y, delta } => {
            // WHY: xdotool click 4 = scroll up, click 5 = scroll down.
            // Repeat the click for the absolute value of delta.
            let button = if *delta > 0 { "5" } else { "4" };
            let count = delta.unsigned_abs();
            vec![
                display_request("xdotool", current_dir, policy).args([
                    "mousemove".to_owned(),
                    "--sync".to_owned(),
                    x.to_string(),
                    y.to_string(),
                ]),
                display_request("xdotool", current_dir, policy).args([
                    "click".to_owned(),
                    "--repeat".to_owned(),
                    count.to_string(),
                    button.to_owned(),
                ]),
            ]
        }
    }
}

/// Execute a computer action via xdotool.
///
/// # Errors
///
/// Returns `Err` if xdotool is not installed or the command fails.
pub(super) fn dispatch_action(
    action: &ComputerAction,
    runner: &SubprocessRunner,
    ctx: &ToolContext,
    current_dir: &Path,
    policy: &SandboxPolicy,
) -> std::io::Result<ActionCommandResult> {
    let mut outputs = Vec::new();
    for request in action_requests(action, current_dir, policy) {
        let output = run_display_command(runner, ctx, request, "xdotool")?;
        outputs.push(output);
    }

    Ok(ActionCommandResult { outputs })
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
    fn action_requests_use_display_runner_policy() {
        let action = ComputerAction::Click {
            x: 12,
            y: 34,
            button: 1,
        };
        let policy = test_policy();
        let requests = action_requests(&action, Path::new("/tmp"), &policy);

        let [request] = requests.as_slice() else {
            panic!("expected one xdotool request");
        };
        assert_eq!(
            request.program_for_test(),
            &std::ffi::OsString::from("xdotool")
        );
        assert!(
            request.allowed_env_vars_for_test().contains(&"DISPLAY"),
            "action request should preserve display env"
        );
        assert!(
            request.allowed_env_vars_for_test().contains(&"XAUTHORITY"),
            "action request should preserve display auth env"
        );
        assert!(
            request.explicit_sandbox_policy_for_test().is_some(),
            "action request should use explicit computer-use sandbox policy"
        );
    }

    #[test]
    fn scroll_builds_move_then_click_requests() {
        let action = ComputerAction::Scroll {
            x: 10,
            y: 20,
            delta: -3,
        };
        let policy = test_policy();
        let requests = action_requests(&action, Path::new("/tmp"), &policy);

        let [move_request, click_request] = requests.as_slice() else {
            panic!("expected move and click xdotool requests");
        };
        assert_eq!(
            move_request.args_for_test(),
            &[
                std::ffi::OsString::from("mousemove"),
                std::ffi::OsString::from("--sync"),
                std::ffi::OsString::from("10"),
                std::ffi::OsString::from("20"),
            ]
        );
        assert_eq!(
            click_request.args_for_test(),
            &[
                std::ffi::OsString::from("click"),
                std::ffi::OsString::from("--repeat"),
                std::ffi::OsString::from("3"),
                std::ffi::OsString::from("4"),
            ]
        );
    }
}
