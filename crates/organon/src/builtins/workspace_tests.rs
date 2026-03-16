#![expect(clippy::expect_used, reason = "test assertions")]
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
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(result.is_error);
    assert!(result.content.text_summary().contains("timed out"));
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
    assert!(err.to_string().contains("outside allowed roots"));
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
    let err = (ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    })
    .execute(&input, &ctx)
    .await
    .expect_err("missing command should error");
    assert!(err.to_string().contains("missing or invalid field"));
}

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

#[tokio::test]
async fn test_exec_failed_command_reports_nonzero_exit_code() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    // WHY: Use explicit `sh -c 'exit 42'` to test non-zero exit codes.
    // Exec no longer passes the whole string to a shell. The shell is invoked
    // explicitly here as the program, which is safe.
    let input = tool_input("exec", serde_json::json!({ "command": "sh -c 'exit 42'" }));
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig::disabled(),
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(!result.is_error);
    assert!(result.content.text_summary().contains("exit=42"));
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
    assert!(!result.is_error);
    assert!(result.content.text_summary().contains("errline"));
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
    assert!(!result.is_error);
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
    assert!(err.to_string().contains("path must not be empty"));
}

#[test]
fn test_expand_tilde_str_expands_home() {
    if let Ok(home) = std::env::var("HOME") {
        let expanded = expand_tilde_str("~/notes.txt");
        assert_eq!(expanded, format!("{home}/notes.txt"));

        let expanded_bare = expand_tilde_str("~");
        assert_eq!(expanded_bare, home);
    }
}

#[test]
fn test_expand_tilde_str_leaves_non_tilde_unchanged() {
    let result = expand_tilde_str("/absolute/path");
    assert_eq!(result, "/absolute/path");

    let result2 = expand_tilde_str("relative/path");
    assert_eq!(result2, "relative/path");
}

#[test]
fn test_validate_path_tilde_expands_to_home_before_resolution() {
    // Build a ctx whose workspace is the HOME directory so the tilde-expanded
    // path is inside allowed_roots.
    if let Ok(home) = std::env::var("HOME") {
        let home_path = std::path::PathBuf::from(&home);
        // workspace = HOME, allowed_roots = [HOME]
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

        // "~/file.txt" must resolve to HOME/file.txt, which is inside HOME.
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

#[tokio::test]
async fn test_all_workspace_tools_registered() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg, crate::sandbox::SandboxConfig::disabled()).expect("register");
    for name in ["read", "write", "edit", "exec"] {
        let tn = aletheia_koina::id::ToolName::new(name).expect("valid");
        assert!(reg.get_def(&tn).is_some(), "{name} should be registered");
    }
}

#[test]
fn test_read_tool_def_has_path_as_required() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg, crate::sandbox::SandboxConfig::disabled()).expect("register");
    let tn = aletheia_koina::id::ToolName::new("read").expect("valid");
    let def = reg.get_def(&tn).expect("read registered");
    assert!(def.input_schema.required.contains(&"path".to_owned()));
}

