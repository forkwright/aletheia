//! Workspace tool executors: read, write, edit, exec.

use std::future::Future;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

struct ExecExecutor;

impl ToolExecutor for ExecExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let command = extract_str(&input.arguments, "command", &input.name)?;
            let timeout_ms = extract_opt_u64(&input.arguments, "timeout").unwrap_or(30_000);

            let mut child = match Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.workspace)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    return Ok(err_result(format!("spawn failed: {e}")));
                }
            };

            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let status = loop {
                match child.try_wait() {
                    Ok(Some(s)) => break s,
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
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

            let mut stdout = String::new();
            if let Some(ref mut pipe) = child.stdout {
                let _ = pipe.read_to_string(&mut stdout);
            }
            let mut stderr = String::new();
            if let Some(ref mut pipe) = child.stderr {
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
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(read_def(), Box::new(ReadExecutor))?;
    registry.register(write_def(), Box::new(WriteExecutor))?;
    registry.register(edit_def(), Box::new(EditExecutor))?;
    registry.register(exec_def(), Box::new(ExecExecutor))?;
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
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use aletheia_koina::id::{NousId, SessionId};

    use super::*;

    fn test_ctx(dir: &Path) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: dir.to_path_buf(),
            allowed_roots: vec![dir.to_path_buf()],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: ToolName::new(name).expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: args,
        }
    }

    // -- ReadExecutor -------------------------------------------------------

    #[tokio::test]
    async fn read_existing_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("hello.txt"), "hello world").expect("write");

        let ctx = test_ctx(dir.path());
        let input = tool_input("read", serde_json::json!({ "path": "hello.txt" }));
        let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
        assert_eq!(result.content.text_summary(), "hello world");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn read_with_max_lines() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("lines.txt"), "a\nb\nc\nd\ne\n").expect("write");

        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "read",
            serde_json::json!({ "path": "lines.txt", "maxLines": 2 }),
        );
        let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
        assert_eq!(result.content.text_summary(), "a\nb");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn read_missing_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("read", serde_json::json!({ "path": "nope.txt" }));
        let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("file not found"));
    }

    // -- WriteExecutor ------------------------------------------------------

    #[tokio::test]
    async fn write_creates_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "write",
            serde_json::json!({ "path": "out.txt", "content": "data" }),
        );
        let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("wrote 4 bytes"));
        let on_disk = std::fs::read_to_string(dir.path().join("out.txt")).expect("read");
        assert_eq!(on_disk, "data");
    }

    #[tokio::test]
    async fn write_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "write",
            serde_json::json!({ "path": "sub/deep/file.txt", "content": "nested" }),
        );
        let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let on_disk = std::fs::read_to_string(dir.path().join("sub/deep/file.txt")).expect("read");
        assert_eq!(on_disk, "nested");
    }

    #[tokio::test]
    async fn write_append_mode() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("log.txt"), "first\n").expect("write");

        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "write",
            serde_json::json!({ "path": "log.txt", "content": "second\n", "append": true }),
        );
        let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let on_disk = std::fs::read_to_string(dir.path().join("log.txt")).expect("read");
        assert_eq!(on_disk, "first\nsecond\n");
    }

    // -- EditExecutor -------------------------------------------------------

    #[tokio::test]
    async fn edit_single_match() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("code.rs"), "fn old_name() {}").expect("write");

        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "edit",
            serde_json::json!({
                "path": "code.rs",
                "old_text": "old_name",
                "new_text": "new_name"
            }),
        );
        let result = EditExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("edited"));
        let on_disk = std::fs::read_to_string(dir.path().join("code.rs")).expect("read");
        assert_eq!(on_disk, "fn new_name() {}");
    }

    #[tokio::test]
    async fn edit_not_found() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("code.rs"), "fn hello() {}").expect("write");

        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "edit",
            serde_json::json!({
                "path": "code.rs",
                "old_text": "nonexistent",
                "new_text": "whatever"
            }),
        );
        let result = EditExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("old_text not found"));
    }

    #[tokio::test]
    async fn edit_multiple_matches() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("dup.txt"), "aaa bbb aaa").expect("write");

        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "edit",
            serde_json::json!({
                "path": "dup.txt",
                "old_text": "aaa",
                "new_text": "ccc"
            }),
        );
        let result = EditExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("2 times"));
    }

    // -- ExecExecutor -------------------------------------------------------

    #[tokio::test]
    async fn exec_simple_command() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("exec", serde_json::json!({ "command": "echo hello" }));
        let result = ExecExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("hello"));
        assert!(result.content.text_summary().contains("exit=0"));
    }

    #[tokio::test]
    async fn exec_timeout() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "exec",
            serde_json::json!({ "command": "sleep 60", "timeout": 200 }),
        );
        let result = ExecExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("timed out"));
    }

    // -- Path traversal -----------------------------------------------------

    #[tokio::test]
    async fn path_traversal_blocked() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("read", serde_json::json!({ "path": "../../etc/passwd" }));
        let err = ReadExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("should reject traversal");
        assert!(err.to_string().contains("outside allowed roots"));
    }

    // -- Parameter validation -----------------------------------------------

    #[tokio::test]
    async fn test_read_when_path_argument_missing_returns_invalid_input_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("read", serde_json::json!({}));
        let err = ReadExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("missing path should error");
        assert!(
            err.to_string().contains("missing or invalid field"),
            "error: {err}"
        );
    }

    #[tokio::test]
    async fn test_write_when_path_argument_missing_returns_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("write", serde_json::json!({ "content": "data" }));
        let err = WriteExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("missing path should error");
        assert!(err.to_string().contains("missing or invalid field"));
    }

    #[tokio::test]
    async fn test_write_when_content_argument_missing_returns_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("write", serde_json::json!({ "path": "out.txt" }));
        let err = WriteExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("missing content should error");
        assert!(err.to_string().contains("missing or invalid field"));
    }

    #[tokio::test]
    async fn test_edit_when_old_text_argument_missing_returns_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("f.txt"), "hello world").expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "edit",
            serde_json::json!({ "path": "f.txt", "new_text": "bye" }),
        );
        let err = EditExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("missing old_text should error");
        assert!(err.to_string().contains("missing or invalid field"));
    }

    #[tokio::test]
    async fn test_exec_when_command_argument_missing_returns_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("exec", serde_json::json!({}));
        let err = ExecExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("missing command should error");
        assert!(err.to_string().contains("missing or invalid field"));
    }

    // -- Extra / unknown params handled gracefully --------------------------

    #[tokio::test]
    async fn test_read_ignores_unknown_extra_fields() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("hi.txt"), "hello").expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "read",
            serde_json::json!({ "path": "hi.txt", "unknownField": "ignored" }),
        );
        let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert_eq!(result.content.text_summary(), "hello");
    }

    // -- Write result formatting --------------------------------------------

    #[tokio::test]
    async fn test_write_reports_byte_count_in_success_message() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "write",
            serde_json::json!({ "path": "out.txt", "content": "hello" }),
        );
        let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("wrote 5 bytes"));
    }

    #[tokio::test]
    async fn test_write_overwrite_replaces_existing_content() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("out.txt"), "old content").expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "write",
            serde_json::json!({ "path": "out.txt", "content": "new content" }),
        );
        let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let on_disk = std::fs::read_to_string(dir.path().join("out.txt")).expect("read");
        assert_eq!(on_disk, "new content");
    }

    #[tokio::test]
    async fn test_write_append_creates_file_when_absent() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "write",
            serde_json::json!({ "path": "new.txt", "content": "data", "append": true }),
        );
        let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let on_disk = std::fs::read_to_string(dir.path().join("new.txt")).expect("read");
        assert_eq!(on_disk, "data");
    }

    // -- Edit result formatting ---------------------------------------------

    #[tokio::test]
    async fn test_edit_when_file_does_not_exist_returns_error_result() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "edit",
            serde_json::json!({
                "path": "nonexistent.txt",
                "old_text": "x",
                "new_text": "y"
            }),
        );
        let result = EditExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("file not found"));
    }

    #[tokio::test]
    async fn test_edit_success_message_contains_path() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("code.rs"), "fn old_name() {}").expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "edit",
            serde_json::json!({
                "path": "code.rs",
                "old_text": "old_name",
                "new_text": "new_name"
            }),
        );
        let result = EditExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let text = result.content.text_summary();
        assert!(text.contains("code.rs"), "message should mention path");
    }

    #[tokio::test]
    async fn test_edit_preserves_surrounding_content() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let original = "line1\nTARGET\nline3\n";
        std::fs::write(dir.path().join("f.txt"), original).expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "edit",
            serde_json::json!({
                "path": "f.txt",
                "old_text": "TARGET",
                "new_text": "REPLACED"
            }),
        );
        let result = EditExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let on_disk = std::fs::read_to_string(dir.path().join("f.txt")).expect("read");
        assert_eq!(on_disk, "line1\nREPLACED\nline3\n");
    }

    // -- Exec result formatting ---------------------------------------------

    #[tokio::test]
    async fn test_exec_failed_command_reports_nonzero_exit_code() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("exec", serde_json::json!({ "command": "exit 42" }));
        let result = ExecExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("exit=42"));
    }

    #[tokio::test]
    async fn test_exec_stderr_captured_in_output() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("exec", serde_json::json!({ "command": "echo errline >&2" }));
        let result = ExecExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        assert!(result.content.text_summary().contains("errline"));
    }

    #[tokio::test]
    async fn test_exec_working_directory_is_workspace() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input("exec", serde_json::json!({ "command": "pwd" }));
        let result = ExecExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let text = result.content.text_summary();
        let canonical = dir.path().canonicalize().expect("canon");
        assert!(
            text.contains(canonical.to_string_lossy().as_ref()),
            "pwd should show workspace: {text}"
        );
    }

    #[tokio::test]
    async fn test_exec_output_format_includes_exit_then_stdout_then_stderr() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(
            "exec",
            serde_json::json!({ "command": "printf 'out'; echo err >&2" }),
        );
        let result = ExecExecutor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error);
        let text = result.content.text_summary();
        let exit_pos = text.find("exit=0").expect("exit marker");
        let out_pos = text.find("out").expect("stdout");
        assert!(exit_pos < out_pos, "exit code should precede stdout");
    }

    // -- Helper functions ---------------------------------------------------

    #[test]
    fn test_validate_path_empty_string_returns_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let name = aletheia_koina::id::ToolName::new("read").expect("valid");
        let ctx = test_ctx(dir.path());
        let err = validate_path("", &ctx, &name).expect_err("empty path should fail");
        assert!(err.to_string().contains("path must not be empty"));
    }

    #[test]
    fn test_validate_path_relative_resolves_inside_workspace() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let name = aletheia_koina::id::ToolName::new("read").expect("valid");
        let ctx = test_ctx(dir.path());
        let resolved = validate_path("sub/file.txt", &ctx, &name).expect("valid relative path");
        assert!(resolved.starts_with(dir.path()));
        assert!(resolved.ends_with("sub/file.txt"));
    }

    #[test]
    fn test_validate_path_rejects_absolute_outside_allowed_roots() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let name = aletheia_koina::id::ToolName::new("read").expect("valid");
        let ctx = test_ctx(dir.path());
        let err = validate_path("/etc/shadow", &ctx, &name).expect_err("outside roots");
        assert!(err.to_string().contains("outside allowed roots"));
    }

    #[test]
    fn test_normalize_removes_parent_dir_traversal() {
        let input = Path::new("/a/b/../c");
        let result = normalize(input);
        assert_eq!(result, Path::new("/a/c"));
    }

    #[test]
    fn test_normalize_removes_current_dir_component() {
        let input = Path::new("/a/./b/./c");
        let result = normalize(input);
        assert_eq!(result, Path::new("/a/b/c"));
    }

    #[test]
    fn test_normalize_handles_multiple_parent_traversals() {
        let input = Path::new("/a/b/c/../../d");
        let result = normalize(input);
        assert_eq!(result, Path::new("/a/d"));
    }

    #[test]
    fn test_extract_str_missing_field_returns_invalid_input_error() {
        use aletheia_koina::id::ToolName;
        let name = ToolName::new("test").expect("valid");
        let args = serde_json::json!({ "other": "value" });
        let err = extract_str(&args, "path", &name).expect_err("missing should fail");
        assert!(err.to_string().contains("missing or invalid field: path"));
    }

    #[test]
    fn test_extract_str_non_string_value_returns_error() {
        use aletheia_koina::id::ToolName;
        let name = ToolName::new("test").expect("valid");
        let args = serde_json::json!({ "path": 42 });
        let err = extract_str(&args, "path", &name).expect_err("wrong type should fail");
        assert!(err.to_string().contains("missing or invalid field: path"));
    }

    #[test]
    fn test_extract_opt_u64_returns_none_when_field_absent() {
        let args = serde_json::json!({});
        assert_eq!(extract_opt_u64(&args, "maxLines"), None);
    }

    #[test]
    fn test_extract_opt_u64_returns_value_when_field_present() {
        let args = serde_json::json!({ "maxLines": 42 });
        assert_eq!(extract_opt_u64(&args, "maxLines"), Some(42));
    }

    #[test]
    fn test_extract_opt_bool_returns_none_when_field_absent() {
        let args = serde_json::json!({});
        assert_eq!(extract_opt_bool(&args, "append"), None);
    }

    #[test]
    fn test_extract_opt_bool_returns_value_when_field_present() {
        let args = serde_json::json!({ "append": true });
        assert_eq!(extract_opt_bool(&args, "append"), Some(true));
    }

    // -- Tool registration --------------------------------------------------

    #[tokio::test]
    async fn test_all_workspace_tools_registered() {
        let mut reg = crate::registry::ToolRegistry::new();
        register(&mut reg).expect("register");
        for name in ["read", "write", "edit", "exec"] {
            let tn = aletheia_koina::id::ToolName::new(name).expect("valid");
            assert!(reg.get_def(&tn).is_some(), "{name} should be registered");
        }
    }

    #[test]
    fn test_read_tool_def_has_path_as_required() {
        let mut reg = crate::registry::ToolRegistry::new();
        register(&mut reg).expect("register");
        let tn = aletheia_koina::id::ToolName::new("read").expect("valid");
        let def = reg.get_def(&tn).expect("read registered");
        assert!(def.input_schema.required.contains(&"path".to_owned()));
    }

    #[test]
    fn test_write_tool_def_has_path_and_content_as_required() {
        let mut reg = crate::registry::ToolRegistry::new();
        register(&mut reg).expect("register");
        let tn = aletheia_koina::id::ToolName::new("write").expect("valid");
        let def = reg.get_def(&tn).expect("write registered");
        assert!(def.input_schema.required.contains(&"path".to_owned()));
        assert!(def.input_schema.required.contains(&"content".to_owned()));
    }
}
