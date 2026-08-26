//! Tests for thesauros tools.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]

use std::fs;
#[cfg(unix)]
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

#[cfg(unix)]
fn make_executable(dir: &TempDir, path: &str) {
    let full = dir.path().join(path);
    let mut perms = fs::metadata(&full)
        .expect("get file metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&full, perms).expect("set executable permissions");
}

#[cfg(unix)]
fn test_runner() -> SubprocessRunner {
    SubprocessRunner::new(organon::sandbox::SandboxConfig {
        enabled: false,
        nproc_limit: 4096,
        ..organon::sandbox::SandboxConfig::default()
    })
}

/// Build a `ShellToolExecutor` for a script already written+chmod'd under
/// `dir`, capturing its `FileIdentity` the same way registration does
/// (#5213) so the swap-detection check in `execute()` doesn't fire on
/// freshly-built test fixtures.
#[cfg(unix)]
fn test_executor(dir: &TempDir, script_relpath: &str, timeout_ms: u64) -> ShellToolExecutor {
    let command_path = dir
        .path()
        .join(script_relpath)
        .canonicalize()
        .expect("canonicalize test script path");
    let expected_identity =
        FileIdentity::of(&command_path).expect("captured identity for test executor");
    ShellToolExecutor {
        command_path,
        pack_root: dir.path().to_path_buf(),
        runner: test_runner(),
        timeout_ms,
        expected_identity,
        env_vars: Vec::new(),
        write_paths: Vec::new(),
        deny_egress: false,
    }
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
struct EnvCleanup;

#[cfg(unix)]
impl Drop for EnvCleanup {
    #[expect(unsafe_code, reason = "test serializes process environment mutation")]
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("ALETHEIA_TOKEN");
        }
    }
}

#[cfg(unix)]
#[test]
fn validate_command_path_success() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/test.sh");
    let result = validate_command_path(dir.path(), "tools/test.sh");
    assert!(result.is_ok());
}

// SECURITY(#5213): registration must reject a non-executable file, a
// directory, and an absolute/`..`-shaped command string syntactically
// before any filesystem access.
#[cfg(unix)]
#[test]
fn validate_command_path_rejects_non_executable_file() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh\necho ok")]);
    // WHY: setup_pack_dir does not chmod +x — this is the not-executable case.
    let result = validate_command_path(dir.path(), "tools/test.sh");
    assert!(
        matches!(
            result.expect_err("non-executable file must be rejected"),
            error::Error::ToolCommandNotExecutable { .. }
        ),
        "a file lacking the executable bit must fail registration, not first invocation"
    );
}

#[test]
fn validate_command_path_rejects_directory() {
    let dir = setup_pack_dir(&[("tools/subdir/placeholder", "")]);
    let result = validate_command_path(dir.path(), "tools/subdir");
    assert!(
        matches!(
            result.expect_err("a directory must be rejected as a command path"),
            error::Error::ToolCommandNotExecutable { .. }
        ),
        "a directory is not a regular executable file"
    );
}

#[test]
fn validate_command_path_rejects_absolute_command_string_syntactically() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
    make_executable(&dir, "tools/test.sh");
    // WHY: `/etc/passwd` need not even exist — the syntactic pre-check must
    // reject it before any canonicalize/stat call touches the filesystem.
    let result = validate_command_path(dir.path(), "/definitely/does/not/exist");
    assert!(matches!(
        result.expect_err("absolute command string must be rejected"),
        error::Error::ToolCommandEscape { .. }
    ));
}