#[test]
fn test_write_tool_def_has_path_and_content_as_required() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg, crate::sandbox::SandboxConfig::disabled()).expect("register");
    let tn = aletheia_koina::id::ToolName::new("write").expect("valid");
    let def = reg.get_def(&tn).expect("write registered");
    assert!(def.input_schema.required.contains(&"path".to_owned()));
    assert!(def.input_schema.required.contains(&"content".to_owned()));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn exec_permissive_sandbox_runs_tool_regardless_of_landlock_availability() {
    // Permissive enforcement: tool must run whether or not Landlock is available
    // on the kernel. This covers the graceful degradation path from #943.
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "exec",
        serde_json::json!({ "command": "echo sandbox-permissive" }),
    );
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig {
            enabled: true,
            enforcement: crate::sandbox::SandboxEnforcement::Permissive,
            ..crate::sandbox::SandboxConfig::default()
        },
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");
    assert!(
        !result.is_error,
        "tool must run in permissive mode: {:?}",
        result.content.text_summary()
    );
    assert!(
        result.content.text_summary().contains("sandbox-permissive"),
        "output must be captured: {}",
        result.content.text_summary()
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn exec_enforcing_sandbox_returns_clear_error_when_landlock_unavailable() {
    // Enforcing enforcement when Landlock is absent: must get a clear error
    // naming Landlock and ABI: not an opaque "Permission denied".
    // When Landlock IS available on the CI kernel, the command runs normally.
    use crate::sandbox::probe_landlock_abi;

    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("exec", serde_json::json!({ "command": "echo unreachable" }));
    let result = ExecExecutor {
        sandbox: crate::sandbox::SandboxConfig {
            enabled: true,
            enforcement: crate::sandbox::SandboxEnforcement::Enforcing,
            ..crate::sandbox::SandboxConfig::default()
        },
    }
    .execute(&input, &ctx)
    .await
    .expect("execute");

    match probe_landlock_abi() {
        None => {
            // Landlock unavailable: error result must name the cause clearly.
            assert!(
                result.is_error,
                "enforcing mode must error when Landlock unavailable"
            );
            let msg = result.content.text_summary();
            assert!(
                msg.contains("sandbox setup failed"),
                "error must indicate sandbox setup failure: {msg}"
            );
            assert!(
                msg.contains("Landlock") || msg.contains("ABI"),
                "error must name Landlock or ABI: {msg}"
            );
        }
        Some(_) => {
            // Landlock is available: execution proceeds normally, no opaque error.
            assert!(
                !result.is_error,
                "enforcing mode must succeed when Landlock is available"
            );
        }
    }
}

#[test]
fn parse_command_args_splits_simple_command() {
    let (prog, args) = parse_command_args("echo hello world").expect("parse");
    assert_eq!(prog, "echo");
    assert_eq!(args, ["hello", "world"]);
}

#[test]
fn parse_command_args_handles_single_quoted_string() {
    let (prog, args) = parse_command_args("echo 'hello world'").expect("parse");
    assert_eq!(prog, "echo");
    assert_eq!(args, ["hello world"]);
}

#[test]
fn parse_command_args_handles_double_quoted_string() {
    let (prog, args) = parse_command_args("echo \"hello world\"").expect("parse");
    assert_eq!(prog, "echo");
    assert_eq!(args, ["hello world"]);
}

#[test]
fn parse_command_args_handles_backslash_escape_in_double_quotes() {
    let (prog, args) = parse_command_args("echo \"a\\\"b\"").expect("parse");
    assert_eq!(prog, "echo");
    assert_eq!(args, ["a\"b"]);
}

#[test]
fn parse_command_args_rejects_unterminated_single_quote() {
    let err = parse_command_args("echo 'hello").expect_err("should fail");
    assert!(err.contains("unterminated single quote"));
}

#[test]
fn parse_command_args_rejects_empty_command() {
    let err = parse_command_args("").expect_err("should fail");
    assert!(err.contains("empty"));
}

#[test]
fn parse_command_args_rejects_whitespace_only_command() {
    let err = parse_command_args("   ").expect_err("should fail");
    assert!(err.contains("empty"));
}

#[test]
fn parse_command_args_treats_shell_metacharacters_as_literals() {
    // WHY: Shell injection prevention. Semicolons, pipes, and ampersands must
    // not be interpreted as command separators or operators when the LLM
    // includes them in a command string.
    let (prog, args) = parse_command_args("echo hello; rm -rf /").expect("parse");
    assert_eq!(prog, "echo");
    assert_eq!(args, ["hello;", "rm", "-rf", "/"]);
}

#[test]
fn parse_command_args_treats_dollar_sign_as_literal() {
    let (prog, args) = parse_command_args("echo $HOME").expect("parse");
    assert_eq!(prog, "echo");
    assert_eq!(args, ["$HOME"]);
}

#[test]
fn parse_command_args_program_only() {
    let (prog, args) = parse_command_args("ls").expect("parse");
    assert_eq!(prog, "ls");
    assert!(args.is_empty());
}

#[tokio::test]
async fn write_blocks_identity_md() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "IDENTITY.md", "content": "tampered" }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error);
    assert!(
        result.content.text_summary().contains("protected"),
        "must mention protected: {}",
        result.content.text_summary()
    );
}

#[tokio::test]
async fn write_blocks_soul_md() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "SOUL.md", "content": "overwritten" }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error);
    assert!(result.content.text_summary().contains("protected"));
}

#[tokio::test]
async fn write_blocks_goals_md() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "GOALS.md", "content": "replaced" }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error);
    assert!(result.content.text_summary().contains("protected"));
}

#[tokio::test]
async fn write_allows_non_protected_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "write",
        serde_json::json!({ "path": "notes.txt", "content": "safe" }),
    );
    let result = WriteExecutor.execute(&input, &ctx).await.expect("execute");
    assert!(!result.is_error, "non-protected file must be writable");
}
