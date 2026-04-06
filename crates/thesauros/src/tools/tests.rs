//! Tests for thesauros tools.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;

use super::*;
use crate::manifest::{PackInputSchema, PackManifest, PackPropertyDef, PackToolDef};

fn setup_pack_dir(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("create temp dir");
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        // WHY: explicit File ensures fd is closed before chmod/exec: avoids ETXTBSY
        let file = std::fs::File::create(&path).expect("create pack file");
        std::io::Write::write_all(&mut &file, content.as_bytes()).expect("write pack file content");
        file.sync_all().expect("sync pack file");
        drop(file);
    }
    dir
}

fn make_executable(dir: &TempDir, path: &str) {
    let full = dir.path().join(path);
    let mut perms = fs::metadata(&full)
        .expect("get file metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&full, perms).expect("set executable permissions");
}

fn minimal_loaded_pack(dir: &TempDir, tools: Vec<PackToolDef>) -> LoadedPack {
    LoadedPack {
        manifest: PackManifest {
            name: "test-pack".to_owned(),
            version: "1.0".to_owned(),
            description: None,
            context: vec![],
            tools,
            overlays: std::collections::HashMap::new(),
        },
        sections: vec![],
        root: dir.path().to_path_buf(),
    }
}

#[test]
fn validate_command_path_success() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh\necho ok")]);
    let result = validate_command_path(dir.path(), "tools/test.sh");
    assert!(result.is_ok());
}

#[test]
fn validate_command_path_missing() {
    let dir = setup_pack_dir(&[]);
    let result = validate_command_path(dir.path(), "tools/missing.sh");
    assert!(matches!(
        result.expect_err("missing command path should fail"),
        error::Error::ToolCommandNotFound { .. }
    ));
}

#[test]
fn validate_command_path_escape_rejected() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
    let result = validate_command_path(dir.path(), "../../../etc/passwd");
    // NOTE: returns ToolCommandNotFound (can't canonicalize) or ToolCommandEscape
    let err = result.expect_err("path traversal should be rejected");
    assert!(
        matches!(err, error::Error::ToolCommandNotFound { .. })
            || matches!(err, error::Error::ToolCommandEscape { .. })
    );
}

#[test]
fn parse_property_type_all_variants() {
    assert_eq!(
        parse_property_type("string", "t").expect("string is a valid property type"),
        PropertyType::String
    );
    assert_eq!(
        parse_property_type("number", "t").expect("number is a valid property type"),
        PropertyType::Number
    );
    assert_eq!(
        parse_property_type("integer", "t").expect("integer is a valid property type"),
        PropertyType::Integer
    );
    assert_eq!(
        parse_property_type("boolean", "t").expect("boolean is a valid property type"),
        PropertyType::Boolean
    );
    assert_eq!(
        parse_property_type("array", "t").expect("array is a valid property type"),
        PropertyType::Array
    );
    assert_eq!(
        parse_property_type("object", "t").expect("object is a valid property type"),
        PropertyType::Object
    );
}

#[test]
fn parse_property_type_unknown_rejected() {
    let err =
        parse_property_type("float", "my_tool").expect_err("float is not a valid property type");
    assert!(matches!(err, error::Error::UnknownPropertyType { .. }));
    assert!(err.to_string().contains("float"));
    assert!(err.to_string().contains("my_tool"));
}

#[test]
fn convert_input_schema_success() {
    let schema = PackInputSchema {
        properties: IndexMap::from([
            (
                "sql".to_owned(),
                PackPropertyDef {
                    property_type: "string".to_owned(),
                    description: "SQL query".to_owned(),
                    enum_values: None,
                    default: None,
                },
            ),
            (
                "limit".to_owned(),
                PackPropertyDef {
                    property_type: "integer".to_owned(),
                    description: "Row limit".to_owned(),
                    enum_values: None,
                    default: Some(serde_json::json!(100)),
                },
            ),
        ]),
        required: vec!["sql".to_owned()],
    };

    let result = convert_input_schema(&schema, "test").expect("valid schema should convert");
    assert_eq!(result.properties.len(), 2);
    assert_eq!(result.properties["sql"].property_type, PropertyType::String);
    assert_eq!(
        result.properties["limit"].property_type,
        PropertyType::Integer
    );
    assert_eq!(
        result.properties["limit"].default,
        Some(serde_json::json!(100))
    );
    assert_eq!(result.required, vec!["sql"]);
}

#[test]
fn register_pack_tools_success() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/echo.sh");

    let tool = PackToolDef {
        name: "echo_tool".to_owned(),
        description: "Echo tool".to_owned(),
        command: "tools/echo.sh".to_owned(),
        timeout: 5000,
        input_schema: None,
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(registry.definitions().len(), 1);
    assert_eq!(registry.definitions()[0].name.as_str(), "echo_tool");
    assert_eq!(registry.definitions()[0].category, ToolCategory::Domain);
}

