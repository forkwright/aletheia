//! Tests for workspace path operations and navigation.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use aletheia_koina::id::{NousId, SessionId};

use super::super::*;

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

#[tokio::test]
async fn read_existing_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("hello.txt"), "hello world").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("read", serde_json::json!({ "path": "hello.txt" }));
    let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
    assert_eq!(
        result.content.text_summary(),
        "hello world",
        "read should return file contents"
    );
    assert!(
        !result.is_error,
        "reading an existing file should not produce an error"
    );
}

#[tokio::test]
async fn read_with_max_lines() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("lines.txt"), "a\nb\nc\nd\ne\n").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "read",
        serde_json::json!({ "path": "lines.txt", "maxLines": 2 }),
    );
    let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
    assert_eq!(
        result.content.text_summary(),
        "a\nb",
        "read with maxLines=2 should return only the first 2 lines"
    );
    assert!(
        !result.is_error,
        "reading with maxLines should not produce an error"
    );
}

#[tokio::test]
async fn read_missing_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("read", serde_json::json!({ "path": "nope.txt" }));
    let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(
        result.is_error,
        "reading a missing file should produce an error result"
    );
    assert!(
        result.content.text_summary().contains("file not found"),
        "error message should indicate file not found"
    );
}

#[tokio::test]
async fn write_creates_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "out.txt", "content": "data" }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "writing a new file should not produce an error"
    );
    assert!(
        result.content.text_summary().contains("wrote 4 bytes"),
        "success message should report bytes written"
    );
    let on_disk = std::fs::read_to_string(dir.path().join("out.txt")).expect("read");
    assert_eq!(
        on_disk, "data",
        "file content should match what was written"
    );
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
    assert!(
        !result.is_error,
        "writing to a nested path should create parent directories"
    );
    let on_disk = std::fs::read_to_string(dir.path().join("sub/deep/file.txt")).expect("read");
    assert_eq!(
        on_disk, "nested",
        "nested file content should match what was written"
    );
}

#[tokio::test]
async fn write_append_mode() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("log.txt"), "first\n").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "log.txt", "content": "second\n", "append": true }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(!result.is_error, "append write should not produce an error");
    let on_disk = std::fs::read_to_string(dir.path().join("log.txt")).expect("read");
    assert_eq!(
        on_disk, "first\nsecond\n",
        "append mode should add content after existing data"
    );
}

#[tokio::test]
async fn edit_single_match() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
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
    assert!(
        !result.is_error,
        "editing a file with a single match should not produce an error"
    );
    assert!(
        result.content.text_summary().contains("edited"),
        "success message should indicate the file was edited"
    );
    let on_disk = std::fs::read_to_string(dir.path().join("code.rs")).expect("read");
    assert_eq!(
        on_disk, "fn new_name() {}",
        "file should contain the replacement text"
    );
}

#[tokio::test]
async fn edit_not_found() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
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
    assert!(
        result.is_error,
        "editing with a non-existent old_text should produce an error result"
    );
    assert!(
        result.content.text_summary().contains("old_text not found"),
        "error message should indicate old_text was not found"
    );
}

#[tokio::test]
async fn edit_multiple_matches() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
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
    assert!(
        result.is_error,
        "editing with multiple matches should produce an error result"
    );
    assert!(
        result.content.text_summary().contains("2 times"),
        "error message should indicate the number of matches found"
    );
}

#[tokio::test]
async fn exec_simple_command() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("exec", serde_json::json!({ "command": "echo hello" }));
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(
        !result.is_error,
        "executing a simple echo command should not produce an error"
    );
    assert!(
        result.content.text_summary().contains("hello"),
        "output should contain the echoed text"
    );
    assert!(
        result.content.text_summary().contains("exit=0"),
        "output should report zero exit code"
    );
}

#[tokio::test]
async fn exec_timeout() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "exec",
        serde_json::json!({ "command": "sleep 60", "timeout": 200 }),
    );
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(
        result.is_error,
        "a command exceeding its timeout should produce an error result"
    );
    assert!(
        result.content.text_summary().contains("timed out"),
        "error message should indicate the command timed out"
    );
}

