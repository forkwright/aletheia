//! Workspace tool executors: read, write, edit, exec.

use std::future::Future;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::process_guard::ProcessGuard;

use aletheia_koina::id::ToolName;
use indexmap::IndexMap;

use crate::error::{self, Result};
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

const MAX_OUTPUT_BYTES: usize = 50 * 1024;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn validate_path(raw: &str, ctx: &ToolContext, tool_name: &ToolName) -> Result<PathBuf> {
    if raw.is_empty() {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: "path must not be empty".to_owned(),
        }
        .build());
    }

    let resolved = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        ctx.workspace.join(raw)
    };

    let normalized = normalize(&resolved);

    let allowed = ctx
        .allowed_roots
        .iter()
        .any(|root| normalized.starts_with(root));

    if !allowed {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: format!("path outside allowed roots: {raw}"),
        }
        .build());
    }

    Ok(normalized)
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

pub(crate) fn extract_str<'a>(
    args: &'a serde_json::Value,
    field: &str,
    tool_name: &ToolName,
) -> Result<&'a str> {
    args.get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            error::InvalidInputSnafu {
                name: tool_name.clone(),
                reason: format!("missing or invalid field: {field}"),
            }
            .build()
        })
}

pub(crate) fn extract_opt_u64(args: &serde_json::Value, field: &str) -> Option<u64> {
    args.get(field).and_then(serde_json::Value::as_u64)
}

pub(crate) fn extract_opt_bool(args: &serde_json::Value, field: &str) -> Option<bool> {
    args.get(field).and_then(serde_json::Value::as_bool)
}

fn err_result(msg: String) -> ToolResult {
    ToolResult::error(msg)
}

// ---------------------------------------------------------------------------
// Executors
// ---------------------------------------------------------------------------

struct ReadExecutor;

impl ToolExecutor for ReadExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let max_lines = extract_opt_u64(&input.arguments, "maxLines");
            let path = validate_path(path_str, ctx, &input.name)?;

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(err_result(format!("file not found: {}", path.display())));
                }
                Err(e) => {
                    return Ok(err_result(format!("read failed: {e}")));
                }
            };

            let output = match max_lines {
                Some(n) => {
                    let n = usize::try_from(n).unwrap_or(usize::MAX);
                    content.lines().take(n).collect::<Vec<_>>().join("\n")
                }
                None => content,
            };

            Ok(ToolResult::text(output))
        })
    }
}

struct WriteExecutor;

impl ToolExecutor for WriteExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let content = extract_str(&input.arguments, "content", &input.name)?;
            let append = extract_opt_bool(&input.arguments, "append").unwrap_or(false);
            let path = validate_path(path_str, ctx, &input.name)?;

            if let Some(parent) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Ok(err_result(format!("failed to create directories: {e}")));
                }
            }

            let write_result = if append {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(content.as_bytes())
                    })
            } else {
                std::fs::write(&path, content)
            };

            match write_result {
                Ok(()) => Ok(ToolResult::text(format!(
                    "wrote {} bytes to {}",
                    content.len(),
                    path.display()
                ))),
                Err(e) => Ok(err_result(format!("write failed: {e}"))),
            }
        })
    }
}

struct EditExecutor;

impl ToolExecutor for EditExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let old_text = extract_str(&input.arguments, "old_text", &input.name)?;
            let new_text = extract_str(&input.arguments, "new_text", &input.name)?;
            let path = validate_path(path_str, ctx, &input.name)?;

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(err_result(format!("file not found: {}", path.display())));
                }
                Err(e) => {
                    return Ok(err_result(format!("read failed: {e}")));
                }
            };

            let count = content.matches(old_text).count();
            if count == 0 {
                return Ok(err_result(format!(
                    "old_text not found in {}",
                    path.display()
                )));
            }
            if count > 1 {
                return Ok(err_result(format!(
                    "old_text found {count} times in {} \u{2014} must be unique",
                    path.display()
                )));
            }

            let new_content = content.replacen(old_text, new_text, 1);
            if let Err(e) = std::fs::write(&path, &new_content) {
                return Ok(err_result(format!("write failed: {e}")));
            }

            Ok(ToolResult::text(format!(
                "edited {}: replaced {} chars with {} chars",
                path.display(),
                old_text.len(),
                new_text.len()
            )))
        })
    }
}

