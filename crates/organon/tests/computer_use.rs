//! Integration tests for computer use tool: sandbox enforcement, action
//! parsing, and result extraction.
//!
//! These tests exercise the tool registration and sandbox policy construction
//! without requiring a display server (no X11/Wayland). Screen capture and
//! action dispatch are tested via unit tests in the module; this file focuses
//! on integration points.

#![cfg(feature = "computer-use")]

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use aletheia_koina::id::{NousId, SessionId, ToolName};
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::sandbox::{SandboxConfig, SandboxEnforcement};
use aletheia_organon::types::{ToolContext, ToolInput};

#[expect(clippy::expect_used, reason = "test assertions")]
fn test_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

/// Registration must succeed and produce a tool definition with the correct
/// name and schema properties.
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn registers_computer_use_tool() {
    let mut registry = ToolRegistry::new();
    aletheia_organon::builtins::computer_use::register(&mut registry, &SandboxConfig::default())
        .expect("registration should succeed");

    let name = ToolName::new("computer_use").expect("valid");
    let def = registry.get_def(&name).expect("tool should be registered");
    assert_eq!(def.name.as_str(), "computer_use");
    assert!(
        !def.auto_activate,
        "computer_use should require explicit activation"
    );

    // Verify all action-related properties exist in the schema.
    let schema = def.input_schema.to_json_schema();
    let props = schema
        .get("properties")
        .expect("schema should have properties");
    assert!(props.get("action").is_some(), "schema should have action");
    assert!(props.get("x").is_some(), "schema should have x");
    assert!(props.get("y").is_some(), "schema should have y");
    assert!(props.get("text").is_some(), "schema should have text");
    assert!(props.get("combo").is_some(), "schema should have combo");
    assert!(props.get("delta").is_some(), "schema should have delta");
    assert!(props.get("button").is_some(), "schema should have button");
}

/// The tool should return an error result (not panic) for unknown actions.
#[tokio::test]
#[expect(clippy::expect_used, reason = "test assertions")]
async fn unknown_action_returns_error() {
    let mut registry = ToolRegistry::new();
    aletheia_organon::builtins::computer_use::register(&mut registry, &SandboxConfig::default())
        .expect("register");

    let input = ToolInput {
        name: ToolName::new("computer_use").expect("valid"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({"action": "fly"}),
    };

    let result = registry
        .execute(&input, &test_ctx())
        .await
        .expect("execute should not error");

    assert!(
        result.is_error,
        "unknown action should produce error result"
    );
    assert!(
        result.content.text_summary().contains("unknown action"),
        "error should mention unknown action"
    );
}

/// Missing required fields for an action should produce an input validation error.
#[tokio::test]
#[expect(clippy::expect_used, reason = "test assertions")]
async fn click_missing_coordinates_returns_error() {
    let mut registry = ToolRegistry::new();
    aletheia_organon::builtins::computer_use::register(&mut registry, &SandboxConfig::default())
        .expect("register");

    let input = ToolInput {
        name: ToolName::new("computer_use").expect("valid"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({"action": "click"}),
    };

    let result = registry.execute(&input, &test_ctx()).await;
    assert!(
        result.is_err(),
        "missing coordinates should produce an error"
    );
}

/// Sandbox enforcement flag should propagate correctly.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn sandbox_enforcement_propagates() {
    use aletheia_organon::sandbox::{apply_sandbox, probe_landlock_abi};

    let config = SandboxConfig {
        enabled: true,
        enforcement: SandboxEnforcement::Enforcing,
        ..SandboxConfig::default()
    };

    let policy = config.build_policy(&PathBuf::from("/tmp/test"), &[]);
    assert!(policy.enabled, "policy should be enabled");

    // Build a command and apply sandbox to verify no panics.
    let mut cmd = std::process::Command::new("echo");
    cmd.arg("sandbox-test");

    if probe_landlock_abi().is_some() {
        let result = apply_sandbox(&mut cmd, policy);
        assert!(
            result.is_ok(),
            "sandbox application should succeed when Landlock is available"
        );

        let output = cmd.output().expect("command should execute");
        assert!(output.status.success(), "sandboxed echo should succeed");
    }
}

/// A sandboxed session denies writes outside the allowlist.
#[cfg(target_os = "linux")]
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn sandbox_denies_writes_outside_allowlist() {
    use aletheia_organon::sandbox::{apply_sandbox, probe_landlock_abi};

    if probe_landlock_abi().is_none() {
        // Skip on kernels without Landlock.
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let config = SandboxConfig {
        enabled: true,
        enforcement: SandboxEnforcement::Enforcing,
        ..SandboxConfig::default()
    };
    let policy = config.build_policy(dir.path(), &[]);

    // Try to write to /opt which is outside the allowlist.
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", "touch /opt/aletheia_sandbox_test_file 2>&1"]);

    let result = apply_sandbox(&mut cmd, policy);
    assert!(result.is_ok(), "sandbox setup should succeed");

    let output = cmd.output().expect("command should execute");
    // The touch command should fail because /opt is not in the write allowlist.
    assert!(
        !output.status.success(),
        "write to /opt should be denied by Landlock sandbox"
    );
}