#[tokio::test]
async fn path_traversal_blocked() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("read", serde_json::json!({ "path": "../../etc/passwd" }));
    let err = ReadExecutor
        .execute(&input, &ctx)
        .await
        .expect_err("should reject traversal");
    assert!(
        err.to_string().contains("outside allowed roots"),
        "path traversal should be rejected with appropriate error message"
    );
}

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
    assert!(
        err.to_string().contains("missing or invalid field"),
        "missing path argument should produce a field validation error"
    );
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
    assert!(
        err.to_string().contains("missing or invalid field"),
        "missing content argument should produce a field validation error"
    );
}

#[tokio::test]
async fn test_edit_when_old_text_argument_missing_returns_error() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
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
    assert!(
        err.to_string().contains("missing or invalid field"),
        "missing old_text argument should produce a field validation error"
    );
}

#[tokio::test]
async fn test_exec_when_command_argument_missing_returns_error() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("exec", serde_json::json!({}));
    let err = (ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    })
    .execute(&input, &ctx)
    .await
    .expect_err("missing command should error");
    assert!(
        err.to_string().contains("missing or invalid field"),
        "missing command argument should produce a field validation error"
    );
}

#[tokio::test]
async fn test_read_ignores_unknown_extra_fields() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("hi.txt"), "hello").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "read",
        serde_json::json!({ "path": "hi.txt", "unknownField": "ignored" }),
    );
    let result = ReadExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "read with unknown extra fields should not produce an error"
    );
    assert_eq!(
        result.content.text_summary(),
        "hello",
        "unknown fields should be ignored and file should be read normally"
    );
}

#[tokio::test]
async fn test_write_reports_byte_count_in_success_message() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "out.txt", "content": "hello" }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "writing a file should not produce an error"
    );
    assert!(
        result.content.text_summary().contains("wrote 5 bytes"),
        "success message should report the correct byte count"
    );
}

#[tokio::test]
async fn test_write_overwrite_replaces_existing_content() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("out.txt"), "old content").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "out.txt", "content": "new content" }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "overwrite write should not produce an error"
    );
    let on_disk = std::fs::read_to_string(dir.path().join("out.txt")).expect("read");
    assert_eq!(
        on_disk, "new content",
        "overwrite should replace existing file content"
    );
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
    assert!(
        !result.is_error,
        "append to non-existent file should create it without error"
    );
    let on_disk = std::fs::read_to_string(dir.path().join("new.txt")).expect("read");
    assert_eq!(
        on_disk, "data",
        "append mode should create the file with the provided content"
    );
}

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
    assert!(
        result.is_error,
        "editing a non-existent file should produce an error result"
    );
    assert!(
        result.content.text_summary().contains("file not found"),
        "error message should indicate the file was not found"
    );
}

#[tokio::test]
async fn test_edit_success_message_contains_path() {
    let dir = tempfile::tempdir().expect("create temp dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
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
    assert!(
        !result.is_error,
        "edit should succeed when old_text is found exactly once"
    );
    let text = result.content.text_summary();
    assert!(text.contains("code.rs"), "message should mention path");
}

#[tokio::test]
async fn test_edit_preserves_surrounding_content() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let original = "line1\nTARGET\nline3\n";
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
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
    assert!(
        !result.is_error,
        "edit should succeed when target text is found"
    );
    let on_disk = std::fs::read_to_string(dir.path().join("f.txt")).expect("read");
    assert_eq!(
        on_disk, "line1\nREPLACED\nline3\n",
        "surrounding content should be preserved after edit"
    );
}

#[tokio::test]
async fn test_exec_failed_command_reports_nonzero_exit_code() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    // WHY: Use explicit `sh -c 'exit 42'` to test non-zero exit codes.
    // Exec no longer passes the whole string to a shell: the shell is invoked
    // explicitly here as the program, which is safe.
    let input = tool_input("exec", serde_json::json!({ "command": "sh -c 'exit 42'" }));
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(
        !result.is_error,
        "a failed command should not itself be an error result"
    );
    assert!(
        result.content.text_summary().contains("exit=42"),
        "output should include the non-zero exit code"
    );
}

#[tokio::test]
async fn test_exec_stderr_captured_in_output() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    // WHY: Shell redirection (>&2) is a shell feature, not available without
    // an explicit shell invocation. The program is `sh`, not a raw string.
    let input = tool_input(
        "exec",
        serde_json::json!({ "command": "sh -c 'echo errline >&2'" }),
    );
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(
        !result.is_error,
        "a command that writes to stderr should not produce an error result"
    );
    assert!(
        result.content.text_summary().contains("errline"),
        "stderr output should be captured in the result"
    );
}

