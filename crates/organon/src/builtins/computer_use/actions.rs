//! Action dispatch via xdotool.

use super::types::ComputerAction;

/// Execute a computer action via xdotool.
///
/// # Errors
///
/// Returns `Err` if xdotool is not installed or the command fails.
pub(super) fn dispatch_action(action: &ComputerAction) -> std::io::Result<String> {
    let output = match action {
        ComputerAction::Click { x, y, button } => {
            let mut cmd = std::process::Command::new("xdotool");
            cmd.args([
                "mousemove",
                "--sync",
                &x.to_string(),
                &y.to_string(),
                "click",
                &button.to_string(),
            ]);
            cmd.output()?
        }
        ComputerAction::TypeText { text } => {
            let mut cmd = std::process::Command::new("xdotool");
            // WHY: --clearmodifiers prevents modifier keys held by the user
            // from interfering with the typed text.
            cmd.args(["type", "--clearmodifiers", "--", text]);
            cmd.output()?
        }
        ComputerAction::Key { combo } => {
            let mut cmd = std::process::Command::new("xdotool");
            cmd.args(["key", "--clearmodifiers", combo]);
            cmd.output()?
        }
        ComputerAction::Scroll { x, y, delta } => {
            let mut cmd = std::process::Command::new("xdotool");
            cmd.args(["mousemove", "--sync", &x.to_string(), &y.to_string()]);
            let move_output = cmd.output()?;
            if !move_output.status.success() {
                return Err(std::io::Error::other("failed to move mouse for scroll"));
            }

            // WHY: xdotool click 4 = scroll up, click 5 = scroll down.
            // Repeat the click for the absolute value of delta.
            let button = if *delta > 0 { "5" } else { "4" };
            let count = delta.unsigned_abs();
            let mut scroll_cmd = std::process::Command::new("xdotool");
            scroll_cmd.args(["click", "--repeat", &count.to_string(), button]);
            scroll_cmd.output()?
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!(
            "xdotool command failed: {stderr}"
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
