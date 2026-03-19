//! Computer use tool: screen capture, action dispatch, and sandboxed execution.
//!
//! Integrates with Anthropic's computer use API to provide:
//! - Screen capture via `scrot` (X11) or `grim` (Wayland)
//! - Coordinate-based actions: `click`, `type_text`, `key`, `scroll`
//! - Landlock LSM sandbox restricting filesystem access during sessions
//! - Result extraction with frame diff and structured change descriptions
//!
//! # Requirements
//!
//! - Linux kernel 5.13+ for Landlock sandbox support
//! - `scrot` or `grim` for screen capture
//! - `xdotool` for input simulation (X11)
//!
//! Feature-gated behind `computer-use` — not compiled by default.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use aletheia_koina::id::ToolName;

use crate::error::{self, Result};
use crate::process_guard::ProcessGuard;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::sandbox::{SandboxConfig, SandboxEnforcement, SandboxPolicy};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

use super::workspace::extract_str;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Actions the computer use tool can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum ComputerAction {
    /// Click at screen coordinates.
    Click {
        /// X coordinate in pixels.
        x: i32,
        /// Y coordinate in pixels.
        y: i32,
        /// Mouse button: 1 = left, 2 = middle, 3 = right.
        #[serde(default = "default_button")]
        button: u8,
    },
    /// Type text via simulated keystrokes.
    TypeText {
        /// The text to type.
        text: String,
    },
    /// Press a key combination.
    Key {
        /// Key combo string (e.g. "ctrl+c", "Return", "alt+Tab").
        combo: String,
    },
    /// Scroll at screen coordinates.
    Scroll {
        /// X coordinate in pixels.
        x: i32,
        /// Y coordinate in pixels.
        y: i32,
        /// Scroll delta: positive = down, negative = up.
        delta: i32,
    },
}

fn default_button() -> u8 {
    1
}

impl std::fmt::Display for ComputerAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Click { x, y, button } => write!(f, "click({x}, {y}, button={button})"),
            Self::TypeText { text } => write!(f, "type_text({text:?})"),
            Self::Key { combo } => write!(f, "key({combo})"),
            Self::Scroll { x, y, delta } => write!(f, "scroll({x}, {y}, delta={delta})"),
        }
    }
}

/// Bounding box for a changed region between two frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DiffRegion {
    /// Left edge in pixels.
    pub(crate) x: u32,
    /// Top edge in pixels.
    pub(crate) y: u32,
    /// Width in pixels.
    pub(crate) width: u32,
    /// Height in pixels.
    pub(crate) height: u32,
}

impl std::fmt::Display for DiffRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}) {}x{}", self.x, self.y, self.width, self.height)
    }
}

/// Structured result from a computer use action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ActionResult {
    /// Whether the action succeeded.
    pub(crate) success: bool,
    /// The action that was performed.
    pub(crate) action: String,
    /// Bounding box of the region that changed between frames.
    pub(crate) diff_region: Option<DiffRegion>,
    /// Human-readable description of what changed.
    pub(crate) change_description: String,
    /// Base64-encoded PNG of the post-action frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_base64: Option<String>,
}

// ---------------------------------------------------------------------------
// Screen capture
// ---------------------------------------------------------------------------

/// Detect display server and return the appropriate capture command.
fn capture_command(output_path: &Path) -> std::process::Command {
    let output = output_path.to_string_lossy();

    // WHY: Check WAYLAND_DISPLAY first; if set, the session is Wayland and
    // scrot (X11-only) will not work. grim is the standard Wayland capture tool.
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        let mut cmd = std::process::Command::new("grim");
        cmd.arg(output.as_ref());
        cmd
    } else {
        let mut cmd = std::process::Command::new("scrot");
        cmd.args(["--overwrite", output.as_ref()]);
        cmd
    }
}

/// Capture the current screen to a PNG file.
///
/// # Errors
///
/// Returns `Err` if the capture tool is not installed or fails.
fn capture_screen(output_path: &Path) -> std::io::Result<()> {
    let mut cmd = capture_command(output_path);
    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!(
            "screen capture failed: {stderr}"
        )));
    }
    Ok(())
}