#[test]
fn validate_command_path_rejects_dotdot_command_string_syntactically() {
    let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
    make_executable(&dir, "tools/test.sh");
    let result = validate_command_path(dir.path(), "tools/../../definitely-does-not-exist");
    assert!(matches!(
        result.expect_err(".. in the command string must be rejected"),
        error::Error::ToolCommandEscape { .. }
    ));
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

#[cfg(unix)]
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
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
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

#[cfg(unix)]
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
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
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

#[cfg(unix)]
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
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].error.to_string().contains("unknown tool group"),
        "unexpected error: {}",
        errors[0].error
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
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
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
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools(&[pack], &mut registry);
    assert_eq!(errors.len(), 1);
    assert!(registry.definitions().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_runs_script() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/echo.sh");

    let executor = test_executor(&dir, "tools/echo.sh", 5000);

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

// SECURITY(#5213): a file swapped in at the registered path after
// registration must be refused at execution, not silently run under the
// tool's original, reviewed name.
#[cfg(unix)]
#[tokio::test]
async fn shell_executor_refuses_a_swapped_command_file() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/echo.sh");

    // WHY: identity is captured here, before the swap below — mirrors what
    // `register_pack_tools` does at pack-load time.
    let executor = test_executor(&dir, "tools/echo.sh", 5000);

    // Swap the file at the same path: different content (different size).
    // WHY: File + write_all (not `std::fs::write`, disallowed in this crate
    // per `clippy.toml` — use tokio::fs or abstract behind a trait) mirrors
    // `setup_pack_dir`'s own approach above.
    let swapped_path = dir.path().join("tools/echo.sh");
    let swapped_content = "#!/bin/sh\necho swapped-in-after-registration";
    let swap_file = std::fs::File::create(&swapped_path).expect("open script for swap");
    std::io::Write::write_all(&mut &swap_file, swapped_content.as_bytes())
        .expect("overwrite script content to simulate a post-registration swap");
    swap_file.sync_all().expect("sync swapped script");
    drop(swap_file);
    make_executable(&dir, "tools/echo.sh");

    let input = ToolInput {
        name: ToolName::new("echo_tool").expect("echo_tool is a valid tool name"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({"message": "hello"}),
    };
    let ctx = test_ctx(&dir);

    let result = executor
        .execute(&input, &ctx)
        .await
        .expect("execute must not itself error — it reports the mismatch as a tool error");
    assert!(
        result.is_error,
        "a swapped command file must be refused, not executed"
    );
    assert!(
        result
            .content
            .text_summary()
            .contains("changed since registration"),
        "error must explain why: {}",
        result.content.text_summary()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_nonzero_exit_is_error() {
    let dir = setup_pack_dir(&[("tools/fail.sh", "#!/bin/sh\nexit 1")]);
    make_executable(&dir, "tools/fail.sh");

    let executor = test_executor(&dir, "tools/fail.sh", 5000);

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

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_keeps_stderr_out_of_model_visible_diagnostics() {
    // SECURITY(#5212): stderr is arbitrary subprocess output. ToolDiagnostics is rendered
    // into the model turn, so neither credential-shaped nor ordinary private text may cross
    // that boundary. Operators still receive metadata through the structured warning log.
    let dir = setup_pack_dir(&[(
        "tools/fail.sh",
        // kanon:ignore SECURITY/hardcoded-openai-api-key + gitleaks:allow + trufflehog:ignore -- synthetic key shape used by boundary test; not a real credential
        "#!/bin/sh\necho stdout-only\necho 'SECRET_TOKEN auth failed for sk-ant-api03-abcdef123456_789XYZ at /home/alice/private' >&2\nexit 1",
    )]);
    make_executable(&dir, "tools/fail.sh");

    let executor = test_executor(&dir, "tools/fail.sh", 5000);
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
    assert!(!text.contains("auth failed"));
    assert!(!text.contains("SECRET_TOKEN"));
    assert!(!text.contains("/home/alice/private"));

    let diagnostics = result.diagnostics.expect("diagnostics should be present");
    assert!(diagnostics.stderr.is_none(), "stderr must remain operator-only");
    assert_eq!(diagnostics.exit_code, Some(1));
}

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_stderr_only_failure_uses_generic_model_message() {
    let dir = setup_pack_dir(&[(
        "tools/stderr_only.sh",
        "#!/bin/sh\necho 'relation \"sales\" does not exist' >&2\nexit 2",
    )]);
    make_executable(&dir, "tools/stderr_only.sh");

    let executor = test_executor(&dir, "tools/stderr_only.sh", 5000);
    let input = ToolInput {
        name: ToolName::new("stderr_only_tool").expect("valid tool name"),
        tool_use_id: "toolu_stderr_only".to_owned(),
        arguments: serde_json::json!({}),
    };

    let result = executor
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("executor should return result");
    assert!(result.is_error);
    assert!(
        result.content.text_summary().contains("status 2"),
        "empty stdout should yield the generic status message: {}",
        result.content.text_summary()
    );
    let diagnostics = result.diagnostics.expect("diagnostics should be present");
    assert!(diagnostics.stderr.is_none(), "stderr must remain operator-only");
}

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_spawn_failure_is_an_error_result() {
    // A syntactically valid script with a nonexistent interpreter fails at
    // spawn (not at registration: the file itself exists and is executable).
    let dir = setup_pack_dir(&[(
        "tools/badinterp.sh",
        "#!/definitely/not/an/interp\necho unreachable",
    )]);
    make_executable(&dir, "tools/badinterp.sh");

    let executor = test_executor(&dir, "tools/badinterp.sh", 5000);
    let input = ToolInput {
        name: ToolName::new("badinterp_tool").expect("valid tool name"),
        tool_use_id: "toolu_badinterp".to_owned(),
        arguments: serde_json::json!({}),
    };

    let result = executor
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("execute maps spawn failure to an error result");
    assert!(result.is_error);
    assert!(
        result
            .content
            .text_summary()
            .contains("process could not start"),
        "spawn failure should be identifiable: {}",
        result.content.text_summary()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_permission_denied_after_registration_is_an_error() {
    // chmod -x after registration: mode is not part of FileIdentity, so the
    // swap check passes and the exec itself fails with permission denied.
    let dir = setup_pack_dir(&[("tools/denied.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/denied.sh");
    let executor = test_executor(&dir, "tools/denied.sh", 5000);

    let mut perms = fs::metadata(dir.path().join("tools/denied.sh"))
        .expect("metadata")
        .permissions();
    perms.set_mode(0o644);
    fs::set_permissions(dir.path().join("tools/denied.sh"), perms).expect("chmod -x");

    let input = ToolInput {
        name: ToolName::new("denied_tool").expect("valid tool name"),
        tool_use_id: "toolu_denied".to_owned(),
        arguments: serde_json::json!({}),
    };

    let result = executor
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("execute maps EACCES to an error result");
    assert!(result.is_error);
    let text = result.content.text_summary();
    assert!(
        text.contains("process could not start") || text.contains("changed since registration"),
        "permission-denied must surface as an execution failure: {text}"
    );
}

#[test]
fn subprocess_failure_marks_sandbox_setup_as_violation() {
    let error = SubprocessError::SandboxSetup(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "seccomp install refused",
    ));
    let result = subprocess_failure(&error);
    assert!(result.is_error);
    let diagnostics = result.diagnostics.expect("diagnostics should be present");
    assert_eq!(diagnostics.sandbox_violations.len(), 1);
    assert!(
        diagnostics.sandbox_violations[0] == "sandbox_setup_failed",
        "violation should carry only a stable category: {:?}",
        diagnostics.sandbox_violations
    );
    assert!(
        !result.content.text_summary().contains("seccomp install refused"),
        "operator error detail must not enter the model-visible result"
    );
}

#[test]
fn subprocess_failure_leaves_spawn_without_violations() {
    let error = SubprocessError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no such file",
    ));
    let result = subprocess_failure(&error);
    assert!(result.is_error);
    let diagnostics = result.diagnostics.expect("diagnostics should be present");
    assert!(diagnostics.sandbox_violations.is_empty());
}

#[cfg(unix)]
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

    let executor = test_executor(&dir, "tools/env.sh", 5000);
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

#[cfg(unix)]
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
            env: Vec::new(),
            write_paths: Vec::new(),
            egress: None,
            platforms: Vec::new(),
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
            env: Vec::new(),
            write_paths: Vec::new(),
            egress: None,
            platforms: Vec::new(),
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

#[cfg(unix)]
#[test]
fn duplicate_tool_name_fails_second_pack_and_degrades_its_health() {
    // WHY(#5208): PACKS.md used to claim duplicate tool names are "rejected at
    // startup". The actual policy is first-registration-wins: the duplicate is
    // skipped, and the failure is recorded so the second pack reports Degraded.
    let dir_a = setup_pack_dir(&[("tools/dup.sh", "#!/bin/sh\necho a")]);
    make_executable(&dir_a, "tools/dup.sh");
    let dir_b = setup_pack_dir(&[("tools/dup.sh", "#!/bin/sh\necho b")]);
    make_executable(&dir_b, "tools/dup.sh");

    let tool = |name: &str| PackToolDef {
        name: name.to_owned(),
        description: "Duplicate tool".to_owned(),
        command: "tools/dup.sh".to_owned(),
        timeout: 5000,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
    };
    let pack_a = minimal_loaded_pack(&dir_a, vec![tool("dup_tool")]);
    let mut pack_b = minimal_loaded_pack(&dir_b, vec![tool("dup_tool")]);
    pack_b.manifest.name = "pack-b".to_owned();

    let mut registry = ToolRegistry::new();
    let failures = register_pack_tools(&[pack_a, pack_b], &mut registry);
    assert_eq!(failures.len(), 1, "second registration must fail");
    assert_eq!(failures[0].pack_name, "pack-b");
    assert_eq!(failures[0].tool_name, "dup_tool");
    assert_eq!(registry.definitions().len(), 1, "first registration wins");

    let mut report = crate::health::PackReport::default();
    report.packs.push(crate::health::PackHealth::active(
        "test-pack".to_owned(),
        dir_a.path().to_path_buf(),
    ));
    report.packs.push(crate::health::PackHealth::active(
        "pack-b".to_owned(),
        dir_b.path().to_path_buf(),
    ));
    report.record_tool_failures(&failures);

    assert_eq!(report.packs[0].status, crate::health::PackStatus::Active);
    assert_eq!(report.packs[1].status, crate::health::PackStatus::Degraded);
    assert!(
        report.packs[1].issues[0].message.contains("dup_tool"),
        "health issue should name the failed tool: {}",
        report.packs[1].issues[0].message
    );
}

#[cfg(unix)]
#[tokio::test]
async fn shell_metacharacters_in_arguments_passed_safely_via_stdin() {
    let dir = setup_pack_dir(&[("tools/cat.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/cat.sh");

    let executor = test_executor(&dir, "tools/cat.sh", 5000);

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

#[cfg(unix)]
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

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_does_not_expand_env_vars_in_arguments() {
    let dir = setup_pack_dir(&[("tools/cat.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/cat.sh");

    let executor = test_executor(&dir, "tools/cat.sh", 5000);

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

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_timeout_returns_error() {
    let dir = setup_pack_dir(&[("tools/slow.sh", "#!/bin/sh\nsleep 60")]);
    make_executable(&dir, "tools/slow.sh");

    let executor = test_executor(&dir, "tools/slow.sh", 100);

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

#[cfg(unix)]
#[tokio::test]
async fn shell_executor_records_nonzero_duration() {
    let dir = setup_pack_dir(&[("tools/sleep.sh", "#!/bin/sh\nsleep 0.05")]);
    make_executable(&dir, "tools/sleep.sh");

    let executor = test_executor(&dir, "tools/sleep.sh", 5000);

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

#[cfg(unix)]
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

    let executor = test_executor(&dir, "tools/multibyte.sh", 5000);

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

#[cfg(unix)]
fn unsandboxed_test_config() -> organon::sandbox::SandboxConfig {
    organon::sandbox::SandboxConfig {
        enabled: false,
        nproc_limit: 4096,
        ..organon::sandbox::SandboxConfig::default()
    }
}

#[cfg(unix)]
#[test]
fn register_with_limits_rejects_zero_timeout() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/echo.sh");

    let tool = PackToolDef {
        name: "zero_timeout_tool".to_owned(),
        description: "Zero timeout".to_owned(),
        command: "tools/echo.sh".to_owned(),
        timeout: 0,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools_with_sandbox_and_limits(
        &[pack],
        &mut registry,
        unsandboxed_test_config(),
        60,
    );

    assert_eq!(errors.len(), 1, "errors: {errors:?}");
    assert!(
        matches!(errors[0].error, error::Error::InvalidToolTimeout { .. }),
        "expected InvalidToolTimeout, got: {}",
        errors[0].error
    );
    assert!(registry.definitions().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn register_with_limits_clamps_timeout_below_floor() {
    let dir = setup_pack_dir(&[("tools/sleep.sh", "#!/bin/sh\nsleep 0.4")]);
    make_executable(&dir, "tools/sleep.sh");

    let tool = PackToolDef {
        name: "below_floor_tool".to_owned(),
        description: "Below clamp floor".to_owned(),
        command: "tools/sleep.sh".to_owned(),
        timeout: 1,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools_with_sandbox_and_limits(
        &[pack],
        &mut registry,
        unsandboxed_test_config(),
        60,
    );
    assert!(errors.is_empty(), "errors: {errors:?}");

    let input = ToolInput {
        name: ToolName::new("below_floor_tool").expect("below_floor_tool is a valid tool name"),
        tool_use_id: "toolu_floor".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = registry
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("execute should return a result");
    assert!(
        !result.is_error,
        "a 1ms declared timeout must be clamped up to the 1_000ms floor \
         so a 0.4s script completes, got: {}",
        result.content.text_summary()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn register_with_limits_clamps_timeout_above_ceiling() {
    let dir = setup_pack_dir(&[("tools/sleep.sh", "#!/bin/sh\nsleep 3")]);
    make_executable(&dir, "tools/sleep.sh");

    let tool = PackToolDef {
        name: "above_ceiling_tool".to_owned(),
        description: "Above deployment ceiling".to_owned(),
        command: "tools/sleep.sh".to_owned(),
        timeout: 999_999_000,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools_with_sandbox_and_limits(
        &[pack],
        &mut registry,
        unsandboxed_test_config(),
        1,
    );
    assert!(errors.is_empty(), "errors: {errors:?}");

    let input = ToolInput {
        name: ToolName::new("above_ceiling_tool").expect("above_ceiling_tool is a valid tool name"),
        tool_use_id: "toolu_ceiling".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = registry
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("execute should return a result");
    assert!(
        result.is_error && result.content.text_summary().contains("timed out"),
        "a huge declared timeout must be clamped down to the 1s deployment ceiling \
         so a 3s script times out, got: {}",
        result.content.text_summary()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn register_with_limits_leaves_in_range_timeout_unclamped() {
    let dir = setup_pack_dir(&[("tools/sleep.sh", "#!/bin/sh\nsleep 0.05")]);
    make_executable(&dir, "tools/sleep.sh");

    let tool = PackToolDef {
        name: "in_range_tool".to_owned(),
        description: "In-range timeout".to_owned(),
        command: "tools/sleep.sh".to_owned(),
        timeout: 5_000,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: Vec::new(),
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let errors = register_pack_tools_with_sandbox_and_limits(
        &[pack],
        &mut registry,
        unsandboxed_test_config(),
        60,
    );
    assert!(errors.is_empty(), "errors: {errors:?}");

    let input = ToolInput {
        name: ToolName::new("in_range_tool").expect("in_range_tool is a valid tool name"),
        tool_use_id: "toolu_range".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = registry
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("execute should return a result");
    assert!(
        !result.is_error,
        "a declared timeout already within [1_000ms, ceiling] must pass through unclamped, got: {}",
        result.content.text_summary()
    );
}

// --- #5214: per-tool environment / write-path / egress contract ---

#[cfg(unix)]
fn tool_def_with_policy(
    name: &str,
    command: &str,
    env: Vec<String>,
    write_paths: Vec<String>,
    egress: Option<String>,
) -> PackToolDef {
    PackToolDef {
        name: name.to_owned(),
        description: "Policy tool".to_owned(),
        command: command.to_owned(),
        timeout: 5000,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env,
        write_paths,
        egress,
        platforms: Vec::new(),
    }
}

#[cfg(unix)]
#[test]
#[expect(unsafe_code, reason = "test serializes process environment mutation")]
fn register_rejects_declared_env_var_missing_from_daemon_environment() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("THESAUROS_TEST_MISSING_ENV");
    }

    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/echo.sh");
    let tool = tool_def_with_policy(
        "needs_env",
        "tools/echo.sh",
        vec!["THESAUROS_TEST_MISSING_ENV".to_owned()],
        Vec::new(),
        None,
    );
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let failures = register_pack_tools(&[pack], &mut registry);
    assert_eq!(failures.len(), 1, "declared-but-absent env must fail");
    assert!(
        failures[0]
            .error
            .to_string()
            .contains("THESAUROS_TEST_MISSING_ENV"),
        "failure must name the missing variable: {}",
        failures[0].error
    );
    assert!(registry.definitions().is_empty());
}

#[cfg(unix)]
#[test]
#[expect(unsafe_code, reason = "test serializes process environment mutation")]
fn declared_env_var_is_injected_into_subprocess() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::set_var("THESAUROS_TEST_DECLARED_ENV", "declared-value");
    }

    let dir = setup_pack_dir(&[(
        "tools/env.sh",
        "#!/bin/sh\nprintf '%s' \"${THESAUROS_TEST_DECLARED_ENV-unset}\"",
    )]);
    make_executable(&dir, "tools/env.sh");

    let mut executor = test_executor(&dir, "tools/env.sh", 5000);
    executor
        .env_vars
        .push("THESAUROS_TEST_DECLARED_ENV".to_owned());

    let input = ToolInput {
        name: ToolName::new("env_tool").expect("valid tool name"),
        tool_use_id: "toolu_declared_env".to_owned(),
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

    unsafe {
        std::env::remove_var("THESAUROS_TEST_DECLARED_ENV");
    }

    assert!(!result.is_error);
    assert_eq!(result.content.text_summary(), "declared-value");
}

#[cfg(unix)]
#[test]
fn register_rejects_write_path_escaping_pack_root() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/echo.sh");
    let tool = tool_def_with_policy(
        "writes_outside",
        "tools/echo.sh",
        Vec::new(),
        vec!["../outside".to_owned()],
        None,
    );
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let failures = register_pack_tools(&[pack], &mut registry);
    assert_eq!(failures.len(), 1, "escaping write path must fail");
    assert!(
        failures[0].error.to_string().contains("write path"),
        "failure must name the write path contract: {}",
        failures[0].error
    );
    assert!(registry.definitions().is_empty());
}

#[cfg(unix)]
#[test]
fn register_rejects_unknown_egress_intent() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/echo.sh");
    let tool = tool_def_with_policy(
        "bad_egress",
        "tools/echo.sh",
        Vec::new(),
        Vec::new(),
        Some("everything".to_owned()),
    );
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let failures = register_pack_tools(&[pack], &mut registry);
    assert_eq!(failures.len(), 1, "unknown egress intent must fail");
    assert!(
        failures[0]
            .error
            .to_string()
            .contains("unknown egress intent"),
        "unexpected failure: {}",
        failures[0].error
    );
    assert!(registry.definitions().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn register_accepts_egress_none_and_inherit() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\ncat")]);
    make_executable(&dir, "tools/echo.sh");
    let tools = vec![
        tool_def_with_policy(
            "no_network",
            "tools/echo.sh",
            Vec::new(),
            Vec::new(),
            Some("none".to_owned()),
        ),
        tool_def_with_policy(
            "inherits_policy",
            "tools/echo.sh",
            Vec::new(),
            Vec::new(),
            Some("inherit".to_owned()),
        ),
    ];
    let pack = minimal_loaded_pack(&dir, tools);

    let mut registry = ToolRegistry::new();
    // NOTE: sandbox disabled in tests, so the egress = "none" tool logs an
    // unenforced-intent warning but still registers.
    let failures = register_pack_tools(&[pack], &mut registry);
    assert!(failures.is_empty(), "failures: {failures:?}");
    assert_eq!(registry.definitions().len(), 2);

    let input = ToolInput {
        name: ToolName::new("no_network").expect("valid tool name"),
        tool_use_id: "toolu_egress".to_owned(),
        arguments: serde_json::json!({"ok": true}),
    };
    let result = registry
        .execute(&input, &test_ctx(&dir))
        .await
        .expect("execute should return a result");
    assert!(
        !result.is_error,
        "declared egress intent must not break execution: {}",
        result.content.text_summary()
    );
}

// --- #5215: explicit platform support ---

#[test]
fn register_skips_tool_for_unsupported_platform() {
    // WHY: a tool whose platforms exclude the current host must be skipped at
    // registration with a visible failure (pack health: degraded), not
    // registered to fail at first exec. Runs on any host by picking a
    // platform the host is not; the platform check runs before any
    // filesystem validation, so no executable fixture is needed.
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
    let foreign = if cfg!(windows) { "linux" } else { "windows" };
    let tool = PackToolDef {
        name: "foreign_tool".to_owned(),
        description: "Wrong-platform tool".to_owned(),
        command: "tools/echo.sh".to_owned(),
        timeout: 5000,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: vec![foreign.to_owned()],
    };
    let dir_root = dir.path().to_path_buf();
    let pack = LoadedPack {
        manifest: PackManifest {
            name: "test-pack".to_owned(),
            version: "1.0".to_owned(),
            description: None,
            context: vec![],
            tools: vec![tool],
            overlays: std::collections::HashMap::new(),
        },
        sections: vec![],
        root: dir_root,
    };

    let mut registry = ToolRegistry::new();
    let failures = register_pack_tools(&[pack], &mut registry);
    assert_eq!(failures.len(), 1, "unsupported-platform tool must fail");
    assert!(
        failures[0].error.to_string().contains("tool skipped"),
        "failure must name the platform mismatch: {}",
        failures[0].error
    );
    assert!(registry.definitions().is_empty());
}

#[cfg(unix)]
#[test]
fn register_accepts_tool_covering_current_host() {
    let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
    make_executable(&dir, "tools/echo.sh");
    let tool = PackToolDef {
        name: "unix_tool".to_owned(),
        description: "Unix tool".to_owned(),
        command: "tools/echo.sh".to_owned(),
        timeout: 5000,
        input_schema: None,
        groups: Vec::new(),
        tags: Vec::new(),
        reversibility: None,
        env: Vec::new(),
        write_paths: Vec::new(),
        egress: None,
        platforms: vec!["unix".to_owned()],
    };
    let pack = minimal_loaded_pack(&dir, vec![tool]);

    let mut registry = ToolRegistry::new();
    let failures = register_pack_tools(&[pack], &mut registry);
    assert!(failures.is_empty(), "failures: {failures:?}");
    assert_eq!(registry.definitions().len(), 1);
}
