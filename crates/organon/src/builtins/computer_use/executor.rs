//! Tool executor, definition, and registration for the computer use tool.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_koina::id::ToolName;

use crate::error::{self, Result};
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::sandbox::{SandboxConfig, SandboxEnforcement};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::sandbox::{ComputerUseSessionConfig, execute_sandboxed_action};
use super::types::ComputerAction;
use crate::builtins::workspace::extract_str;

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

#[expect(
    clippy::expect_used,
    reason = "ToolName::new() with static string literal is infallible"
)]
fn computer_use_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("computer_use"), // kanon:ignore RUST/expect
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
        reversibility: Reversibility::Irreversible,
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
    // kanon:ignore RUST/pub-visibility  // kanon:ignore RUST/missing-must-use
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