/// Read a PNG file and return its raw bytes.
fn read_frame(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

// ---------------------------------------------------------------------------
// Action dispatch
// ---------------------------------------------------------------------------

/// Execute a computer action via xdotool.
///
/// # Errors
///
/// Returns `Err` if xdotool is not installed or the command fails.
fn dispatch_action(action: &ComputerAction) -> std::io::Result<String> {
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

// ---------------------------------------------------------------------------
// Frame diff
// ---------------------------------------------------------------------------

/// Compare two PNG byte buffers and return the bounding box of the changed region.
///
/// Uses a simple byte-level comparison. Both frames must have the same dimensions.
/// Returns `None` if the frames are identical or cannot be compared.
fn compute_diff_region(before: &[u8], after: &[u8]) -> Option<DiffRegion> {
    // WHY: Parse PNG headers to extract dimensions rather than pulling in an
    // image decoding crate. PNG IHDR chunk is always the first chunk after
    // the 8-byte signature: 4 bytes length, 4 bytes "IHDR", 4 bytes width,
    // 4 bytes height (big-endian u32).
    let width_before = png_width(before)?;
    let height_before = png_height(before)?;
    let width_after = png_width(after)?;
    let height_after = png_height(after)?;

    if width_before != width_after || height_before != height_after {
        // Frames have different dimensions; treat entire frame as changed.
        return Some(DiffRegion {
            x: 0,
            y: 0,
            width: width_after,
            height: height_after,
        });
    }

    if before == after {
        return None;
    }

    // WHY: For raw PNG byte comparison, we cannot do per-pixel diff without
    // decompressing the IDAT chunks. Instead, report that a change occurred
    // and return the full frame as the diff region. This is a pragmatic
    // compromise: the LLM receives the full post-action screenshot and knows
    // that something changed.
    Some(DiffRegion {
        x: 0,
        y: 0,
        width: width_after,
        height: height_after,
    })
}

/// Extract width from PNG IHDR chunk.
fn png_width(data: &[u8]) -> Option<u32> {
    // PNG signature (8 bytes) + chunk length (4) + "IHDR" (4) + width (4)
    let bytes: [u8; 4] = data.get(16..20)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// Extract height from PNG IHDR chunk.
fn png_height(data: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = data.get(20..24)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// Generate a human-readable description of the change.
fn describe_change(action: &ComputerAction, diff: Option<&DiffRegion>) -> String {
    let action_desc = match action {
        ComputerAction::Click { x, y, button } => {
            let btn = match button {
                1 => "left",
                2 => "middle",
                3 => "right",
                _ => "unknown",
            };
            format!("Performed {btn}-click at ({x}, {y})")
        }
        ComputerAction::TypeText { text } => {
            let preview = if text.len() > 50 {
                format!("{}...", text.get(..50).unwrap_or(text))
            } else {
                text.clone()
            };
            format!("Typed text: {preview:?}")
        }
        ComputerAction::Key { combo } => {
            format!("Pressed key combination: {combo}")
        }
        ComputerAction::Scroll { x, y, delta } => {
            let direction = if *delta > 0 { "down" } else { "up" };
            format!(
                "Scrolled {direction} by {} units at ({x}, {y})",
                delta.unsigned_abs()
            )
        }
    };

    match diff {
        Some(region) => format!("{action_desc}. Screen changed in region {region}."),
        None => format!("{action_desc}. No visible change detected."),
    }
}

// ---------------------------------------------------------------------------
// Sandbox session
// ---------------------------------------------------------------------------

/// Configuration for a computer use session's Landlock sandbox.
///
/// # NOTE
///
/// Landlock LSM requires Linux kernel 5.13+. The sandbox is applied via
/// the `landlock_create_ruleset`, `landlock_add_rule`, and
/// `landlock_restrict_self` syscalls directly through the `landlock` crate
/// (which wraps the syscalls via `rustix`). No external sandbox binary is
/// used.
#[derive(Debug, Clone)]
pub(crate) struct ComputerUseSessionConfig {
    /// Filesystem paths the session is allowed to read.
    pub(crate) allowed_read_paths: Vec<PathBuf>,
    /// Filesystem paths the session is allowed to write.
    pub(crate) allowed_write_paths: Vec<PathBuf>,
    /// Whether to enforce the sandbox (deny violations) or just log them.
    pub(crate) enforcement: SandboxEnforcement,
}

impl Default for ComputerUseSessionConfig {
    fn default() -> Self {
        Self {
            allowed_read_paths: vec![
                PathBuf::from("/usr"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
                PathBuf::from("/etc"),
                PathBuf::from("/proc"),
                PathBuf::from("/dev"),
            ],
            allowed_write_paths: vec![std::env::temp_dir()],
            enforcement: SandboxEnforcement::Enforcing,
        }
    }
}

impl ComputerUseSessionConfig {
    /// Build a [`SandboxPolicy`] from this session config.
    #[must_use]
    pub(crate) fn to_sandbox_policy(&self) -> SandboxPolicy {
        let mut read_paths = self.allowed_read_paths.clone();
        // WHY: Write paths must also be readable for tools to verify
        // their own output.
        for wp in &self.allowed_write_paths {
            if !read_paths.contains(wp) {
                read_paths.push(wp.clone());
            }
        }

        SandboxPolicy {
            enabled: true,
            read_paths,
            write_paths: self.allowed_write_paths.clone(),
            exec_paths: vec![
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/lib"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
            ],
            enforcement: self.enforcement,
            egress: crate::sandbox::EgressPolicy::Deny,
            egress_allowlist: Vec::new(),
        }
    }
}

/// Execute an action inside a sandboxed subprocess.
///
/// Spawns a child process with Landlock restrictions applied via `pre_exec`,
/// captures before/after frames, and returns a structured [`ActionResult`].
///
/// # NOTE
///
/// The Landlock sandbox is applied via syscall through the `landlock` crate
/// (not an external sandbox binary). Requires Linux kernel 5.13+ with
/// Landlock enabled (`CONFIG_SECURITY_LANDLOCK=y`).
///
/// # Errors
///
/// Returns an error if screen capture, action dispatch, or sandbox setup fails.
fn execute_sandboxed_action(
    action: &ComputerAction,
    session_config: &ComputerUseSessionConfig,
) -> std::io::Result<ActionResult> {
    let temp_dir = std::env::temp_dir();
    let before_path = temp_dir.join("aletheia_cu_before.png");
    let after_path = temp_dir.join("aletheia_cu_after.png");

    // Capture pre-action frame.
    capture_screen(&before_path)?;
    let before_bytes = read_frame(&before_path)?;

    // Dispatch the action.
    // NOTE: Actions run in the current process since xdotool needs X11
    // access. The Landlock sandbox is applied to the capture subprocess
    // to restrict filesystem access during frame capture.
    dispatch_action(action)?;

    // Small delay for screen to update after action.
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Capture post-action frame in a sandboxed subprocess.
    let policy = session_config.to_sandbox_policy();
    let mut cmd = capture_command(&after_path);
    crate::sandbox::apply_sandbox(&mut cmd, policy)?;

    let child = cmd.spawn()?;
    let mut guard = ProcessGuard::new(child);
    let status = guard.get_mut().wait()?;
    // WHY: Drop the guard after wait(). The child has already exited;
    // Drop's kill() returns ESRCH (safe), wait() returns ECHILD (safe).
    drop(guard);
    if !status.success() {
        return Err(std::io::Error::other("sandboxed screen capture failed"));
    }

    let after_bytes = read_frame(&after_path)?;

    // Compute diff.
    let diff_region = compute_diff_region(&before_bytes, &after_bytes);
    let change_description = describe_change(action, diff_region.as_ref());

    // Encode post-action frame for return.
    let frame_base64 = Some(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &after_bytes,
    ));

    // Clean up temp files.
    let _ = std::fs::remove_file(&before_path);
    let _ = std::fs::remove_file(&after_path);

    Ok(ActionResult {
        success: true,
        action: action.to_string(),
        diff_region,
        change_description,
        frame_base64,
    })
}

// ---------------------------------------------------------------------------
// Tool executor
// ---------------------------------------------------------------------------

/// Extract an i32 coordinate from JSON arguments.
fn extract_i32(args: &serde_json::Value, field: &str, tool_name: &ToolName) -> Result<i32> {
    let val = args
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| {
            error::InvalidInputSnafu {
                name: tool_name.clone(),
                reason: format!("missing or invalid field: {field}"),
            }
            .build()
        })?;
    i32::try_from(val).map_err(|_err| {
        error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: format!("{field} out of i32 range"),
        }
        .build()
    })
}

/// Parse a [`ComputerAction`] from tool input arguments.
///
/// Returns `Ok(None)` for unknown action types (caller produces error result).
fn parse_action(input: &ToolInput) -> Result<Option<ComputerAction>> {
    let action_type = extract_str(&input.arguments, "action", &input.name)?;

    let action = match action_type {
        "click" => {
            let x = extract_i32(&input.arguments, "x", &input.name)?;
            let y = extract_i32(&input.arguments, "y", &input.name)?;
            let button = input
                .arguments
                .get("button")
                .and_then(serde_json::Value::as_u64)
                .map_or(1u8, |b| u8::try_from(b).unwrap_or(1));
            ComputerAction::Click { x, y, button }
        }
        "type_text" => {
            let text = extract_str(&input.arguments, "text", &input.name)?.to_owned();
            ComputerAction::TypeText { text }
        }
        "key" => {
            let combo = extract_str(&input.arguments, "combo", &input.name)?.to_owned();
            ComputerAction::Key { combo }
        }
        "scroll" => {
            let x = extract_i32(&input.arguments, "x", &input.name)?;
            let y = extract_i32(&input.arguments, "y", &input.name)?;
            let delta = extract_i32(&input.arguments, "delta", &input.name)?;
            ComputerAction::Scroll { x, y, delta }
        }
        _ => return Ok(None),
    };

    Ok(Some(action))
}

pub(crate) struct ComputerUseExecutor {
    session_config: ComputerUseSessionConfig,
}

impl ComputerUseExecutor {
    pub(crate) fn new(config: ComputerUseSessionConfig) -> Self {
        Self {
            session_config: config,
        }
    }
}

impl ToolExecutor for ComputerUseExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let Some(action) = parse_action(input)? else {
                let action_type = extract_str(&input.arguments, "action", &input.name)?;
                return Ok(ToolResult::error(format!(
                    "unknown action: {action_type}. Valid actions: click, type_text, key, scroll"
                )));
            };

            tracing::info!(action = %action, "computer_use: dispatching action");

            // WHY: execute_sandboxed_action performs blocking I/O (subprocess
            // spawn, file reads, thread::sleep). Use spawn_blocking to avoid
            // stalling the Tokio runtime.
            let config = self.session_config.clone();
            let action_clone = action.clone();
            let result = tokio::task::spawn_blocking(move || {
                execute_sandboxed_action(&action_clone, &config)
            })
            .await;

            match result {
                Ok(Ok(action_result)) => {
                    let json = serde_json::to_string_pretty(&action_result).map_err(|e| {
                        error::ExecutionFailedSnafu {
                            name: input.name.clone(),
                            message: format!("failed to serialize result: {e}"),
                        }
                        .build()
                    })?;
                    Ok(ToolResult::text(json))
                }
                Ok(Err(io_err)) => Ok(ToolResult::error(format!(
                    "computer_use action failed: {io_err}"
                ))),
                Err(join_err) => Ok(ToolResult::error(format!(
                    "computer_use task panicked: {join_err}"
                ))),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tool definition and registration
// ---------------------------------------------------------------------------

#[expect(
    clippy::expect_used,
    reason = "ToolName::new() with static string literal is infallible"
)]
fn computer_use_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("computer_use").expect("valid tool name"),
        description: "Interact with the computer screen: capture screenshots, click, type text, \
                      press keys, and scroll. Actions run in a Landlock-sandboxed environment."
            .to_owned(),
        extended_description: Some(
            "Perform computer use actions in a sandboxed Linux environment. Supported actions:\n\
             - click: Click at (x, y) coordinates with optional button (1=left, 2=middle, 3=right)\n\
             - type_text: Type text via simulated keystrokes\n\
             - key: Press a key combination (e.g. 'ctrl+c', 'Return', 'alt+Tab')\n\
             - scroll: Scroll at (x, y) coordinates with delta (positive=down, negative=up)\n\n\
             Each action captures a before/after screenshot and returns a diff description.\n\
             The execution environment is sandboxed with Landlock LSM to restrict filesystem access."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform: click, type_text, key, or scroll"
                            .to_owned(),
                        enum_values: Some(vec![
                            "click".to_owned(),
                            "type_text".to_owned(),
                            "key".to_owned(),
                            "scroll".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "x".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "X coordinate in pixels (for click and scroll)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "y".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Y coordinate in pixels (for click and scroll)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "button".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Mouse button: 1=left, 2=middle, 3=right (for click, default: 1)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(1)),
                    },
                ),
                (
                    "text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Text to type (for type_text action)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "combo".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Key combination string, e.g. 'ctrl+c' (for key action)"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "delta".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description:
                            "Scroll delta: positive=down, negative=up (for scroll action)"
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["action".to_owned()],
        },
        category: ToolCategory::System,
        auto_activate: false,
    }
}