struct ExecExecutor {
    sandbox: crate::sandbox::SandboxConfig,
}

impl ToolExecutor for ExecExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let command = extract_str(&input.arguments, "command", &input.name)?;
            let timeout_ms = extract_opt_u64(&input.arguments, "timeout").unwrap_or(30_000);

            let mut cmd = Command::new("sh");
            cmd.arg("-c")
                .arg(command)
                .current_dir(&ctx.workspace)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            if self.sandbox.enabled {
                let policy = self
                    .sandbox
                    .build_policy(&ctx.workspace, &ctx.allowed_roots);
                crate::sandbox::apply_sandbox(&mut cmd, policy);
            }

            // Wrap immediately so the child is killed on any early return
            // (timeout, wait error, or panic).
            let mut guard = match cmd.spawn() {
                Ok(c) => ProcessGuard::new(c),
                Err(e) => {
                    return Ok(err_result(format!("spawn failed: {e}")));
                }
            };

            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let status = loop {
                match guard.get_mut().try_wait() {
                    Ok(Some(s)) => break s,
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            // Dropping `guard` kills and reaps the child.
                            return Ok(err_result(format!(
                                "command timed out after {timeout_ms}ms"
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        return Ok(err_result(format!("wait failed: {e}")));
                    }
                }
            };

            // Process exited normally (`try_wait` already reaped the zombie).
            // Read captured stdio via the guard. The guard's Drop will call
            // kill() + wait() on the already-dead process, both of which
            // safely ignore ESRCH / ECHILD errors.
            let mut stdout = String::new();
            if let Some(ref mut pipe) = guard.get_mut().stdout {
                let _ = pipe.read_to_string(&mut stdout);
            }
            let mut stderr = String::new();
            if let Some(ref mut pipe) = guard.get_mut().stderr {
                let _ = pipe.read_to_string(&mut stderr);
            }

            let code = status.code().unwrap_or(-1);
            let mut output = format!("exit={code}\n{stdout}\n{stderr}");
            if output.len() > MAX_OUTPUT_BYTES {
                output.truncate(MAX_OUTPUT_BYTES);
                output.push_str("\n[output truncated]");
            }

            Ok(ToolResult::text(output))
        })
    }
}

// ---------------------------------------------------------------------------
// Tool definitions (schemas unchanged)
// ---------------------------------------------------------------------------

/// Register workspace tool executors.
pub fn register(registry: &mut ToolRegistry, sandbox: crate::sandbox::SandboxConfig) -> Result<()> {
    registry.register(read_def(), Box::new(ReadExecutor))?;
    registry.register(write_def(), Box::new(WriteExecutor))?;
    registry.register(edit_def(), Box::new(EditExecutor))?;
    registry.register(exec_def(), Box::new(ExecExecutor { sandbox }))?;
    Ok(())
}

fn read_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("read").expect("valid tool name"),
        description: "Read a file's contents as text".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "maxLines".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum lines to return (default: all)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn write_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("write").expect("valid tool name"),
        description: "Write content to a file, creating parent directories as needed".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Content to write".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "append".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Append instead of overwrite (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["path".to_owned(), "content".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn edit_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("edit").expect("valid tool name"),
        description: "Replace exact text in a file with new text".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "old_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Exact text to find in the file".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "new_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Replacement text".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "path".to_owned(),
                "old_text".to_owned(),
                "new_text".to_owned(),
            ],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn exec_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("exec").expect("valid tool name"),
        description: "Execute a shell command in your workspace and return stdout/stderr"
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "command".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "The shell command to execute".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeout".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Timeout in milliseconds (default 30000)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(30000)),
                    },
                ),
            ]),
            required: vec!["command".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