#[test]
fn register_pack_tools_skips_missing_command() {
    let dir = setup_pack_dir(&[]);
    let tool = PackToolDef {
        name: "missing_tool".to_owned(),
        description: "Missing command".to_owned(),
        command: "tools/nonexistent.sh".to_owned(),
        timeout: 5000,
        input_schema: None,
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert_eq!(errors.len(), 1);
    assert!(registry.definitions().is_empty());
}

#[test]
fn register_pack_tools_skips_bad_schema() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
    let tool = PackToolDef {
        name: "bad_schema".to_owned(),
        description: "Bad schema".to_owned(),
        command: "tools/test.sh".to_owned(),
        timeout: 5000,
        input_schema: Some(PackInputSchema {
            properties: IndexMap::from([(
                "field".to_owned(),
                PackPropertyDef {
                    property_type: "float".to_owned(),
                    description: "bad type".to_owned(),
                    enum_values: None,
                    default: None,
                },
            )]),
            required: vec![],
        }),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert_eq!(errors.len(), 1);
    assert!(registry.definitions().is_empty());
}

#[tokio::test]
async fn shell_executor_runs_script() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/echo.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/echo.sh")
            .canonicalize()
            .expect("canonicalize echo.sh path"),
        pack_root: dir.path().to_path_buf(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("echo_tool").expect("echo_tool is a valid tool name"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({"message": "hello"}),
    };
    let ctx = ToolContext {
        nous_id: aletheia_koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: aletheia_koina::id::SessionId::new(),
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("echo executor should succeed");
    assert!(
        !result.is_error,
        "unexpected error: {}",
        result.content.text_summary()
    );
    assert!(result.content.text_summary().contains("hello"));
}

#[tokio::test]
async fn shell_executor_nonzero_exit_is_error() {
    let dir = setup_pack_dir(&[("tools/fail.sh", "#!/bin/sh\nexit 1")]);
    make_executable(&dir, "tools/fail.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/fail.sh")
            .canonicalize()
            .expect("canonicalize fail.sh path"),
        pack_root: dir.path().to_path_buf(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("fail_tool").expect("fail_tool is a valid tool name"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = ToolContext {
        nous_id: aletheia_koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: aletheia_koina::id::SessionId::new(),
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("fail executor should return result");
    assert!(result.is_error);
}

#[test]
fn register_empty_packs() {
    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[], &mut registry);
    assert!(errors.is_empty());
    assert!(registry.definitions().is_empty());
}

#[test]
fn error_count_per_pack_not_cumulative() {
    let dir_a = setup_pack_dir(&[]);
    let pack_a = minimal_loaded_pack(
        &dir_a,
        vec![PackToolDef {
            name: "bad_tool_a".to_owned(),
            description: "Missing command".to_owned(),
            command: "tools/nonexistent.sh".to_owned(),
            timeout: 5000,
            input_schema: None,
        }],
    );

    let dir_b = setup_pack_dir(&[("tools/ok.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir_b, "tools/ok.sh");
    let pack_b = minimal_loaded_pack(
        &dir_b,
        vec![PackToolDef {
            name: "good_tool_b".to_owned(),
            description: "Good tool".to_owned(),
            command: "tools/ok.sh".to_owned(),
            timeout: 5000,
            input_schema: None,
        }],
    );

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack_a, pack_b], &mut registry);

    assert_eq!(
        errors.len(),
        1,
        "expected one error from pack A, got: {errors:?}"
    );
    assert_eq!(
        registry.definitions().len(),
        1,
        "pack B's tool should be registered"
    );
    assert_eq!(registry.definitions()[0].name.as_str(), "good_tool_b");
}

#[tokio::test]
async fn shell_metacharacters_in_arguments_passed_safely_via_stdin() {
    let dir = setup_pack_dir(&[("tools/cat.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/cat.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/cat.sh")
            .canonicalize()
            .expect("canonicalize cat.sh path"),
        pack_root: dir.path().to_path_buf(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("cat_tool").expect("cat_tool is a valid tool name"),
        tool_use_id: "toolu_meta".to_owned(),
        arguments: serde_json::json!({
            "cmd": "; rm -rf / && echo pwned | cat /etc/passwd $(whoami) `id`"
        }),
    };
    let ctx = ToolContext {
        nous_id: aletheia_koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: aletheia_koina::id::SessionId::new(),
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("metacharacter executor should succeed");
    let text = result.content.text_summary();
    assert!(
        text.contains("; rm -rf /"),
        "metacharacters must pass through uninterpreted as JSON stdin data"
    );
    assert!(
        text.contains("$(whoami)"),
        "subshell expansion must not execute"
    );
    assert!(text.contains("`id`"), "backtick expansion must not execute");
}

#[test]
fn validate_command_path_rejects_absolute_path_outside_root() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
    let result = validate_command_path(dir.path(), "/etc/passwd");
    let err = result.expect_err("absolute path outside root must be rejected");
    assert!(
        matches!(
            err,
            error::Error::ToolCommandNotFound { .. } | error::Error::ToolCommandEscape { .. }
        ),
        "absolute path outside pack root must be rejected"
    );
}

#[test]
fn validate_command_path_rejects_dotdot_traversal() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
    let result = validate_command_path(dir.path(), "tools/../../etc/passwd");
    let err = result.expect_err(".. traversal must be rejected");
    assert!(
        matches!(
            err,
            error::Error::ToolCommandNotFound { .. } | error::Error::ToolCommandEscape { .. }
        ),
        ".. traversal must be rejected"
    );
}

#[test]
fn validate_command_path_rejects_symlink_escape() {
    let dir = setup_pack_dir(&[("tools/legit.sh", "#!/bin/sh")]);
    let symlink_path = dir.path().join("tools/escape");
    std::os::unix::fs::symlink("/etc", &symlink_path).expect("create symlink for escape test");

    let result = validate_command_path(dir.path(), "tools/escape/passwd");
    let err = result.expect_err("symlink escape must be rejected");
    assert!(
        matches!(
            err,
            error::Error::ToolCommandNotFound { .. } | error::Error::ToolCommandEscape { .. }
        ),
        "symlink escape must be rejected"
    );
}

#[tokio::test]
async fn shell_executor_does_not_expand_env_vars_in_arguments() {
    let dir = setup_pack_dir(&[("tools/cat.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/cat.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/cat.sh")
            .canonicalize()
            .expect("canonicalize cat.sh path"),
        pack_root: dir.path().to_path_buf(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("cat_tool").expect("cat_tool is a valid tool name"),
        tool_use_id: "toolu_env".to_owned(),
        arguments: serde_json::json!({
            "path": "$HOME/.ssh/id_rsa"
        }),
    };
    let ctx = ToolContext {
        nous_id: aletheia_koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: aletheia_koina::id::SessionId::new(),
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("env var executor should succeed");
    let text = result.content.text_summary();
    assert!(
        text.contains("$HOME"),
        "environment variable must not be expanded: {text}"
    );
}

#[tokio::test]
async fn shell_executor_timeout_returns_error() {
    let dir = setup_pack_dir(&[("tools/slow.sh", "#!/bin/sh\nsleep 60")]);
    make_executable(&dir, "tools/slow.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/slow.sh")
            .canonicalize()
            .expect("canonicalize slow.sh path"),
        pack_root: dir.path().to_path_buf(),
        timeout_ms: 100,
    };

    let input = ToolInput {
        name: ToolName::new("slow_tool").expect("slow_tool is a valid tool name"),
        tool_use_id: "toolu_slow".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = ToolContext {
        nous_id: aletheia_koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: aletheia_koina::id::SessionId::new(),
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("timeout executor should return result");
    assert!(result.is_error);
    assert!(
        result.content.text_summary().contains("timed out"),
        "timeout error expected"
    );
}

#[tokio::test]
async fn shell_executor_truncates_at_char_boundary() {
    // NOTE: U+2026 (3 bytes: 0xE2 0x80 0xA6) is placed straddling MAX_OUTPUT_BYTES
    // so that naive truncate() would panic on the invalid byte boundary
    let ellipsis = "\u{2026}"; // NOTE: 3 bytes: 0xE2 0x80 0xA6
    let fill_len = MAX_OUTPUT_BYTES - 1;
    let fill: String = "a".repeat(fill_len);
    let full_output = format!("{fill}{ellipsis}extra");

    let script_content = format!("#!/bin/sh\nprintf '%s' '{full_output}'");
    let dir = setup_pack_dir(&[("tools/multibyte.sh", &script_content)]);
    make_executable(&dir, "tools/multibyte.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/multibyte.sh")
            .canonicalize()
            .expect("canonicalize multibyte.sh path"),
        pack_root: dir.path().to_path_buf(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("mb_tool").expect("mb_tool is a valid tool name"),
        tool_use_id: "toolu_mb".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = ToolContext {
        nous_id: aletheia_koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: aletheia_koina::id::SessionId::new(),
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("truncation executor should succeed");
    let text = result.content.text_summary();
    assert!(text.is_char_boundary(0), "result must be valid UTF-8");
    assert!(
        text.contains("[output truncated]"),
        "truncation marker expected"
    );
    assert!(text.len() <= MAX_OUTPUT_BYTES + "[output truncated]".len() + 2);
}