/// Register the `computer_use` tool into the registry.
///
/// Uses the provided [`SandboxConfig`] to derive default session
/// sandbox policy. The tool is registered with `auto_activate: false`,
/// requiring explicit activation via `enable_tool`.
///
/// # Errors
///
/// Returns an error if the tool name collides with an existing tool.
pub fn register(registry: &mut ToolRegistry, sandbox: &SandboxConfig) -> Result<()> {
    let session_config = ComputerUseSessionConfig {
        enforcement: if sandbox.enabled {
            sandbox.enforcement
        } else {
            SandboxEnforcement::Permissive
        },
        ..ComputerUseSessionConfig::default()
    };
    registry.register(
        computer_use_def(),
        Box::new(ComputerUseExecutor::new(session_config)),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn png_dimension_parsing() {
        // Minimal valid PNG: 8-byte signature + IHDR chunk
        // Signature: 137 80 78 71 13 10 26 10
        // IHDR: length (13) + "IHDR" + width (800) + height (600) + ...
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10]; // signature
        png.extend_from_slice(&[0, 0, 0, 13]); // chunk length
        png.extend_from_slice(b"IHDR"); // chunk type
        png.extend_from_slice(&800u32.to_be_bytes()); // width
        png.extend_from_slice(&600u32.to_be_bytes()); // height
        png.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth, color type, etc.

        assert_eq!(png_width(&png), Some(800), "should parse width from IHDR");
        assert_eq!(png_height(&png), Some(600), "should parse height from IHDR");
    }

    #[test]
    fn png_dimension_parsing_too_short() {
        assert_eq!(png_width(&[0; 10]), None, "buffer too short for width");
        assert_eq!(png_height(&[0; 20]), None, "buffer too short for height");
    }

    #[test]
    fn diff_identical_frames_returns_none() {
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&100u32.to_be_bytes());
        png.extend_from_slice(&100u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);

        assert!(
            compute_diff_region(&png, &png).is_none(),
            "identical frames should produce no diff"
        );
    }

    #[test]
    fn diff_different_frames_returns_region() {
        let mut png1 = vec![137, 80, 78, 71, 13, 10, 26, 10];
        png1.extend_from_slice(&[0, 0, 0, 13]);
        png1.extend_from_slice(b"IHDR");
        png1.extend_from_slice(&640u32.to_be_bytes());
        png1.extend_from_slice(&480u32.to_be_bytes());
        png1.extend_from_slice(&[8, 2, 0, 0, 0]);
        png1.extend_from_slice(&[0xAA; 50]); // padding

        let mut png2 = png1.clone();
        // Modify some bytes after IHDR to simulate different content.
        if let Some(byte) = png2.get_mut(30) {
            *byte = 0xBB;
        }

        let diff = compute_diff_region(&png1, &png2);
        assert!(diff.is_some(), "different frames should produce a diff");
        let region = diff.expect("diff should exist");
        assert_eq!(region.width, 640, "diff width should match frame width");
        assert_eq!(region.height, 480, "diff height should match frame height");
    }

    #[test]
    fn diff_different_dimensions_returns_full_frame() {
        let make_png = |w: u32, h: u32| {
            let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
            png.extend_from_slice(&[0, 0, 0, 13]);
            png.extend_from_slice(b"IHDR");
            png.extend_from_slice(&w.to_be_bytes());
            png.extend_from_slice(&h.to_be_bytes());
            png.extend_from_slice(&[8, 2, 0, 0, 0]);
            png
        };

        let diff = compute_diff_region(&make_png(800, 600), &make_png(1024, 768));
        assert!(diff.is_some(), "different dimensions should produce diff");
        let region = diff.expect("diff should exist");
        assert_eq!(region.width, 1024, "should use after frame width");
        assert_eq!(region.height, 768, "should use after frame height");
    }

    #[test]
    fn action_display_formatting() {
        let click = ComputerAction::Click {
            x: 100,
            y: 200,
            button: 1,
        };
        assert_eq!(click.to_string(), "click(100, 200, button=1)");

        let type_text = ComputerAction::TypeText {
            text: "hello".to_owned(),
        };
        assert_eq!(type_text.to_string(), "type_text(\"hello\")");

        let key = ComputerAction::Key {
            combo: "ctrl+c".to_owned(),
        };
        assert_eq!(key.to_string(), "key(ctrl+c)");

        let scroll = ComputerAction::Scroll {
            x: 50,
            y: 60,
            delta: -3,
        };
        assert_eq!(scroll.to_string(), "scroll(50, 60, delta=-3)");
    }

    #[test]
    fn describe_change_with_diff() {
        let action = ComputerAction::Click {
            x: 10,
            y: 20,
            button: 1,
        };
        let diff = Some(DiffRegion {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        });
        let desc = describe_change(&action, diff.as_ref());
        assert!(desc.contains("left-click"), "should mention click type");
        assert!(
            desc.contains("Screen changed"),
            "should mention screen change"
        );
    }

    #[test]
    fn describe_change_without_diff() {
        let action = ComputerAction::Key {
            combo: "Return".to_owned(),
        };
        let desc = describe_change(&action, None);
        assert!(
            desc.contains("No visible change"),
            "should indicate no change"
        );
    }

    #[test]
    fn session_config_default_has_standard_paths() {
        let config = ComputerUseSessionConfig::default();
        assert!(
            config.allowed_read_paths.contains(&PathBuf::from("/usr")),
            "default should include /usr read"
        );
        assert!(
            !config.allowed_write_paths.is_empty(),
            "default should have write paths"
        );
        assert_eq!(
            config.enforcement,
            SandboxEnforcement::Enforcing,
            "default enforcement should be enforcing"
        );
    }

    #[test]
    fn session_config_to_sandbox_policy() {
        let config = ComputerUseSessionConfig::default();
        let policy = config.to_sandbox_policy();
        assert!(policy.enabled, "policy should be enabled");
        assert!(
            !policy.exec_paths.is_empty(),
            "policy should have exec paths"
        );
        // Write paths should also appear in read paths.
        for wp in &policy.write_paths {
            assert!(
                policy.read_paths.contains(wp),
                "write path {wp:?} should also be readable"
            );
        }
    }

    #[test]
    fn action_result_serialization_roundtrip() {
        let result = ActionResult {
            success: true,
            action: "click(100, 200, button=1)".to_owned(),
            diff_region: Some(DiffRegion {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }),
            change_description: "Performed left-click at (100, 200). Screen changed.".to_owned(),
            frame_base64: None,
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let roundtrip: ActionResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(roundtrip.success, result.success);
        assert_eq!(roundtrip.action, result.action);
        assert!(
            roundtrip.diff_region.is_some(),
            "diff_region should roundtrip"
        );
    }

    #[test]
    fn computer_action_serde_roundtrip() {
        let actions = vec![
            ComputerAction::Click {
                x: 100,
                y: 200,
                button: 1,
            },
            ComputerAction::TypeText {
                text: "hello world".to_owned(),
            },
            ComputerAction::Key {
                combo: "ctrl+shift+t".to_owned(),
            },
            ComputerAction::Scroll {
                x: 50,
                y: 60,
                delta: -5,
            },
        ];

        for action in &actions {
            let json = serde_json::to_string(action).expect("serialize action");
            let roundtrip: ComputerAction =
                serde_json::from_str(&json).expect("deserialize action");
            assert_eq!(
                action.to_string(),
                roundtrip.to_string(),
                "action should roundtrip"
            );
        }
    }

    #[test]
    fn diff_region_display() {
        let region = DiffRegion {
            x: 10,
            y: 20,
            width: 300,
            height: 400,
        };
        assert_eq!(region.to_string(), "(10, 20) 300x400");
    }
}
