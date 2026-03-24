//! Tests for path validation, normalization, and navigation security.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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

#[test]
fn test_validate_path_accepts_canonical_root_with_symlinked_input() {
    // WHY: oikos canonicalizes roots at startup, so the allowed_roots contain
    // the canonical (symlink-resolved) path. Input paths using the non-canonical
    // form (through a symlink) must still be accepted. Closes #1981.
    let dir = tempfile::tempdir().expect("create temp dir");
    let real_dir = dir.path().join("real");
    std::fs::create_dir(&real_dir).expect("create real dir");
    std::fs::create_dir(real_dir.join("sub")).expect("create sub dir");

    // NOTE: Create a symlink pointing to the real directory
    #[cfg(unix)]
    {
        let link_dir = dir.path().join("link");
        std::os::unix::fs::symlink(&real_dir, &link_dir).expect("create symlink");

        // Set allowed_roots to the CANONICAL (real) path
        let canonical_root = real_dir.canonicalize().expect("canonicalize real");
        let ctx = ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: canonical_root.clone(),
            allowed_roots: vec![canonical_root],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        };
        let name = aletheia_koina::id::ToolName::new("ls").expect("valid");

        // Validate a path using the SYMLINK form -- should pass
        let link_sub = link_dir.join("sub");
        let result = validate_path(link_sub.to_str().expect("utf8"), &ctx, &name);
        assert!(
            result.is_ok(),
            "path through symlink should be accepted when canonical root matches: {result:?}"
        );
    }
}

#[test]
fn test_validate_path_trailing_slash_in_root() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let name = aletheia_koina::id::ToolName::new("read").expect("valid");

    // WHY: Trailing slashes in allowed roots must not break the prefix check.
    let root_with_slash = PathBuf::from(format!("{}/", dir.path().display()));
    let ctx = ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![root_with_slash],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    };

    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("file.txt"), "data").expect("write");

    let result = validate_path("file.txt", &ctx, &name);
    assert!(
        result.is_ok(),
        "path within root with trailing slash should be accepted: {result:?}"
    );
}

#[test]
fn test_validate_path_root_exact_match() {
    // WHY: `ls` on the root itself should be allowed.
    let dir = tempfile::tempdir().expect("create temp dir");
    let name = aletheia_koina::id::ToolName::new("ls").expect("valid");
    let canonical = dir.path().canonicalize().expect("canonicalize");
    let ctx = ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: canonical.clone(),
        allowed_roots: vec![canonical.clone()],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    };

    let result = validate_path(canonical.to_str().expect("utf8"), &ctx, &name);
    assert!(
        result.is_ok(),
        "path that exactly matches an allowed root should be accepted: {result:?}"
    );
}
