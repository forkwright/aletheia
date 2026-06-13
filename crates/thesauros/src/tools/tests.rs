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

fn test_runner() -> SubprocessRunner {
    SubprocessRunner::new(organon::sandbox::SandboxConfig {
        enabled: false,
        nproc_limit: 4096,
        ..organon::sandbox::SandboxConfig::default()
    })
}

fn test_ctx(dir: &TempDir) -> ToolContext {
    ToolContext {
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
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

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvCleanup;

impl Drop for EnvCleanup {
    #[expect(unsafe_code, reason = "test serializes process environment mutation")]
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("ALETHEIA_TOKEN");
        }
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
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(registry.definitions().len(), 1);
    assert_eq!(registry.definitions()[0].name.as_str(), "echo_tool");
    assert_eq!(registry.definitions()[0].category, ToolCategory::Domain);
    assert_eq!(registry.definitions()[0].groups, vec![ToolGroupId::Command]);
    assert_eq!(registry.definitions()[0].tags, vec![ToolTag::Execute]);
    assert_eq!(
        registry.definitions()[0].reversibility,
        Reversibility::Irreversible
    );
}

#[test]
fn register_pack_tools_applies_declared_capability_metadata() {
    let dir = setup_pack_dir(&[("tools/read.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/read.sh");

    let tool = PackToolDef {
        name: "read_tool".to_owned(),
        description: "Read tool".to_owned(),
        command: "tools/read.sh".to_owned(),
        timeout: 5000,
        input_schema: None,
        groups: vec!["read".to_owned()],
        tags: vec!["recon".to_owned(), "fetch".to_owned()],
        reversibility: Some("fully_reversible".to_owned()),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert!(errors.is_empty(), "errors: {errors:?}");
    let def = &registry.definitions()[0];
    assert_eq!(def.groups, vec![ToolGroupId::Read]);
    assert_eq!(def.tags, vec![ToolTag::Recon, ToolTag::Fetch]);
    assert_eq!(def.reversibility, Reversibility::FullyReversible);
}

#[test]
fn register_pack_tools_rejects_unknown_capability_metadata() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
    make_executable(&dir, "tools/test.sh");

    let tool = PackToolDef {
        name: "bad_group".to_owned(),
        description: "Bad group".to_owned(),
        command: "tools/test.sh".to_owned(),
        timeout: 5000,
        input_schema: None,
        groups: vec!["superuser".to_owned()],
        tags: Vec::new(),
        reversibility: None,
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].to_string().contains("unknown tool group"),
        "unexpected error: {}",
        errors[0]
    );
    assert!(registry.definitions().is_empty());
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
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
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
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
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
        runner: test_runner(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("echo_tool").expect("echo_tool is a valid tool name"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({"message": "hello"}),
    };
    let ctx = ToolContext {
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
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
        runner: test_runner(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("fail_tool").expect("fail_tool is a valid tool name"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = ToolContext {
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("fail executor should return result");
    assert!(result.is_error);
}

#[tokio::test]
async fn shell_executor_keeps_stderr_out_of_llm_visible_result() {
    let dir = setup_pack_dir(&[(
        "tools/fail.sh",
        "#!/bin/sh\necho stdout-only\necho 'SECRET_TOKEN /home/alice/private' >&2\nexit 1",
    )]);
    make_executable(&dir, "tools/fail.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/fail.sh")
            .canonicalize()
            .expect("canonicalize fail.sh path"),
        pack_root: dir.path().to_path_buf(),
        runner: test_runner(),
        timeout_ms: 5000,
    };
    let input = ToolInput {
        name: ToolName::new("fail_tool").expect("fail_tool is a valid tool name"),
        tool_use_id: "toolu_stderr".to_owned(),
        arguments: serde_json::json!({}),
    };

    let result = executor
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("executor should return result");
    assert!(result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains("stdout-only"));
    assert!(!text.contains("SECRET_TOKEN"));
    assert!(!text.contains("/home/alice/private"));
    let diagnostics = result.diagnostics.expect("diagnostics should be present");
    assert!(diagnostics.stderr.is_none());
}

#[test]
#[expect(unsafe_code, reason = "test serializes process environment mutation")]
fn shell_executor_clears_sensitive_parent_environment() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::set_var("ALETHEIA_TOKEN", "SECRET_TOKEN");
    }
    let _cleanup = EnvCleanup;

    let dir = setup_pack_dir(&[(
        "tools/env.sh",
        "#!/bin/sh\nprintf '%s' \"${ALETHEIA_TOKEN-unset}\"",
    )]);
    make_executable(&dir, "tools/env.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/env.sh")
            .canonicalize()
            .expect("canonicalize env.sh path"),
        pack_root: dir.path().to_path_buf(),
        runner: test_runner(),
        timeout_ms: 5000,
    };
    let input = ToolInput {
        name: ToolName::new("env_tool").expect("env_tool is a valid tool name"),
        tool_use_id: "toolu_env_strip".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = test_ctx(&dir);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let result = runtime
        .block_on(executor.execute(&input, &ctx))
        .expect("executor should return result");
    assert!(!result.is_error);
    assert_eq!(result.content.text_summary(), "unset");
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
            groups: Vec::new(),
            tags: Vec::new(),
            reversibility: None,
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
            groups: Vec::new(),
            tags: Vec::new(),
            reversibility: None,
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
        runner: test_runner(),
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
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
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
        runner: test_runner(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("cat_tool").expect("cat_tool is a valid tool name"),
        tool_use_id: "toolu_env".to_owned(),
        arguments: serde_json::json!({
            "path": "$HOME/.ssh/id_rsa" // pii-allow: SSH filename literal, no key material
        }),
    };
    let ctx = ToolContext {
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
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
        runner: test_runner(),
        timeout_ms: 100,
    };

    let input = ToolInput {
        name: ToolName::new("slow_tool").expect("slow_tool is a valid tool name"),
        tool_use_id: "toolu_slow".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = ToolContext {
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
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
async fn shell_executor_records_nonzero_duration() {
    let dir = setup_pack_dir(&[("tools/sleep.sh", "#!/bin/sh\nsleep 0.05")]);
    make_executable(&dir, "tools/sleep.sh");

    let executor = ShellToolExecutor {
        command_path: dir
            .path()
            .join("tools/sleep.sh")
            .canonicalize()
            .expect("canonicalize sleep.sh path"),
        pack_root: dir.path().to_path_buf(),
        runner: test_runner(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("sleep_tool").expect("sleep_tool is a valid tool name"),
        tool_use_id: "toolu_dur".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = ToolContext {
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
    };

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("duration executor should succeed");
    let diagnostics = result.diagnostics.expect("diagnostics should be present");
    assert!(
        diagnostics.duration_ms >= 10,
        "expected duration >= 10 ms, got {} ms",
        diagnostics.duration_ms
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
        runner: test_runner(),
        timeout_ms: 5000,
    };

    let input = ToolInput {
        name: ToolName::new("mb_tool").expect("mb_tool is a valid tool name"),
        tool_use_id: "toolu_mb".to_owned(),
        arguments: serde_json::json!({}),
    };
    let ctx = ToolContext {
        nous_id: koina::id::NousId::new("test").expect("test is a valid nous id"),
        session_id: koina::id::SessionId::new(),
        turn_number: 0,
        workspace: dir.path().to_path_buf(),
        allowed_roots: vec![],
        services: None,
        active_tools: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
        tool_config: std::sync::Arc::new(taxis::config::ToolLimitsConfig::default()),
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