#[tokio::test]
async fn test_exec_working_directory_is_workspace() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("exec", serde_json::json!({ "command": "pwd" }));
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(
        !result.is_error,
        "pwd command should not produce an error result"
    );
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
    // WHY: Shell features (>&2, semicolons) require explicit shell invocation
    // now that exec no longer passes the command through sh -c.
    let input = tool_input(
        "exec",
        serde_json::json!({ "command": "sh -c \"printf 'out'; echo err >&2\"" }),
    );
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(!result.is_error, "exec should not produce an error result");
    let text = result.content.text_summary();
    let exit_pos = text.find("exit=0").expect("exit marker");
    let out_pos = text.find("out").expect("stdout");
    assert!(exit_pos < out_pos, "exit code should precede stdout");
}

#[test]
fn test_validate_path_empty_string_returns_error() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let name = aletheia_koina::id::ToolName::new("read").expect("valid");
    let ctx = test_ctx(dir.path());
    let err = validate_path("", &ctx, &name).expect_err("empty path should fail");
    assert!(
        err.to_string().contains("path must not be empty"),
        "empty path should produce an appropriate error message"
    );
}

#[test]
fn test_expand_tilde_str_expands_home() {
    if let Ok(home) = std::env::var("HOME") {
        let expanded = expand_tilde_str("~/notes.txt");
        assert_eq!(
            expanded,
            format!("{home}/notes.txt"),
            "tilde prefix should be expanded to HOME"
        );

        let expanded_bare = expand_tilde_str("~");
        assert_eq!(expanded_bare, home, "bare tilde should expand to HOME");
    }
}

#[test]
fn test_expand_tilde_str_leaves_non_tilde_unchanged() {
    let result = expand_tilde_str("/absolute/path");
    assert_eq!(
        result, "/absolute/path",
        "absolute path without tilde should be unchanged"
    );

    let result2 = expand_tilde_str("relative/path");
    assert_eq!(
        result2, "relative/path",
        "relative path without tilde should be unchanged"
    );
}

#[test]
fn test_validate_path_tilde_expands_to_home_before_resolution() {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = std::path::PathBuf::from(&home);
        let ctx = ToolContext {
            nous_id: aletheia_koina::id::NousId::new("test-agent").expect("valid"),
            session_id: aletheia_koina::id::SessionId::new(),
            workspace: home_path.clone(),
            allowed_roots: vec![home_path.clone()],
            services: None,
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        };
        let name = aletheia_koina::id::ToolName::new("read").expect("valid");

        let resolved = validate_path("~/file.txt", &ctx, &name).expect("tilde path should resolve");
        assert!(
            resolved.starts_with(&home_path),
            "resolved path should be under HOME: {}",
            resolved.display()
        );
    }
}

#[test]
fn test_validate_path_relative_resolves_inside_workspace() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let name = aletheia_koina::id::ToolName::new("read").expect("valid");
    let ctx = test_ctx(dir.path());
    let resolved = validate_path("sub/file.txt", &ctx, &name).expect("valid relative path");
    assert!(
        resolved.starts_with(dir.path()),
        "resolved path should be under the workspace directory"
    );
    assert!(
        resolved.ends_with("sub/file.txt"),
        "resolved path should end with the relative path provided"
    );
}

#[test]
fn test_validate_path_rejects_absolute_outside_allowed_roots() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let name = aletheia_koina::id::ToolName::new("read").expect("valid");
    let ctx = test_ctx(dir.path());
    let err = validate_path("/etc/shadow", &ctx, &name).expect_err("outside roots");
    assert!(
        err.to_string().contains("outside allowed roots"),
        "absolute path outside allowed roots should be rejected"
    );
}

#[test]
fn test_normalize_removes_parent_dir_traversal() {
    let input = Path::new("/a/b/../c");
    let result = normalize(input);
    assert_eq!(
        result,
        Path::new("/a/c"),
        "parent directory traversal should be resolved"
    );
}

#[test]
fn test_normalize_removes_current_dir_component() {
    let input = Path::new("/a/./b/./c");
    let result = normalize(input);
    assert_eq!(
        result,
        Path::new("/a/b/c"),
        "current directory components should be removed"
    );
}
