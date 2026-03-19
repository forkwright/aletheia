//! Tests for workspace filesystem operations.
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
    assert_eq!(result.content.text_summary(), "a\nb",);
}

#[test]
fn test_normalize_handles_multiple_parent_traversals() {
    let input = Path::new("/a/b/c/../../d");
    let result = normalize(input);
    assert_eq!(
        result,
        Path::new("/a/d"),
        "multiple parent directory traversals should all be resolved"
    );
}

#[test]
fn test_extract_str_missing_field_returns_invalid_input_error() {
    use aletheia_koina::id::ToolName;
    let name = ToolName::new("test").expect("valid");
    let args = serde_json::json!({ "other": "value" });
    let err = extract_str(&args, "path", &name).expect_err("missing should fail");
    assert!(
        err.to_string().contains("missing or invalid field: path"),
        "missing field error should name the missing field"
    );
}

#[test]
fn test_extract_str_non_string_value_returns_error() {
    use aletheia_koina::id::ToolName;
    let name = ToolName::new("test").expect("valid");
    let args = serde_json::json!({ "path": 42 });
    let err = extract_str(&args, "path", &name).expect_err("wrong type should fail");
    assert!(
        err.to_string().contains("missing or invalid field: path"),
        "wrong type error should name the invalid field"
    );
}

#[test]
fn test_extract_opt_u64_returns_none_when_field_absent() {
    let args = serde_json::json!({});
    assert_eq!(
        extract_opt_u64(&args, "maxLines"),
        None,
        "absent field should return None"
    );
}

#[test]
fn test_extract_opt_u64_returns_value_when_field_present() {
    let args = serde_json::json!({ "maxLines": 42 });
    assert_eq!(
        extract_opt_u64(&args, "maxLines"),
        Some(42),
        "present numeric field should return its value wrapped in Some"
    );
}

#[test]
fn test_extract_opt_bool_returns_none_when_field_absent() {
    let args = serde_json::json!({});
    assert_eq!(
        extract_opt_bool(&args, "append"),
        None,
        "absent field should return None"
    );
}

#[test]
fn test_extract_opt_bool_returns_value_when_field_present() {
    let args = serde_json::json!({ "append": true });
    assert_eq!(
        extract_opt_bool(&args, "append"),
        Some(true),
        "present boolean field should return its value wrapped in Some"
    );
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
    assert!(
        def.input_schema.required.contains(&"path".to_owned()),
        "read tool schema should require the path field"
    );
}

#[test]
fn test_write_tool_def_has_path_and_content_as_required() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg, crate::sandbox::SandboxConfig::disabled()).expect("register");
    let tn = aletheia_koina::id::ToolName::new("write").expect("valid");
    let def = reg.get_def(&tn).expect("write registered");
    assert!(
        def.input_schema.required.contains(&"path".to_owned()),
        "write tool schema should require the path field"
    );
    assert!(
        def.input_schema.required.contains(&"content".to_owned()),
        "write tool schema should require the content field"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn exec_permissive_sandbox_runs_tool_regardless_of_landlock_availability() {
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
    assert_eq!(prog, "echo", "program should be the first token");
    assert_eq!(
        args,
        ["hello", "world"],
        "remaining tokens should be arguments"
    );
}

#[test]
fn parse_command_args_handles_single_quoted_string() {
    let (prog, args) = parse_command_args("echo 'hello world'").expect("parse");
    assert_eq!(
        prog, "echo",
        "program should be parsed correctly with quoted args"
    );
    assert_eq!(
        args,
        ["hello world"],
        "single-quoted string should be treated as one argument"
    );
}

#[test]
fn parse_command_args_handles_double_quoted_string() {
    let (prog, args) = parse_command_args("echo \"hello world\"").expect("parse");
    assert_eq!(
        prog, "echo",
        "program should be parsed correctly with double-quoted args"
    );
    assert_eq!(
        args,
        ["hello world"],
        "double-quoted string should be treated as one argument"
    );
}

#[test]
fn parse_command_args_handles_backslash_escape_in_double_quotes() {
    let (prog, args) = parse_command_args("echo \"a\\\"b\"").expect("parse");
    assert_eq!(
        prog, "echo",
        "program should be parsed correctly with escaped quotes"
    );
    assert_eq!(
        args,
        ["a\"b"],
        "backslash-escaped double quote inside double quotes should be unescaped"
    );
}

#[test]
fn parse_command_args_rejects_unterminated_single_quote() {
    let err = parse_command_args("echo 'hello").expect_err("should fail");
    assert!(
        err.contains("unterminated single quote"),
        "unterminated single quote should produce an appropriate error message"
    );
}

#[test]
fn parse_command_args_rejects_empty_command() {
    let err = parse_command_args("").expect_err("should fail");
    assert!(
        err.contains("empty"),
        "empty command string should produce an error mentioning 'empty'"
    );
}

#[test]
fn parse_command_args_rejects_whitespace_only_command() {
    let err = parse_command_args("   ").expect_err("should fail");
    assert!(
        err.contains("empty"),
        "whitespace-only command string should produce an error mentioning 'empty'"
    );
}

#[test]
fn parse_command_args_treats_shell_metacharacters_as_literals() {
    // WHY: Shell injection prevention: semicolons, pipes, and ampersands must
    // not be interpreted as command separators or operators when the LLM
    // includes them in a command string.
    let (prog, args) = parse_command_args("echo hello; rm -rf /").expect("parse");
    assert_eq!(
        prog, "echo",
        "program should be the first token even with shell metacharacters present"
    );
    assert_eq!(
        args,
        ["hello;", "rm", "-rf", "/"],
        "shell metacharacters should be treated as literals, not operators"
    );
}

#[test]
fn parse_command_args_treats_dollar_sign_as_literal() {
    let (prog, args) = parse_command_args("echo $HOME").expect("parse");
    assert_eq!(
        prog, "echo",
        "program should be parsed correctly when args include dollar signs"
    );
    assert_eq!(
        args,
        ["$HOME"],
        "dollar sign should be treated as a literal, not expanded"
    );
}

#[test]
fn parse_command_args_program_only() {
    let (prog, args) = parse_command_args("ls").expect("parse");
    assert_eq!(
        prog, "ls",
        "single-token command should be parsed as the program"
    );
    assert!(
        args.is_empty(),
        "program-only command should have no arguments"
    );
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
    assert!(
        result.is_error,
        "writing to IDENTITY.md should produce an error result"
    );
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
    assert!(
        result.is_error,
        "writing to SOUL.md should produce an error result"
    );
    assert!(
        result.content.text_summary().contains("protected"),
        "error message should indicate the file is protected"
    );
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
    assert!(
        result.is_error,
        "writing to GOALS.md should produce an error result"
    );
    assert!(
        result.content.text_summary().contains("protected"),
        "error message should indicate the file is protected"
    );
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
