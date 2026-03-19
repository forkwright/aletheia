//! Tests for sandbox configuration and policy application.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::path::{Path, PathBuf};

use super::config::expand_tilde;
use super::*;
mod egress;

#[test]
fn default_config_is_enabled() {
    let config = SandboxConfig::default();
    assert!(config.enabled, "default config should be enabled");
    assert_eq!(
        config.enforcement,
        SandboxEnforcement::Permissive,
        "default enforcement should be permissive"
    );
    assert!(
        config.extra_read_paths.is_empty(),
        "default extra_read_paths should be empty"
    );
    assert!(
        config.extra_write_paths.is_empty(),
        "default extra_write_paths should be empty"
    );
    assert!(
        config.extra_exec_paths.is_empty(),
        "default extra_exec_paths should be empty"
    );
}

#[test]
fn disabled_config() {
    let config = SandboxConfig::disabled();
    assert!(!config.enabled, "disabled config should not be enabled");
}

#[test]
fn disabled_config_returns_disabled_policy() {
    let config = SandboxConfig::disabled();
    let policy = config.build_policy(Path::new("/tmp/ws"), &[PathBuf::from("/extra")]);
    assert!(
        !policy.enabled,
        "disabled config must produce disabled policy"
    );
    assert!(
        policy.read_paths.is_empty(),
        "disabled policy has no read paths"
    );
    assert!(
        policy.write_paths.is_empty(),
        "disabled policy has no write paths"
    );
    assert!(
        policy.exec_paths.is_empty(),
        "disabled policy has no exec paths"
    );
}

#[test]
fn config_serde_roundtrip() {
    let config = SandboxConfig {
        enabled: true,
        enforcement: SandboxEnforcement::Permissive,
        extra_read_paths: vec![PathBuf::from("/opt/data")],
        extra_write_paths: vec![PathBuf::from("/var/cache")],
        extra_exec_paths: vec![PathBuf::from("/opt/scripts")],
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };
    let json = serde_json::to_string(&config).expect("serialize");
    let back: SandboxConfig = serde_json::from_str(&json).expect("deserialize");
    assert!(back.enabled, "deserialized config should be enabled");
    assert_eq!(
        back.enforcement,
        SandboxEnforcement::Permissive,
        "enforcement should round-trip unchanged"
    );
    assert_eq!(
        back.extra_read_paths,
        vec![PathBuf::from("/opt/data")],
        "extra_read_paths should round-trip unchanged"
    );
    assert_eq!(
        back.extra_write_paths,
        vec![PathBuf::from("/var/cache")],
        "extra_write_paths should round-trip unchanged"
    );
    assert_eq!(
        back.extra_exec_paths,
        vec![PathBuf::from("/opt/scripts")],
        "extra_exec_paths should round-trip unchanged"
    );
}

#[test]
fn enforcement_serde() {
    let json = serde_json::to_string(&SandboxEnforcement::Enforcing).expect("serialize");
    assert_eq!(
        json, "\"enforcing\"",
        "Enforcing should serialize to lowercase string"
    );
    let back: SandboxEnforcement = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back,
        SandboxEnforcement::Enforcing,
        "Enforcing should round-trip unchanged"
    );

    let json = serde_json::to_string(&SandboxEnforcement::Permissive).expect("serialize");
    assert_eq!(
        json, "\"permissive\"",
        "Permissive should serialize to lowercase string"
    );
}

#[test]
fn config_from_yaml_defaults() {
    let json = "{}";
    let config: SandboxConfig = serde_json::from_str(json).expect("parse");
    assert!(
        config.enabled,
        "config from empty json should be enabled by default"
    );
    assert_eq!(
        config.enforcement,
        SandboxEnforcement::Permissive,
        "config from empty json should default to permissive"
    );
}

#[test]
fn policy_includes_workspace() {
    let config = SandboxConfig::default();
    let workspace = PathBuf::from("/home/agent/workspace");
    let policy = config.build_policy(&workspace, &[]);
    assert!(
        policy.write_paths.contains(&workspace),
        "workspace should be in write_paths"
    );
    assert!(
        policy.read_paths.contains(&workspace),
        "workspace should be in read_paths"
    );
}

#[test]
fn policy_includes_allowed_roots_as_read_only() {
    let config = SandboxConfig::default();
    let workspace = PathBuf::from("/home/agent/workspace");
    let extra = PathBuf::from("/shared/data");
    let policy = config.build_policy(&workspace, std::slice::from_ref(&extra));
    assert!(
        policy.read_paths.contains(&extra),
        "allowed_roots must appear in read_paths"
    );
    assert!(
        !policy.write_paths.contains(&extra),
        "allowed_roots must not appear in write_paths — read-only access only"
    );
}

#[test]
fn policy_includes_system_paths() {
    let config = SandboxConfig::default();
    let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
    assert!(
        policy.read_paths.contains(&PathBuf::from("/usr")),
        "policy should include /usr in read paths"
    );
    assert!(
        policy.read_paths.contains(&PathBuf::from("/lib")),
        "policy should include /lib in read paths"
    );
    assert!(
        policy.read_paths.contains(&PathBuf::from("/etc")),
        "policy should include /etc in read paths"
    );
    assert!(
        policy.exec_paths.contains(&PathBuf::from("/usr/bin")),
        "policy should include /usr/bin in exec paths"
    );
    assert!(
        policy.exec_paths.contains(&PathBuf::from("/bin")),
        "policy should include /bin in exec paths"
    );
    assert!(
        policy.exec_paths.contains(&PathBuf::from("/lib")),
        "policy should include /lib in exec paths"
    );
    assert!(
        policy.exec_paths.contains(&PathBuf::from("/lib64")),
        "policy should include /lib64 in exec paths"
    );
    assert!(
        policy.write_paths.contains(&PathBuf::from("/tmp")),
        "policy should include /tmp in write paths"
    );
}

#[test]
fn policy_includes_workspace_in_exec_paths() {
    let config = SandboxConfig::default();
    let workspace = PathBuf::from("/home/agent/workspace");
    let policy = config.build_policy(&workspace, &[]);
    assert!(
        policy.exec_paths.contains(&workspace),
        "workspace must be in exec_paths so agents can run scripts in their workspace"
    );
}

#[test]
fn policy_includes_allowed_roots_in_exec_paths() {
    let config = SandboxConfig::default();
    let workspace = PathBuf::from("/home/agent/workspace");
    let shared = PathBuf::from("/shared/scripts");
    let policy = config.build_policy(&workspace, std::slice::from_ref(&shared));
    assert!(
        policy.exec_paths.contains(&shared),
        "allowed_roots must be in exec_paths so agents can run scripts in shared dirs"
    );
}

#[test]
fn policy_includes_extra_exec_paths() {
    let config = SandboxConfig {
        extra_exec_paths: vec![PathBuf::from("/opt/scripts")],
        ..SandboxConfig::default()
    };
    let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
    assert!(
        policy.exec_paths.contains(&PathBuf::from("/opt/scripts")),
        "extra_exec_paths must appear in exec_paths"
    );
}

#[test]
fn expand_tilde_replaces_home() {
    if let Ok(home) = std::env::var("HOME") {
        let p = expand_tilde(Path::new("~/scripts"));
        assert_eq!(
            p,
            PathBuf::from(format!("{home}/scripts")),
            "tilde prefix should be replaced with HOME value"
        );

        let p2 = expand_tilde(Path::new("~"));
        assert_eq!(p2, PathBuf::from(&home), "bare tilde should expand to HOME");
    }
}

#[test]
fn expand_tilde_leaves_absolute_path_unchanged() {
    let p = expand_tilde(Path::new("/usr/local/bin"));
    assert_eq!(
        p,
        PathBuf::from("/usr/local/bin"),
        "absolute path should be returned unchanged"
    );
}

#[test]
fn policy_expands_tilde_in_extra_exec_paths() {
    if let Ok(home) = std::env::var("HOME") {
        let config = SandboxConfig {
            extra_exec_paths: vec![PathBuf::from("~")],
            ..SandboxConfig::default()
        };
        let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
        assert!(
            policy.exec_paths.contains(&PathBuf::from(&home)),
            "~ in extra_exec_paths must be expanded to HOME"
        );
    }
}

#[test]
fn policy_includes_extra_paths() {
    let config = SandboxConfig {
        extra_read_paths: vec![PathBuf::from("/opt/readonly")],
        extra_write_paths: vec![PathBuf::from("/var/scratch")],
        ..SandboxConfig::default()
    };
    let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
    assert!(
        policy.read_paths.contains(&PathBuf::from("/opt/readonly")),
        "extra_read_paths should appear in policy read_paths"
    );
    assert!(
        policy.write_paths.contains(&PathBuf::from("/var/scratch")),
        "extra_write_paths should appear in policy write_paths"
    );
    assert!(
        policy.read_paths.contains(&PathBuf::from("/var/scratch")),
        "write paths should also be readable"
    );
}

#[test]
fn policy_no_duplicate_write_roots() {
    let config = SandboxConfig::default();
    let workspace = PathBuf::from("/home/agent/workspace");
    let policy = config.build_policy(&workspace, std::slice::from_ref(&workspace));
    let count = policy
        .write_paths
        .iter()
        .filter(|p| **p == workspace)
        .count();
    assert_eq!(count, 1, "workspace should not be duplicated");
}

#[cfg(target_os = "linux")]
#[test]
fn probe_returns_consistent_result() {
    let first = probe_landlock_abi();
    let second = probe_landlock_abi();
    assert_eq!(
        first, second,
        "ABI probe must be deterministic across calls"
    );
    if let Some(abi) = first {
        assert!(
            abi >= 1,
            "ABI version must be at least 1 when available, got {abi}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn permissive_skips_sandbox_when_landlock_unavailable() {
    use std::process::Command;

    // WHY: Simulate the permissive fallback by building a policy with permissive
    // enforcement and verifying the tool still executes even when we cannot
    // rely on Landlock being present.
    let config = SandboxConfig {
        enforcement: SandboxEnforcement::Permissive,
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("echo");
    cmd.arg("permissive fallback");

    // WHY: apply_sandbox must not return an error in permissive mode regardless
    // of whether Landlock is available on this kernel.
    let result = apply_sandbox(&mut cmd, policy);
    assert!(
        result.is_ok(),
        "permissive mode must not error when sandbox is unavailable: {result:?}"
    );

    let output = cmd.output().expect("spawn");
    assert!(
        output.status.success(),
        "tool must execute in permissive mode"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("permissive fallback"),
        "tool output must be captured"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn enforcing_surfaces_clear_error_when_landlock_unavailable() {
    use std::process::Command;

    // WHY: We cannot force a kernel to lack Landlock in a unit test.
    // Instead we verify the error message content when probe returns None,
    // testing the code path directly via the internal helper.
    let config = SandboxConfig {
        enforcement: SandboxEnforcement::Enforcing,
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("echo");
    cmd.arg("should not run");

    if probe_landlock_abi().is_none() {
        let err = apply_sandbox(&mut cmd, policy).expect_err("enforcing must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("Landlock not available"),
            "error must name Landlock: {msg}"
        );
        assert!(
            msg.contains("ABI"),
            "error must mention ABI for diagnostics: {msg}"
        );
        assert!(
            msg.contains("enforcement=permissive"),
            "error must suggest permissive mode: {msg}"
        );
    } else {
        let result = apply_sandbox(&mut cmd, policy);
        assert!(
            result.is_ok(),
            "enforcing mode must succeed when Landlock is available: {result:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn landlock_applies_in_child() {
    use std::process::Command;

    let config = SandboxConfig::default();
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("cat");
    cmd.arg("/etc/hostname");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "reading /etc/hostname should be allowed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn landlock_blocks_outside_workspace() {
    use std::process::Command;

    let dir = tempfile::tempdir().expect("create temp dir");
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "top secret").expect("write");

    let workspace = tempfile::tempdir().expect("create workspace");

    let read_paths = vec![
        PathBuf::from("/usr"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
        PathBuf::from("/etc"),
        PathBuf::from("/proc"),
        PathBuf::from("/dev"),
        workspace.path().to_path_buf(),
    ];
    let write_paths = vec![workspace.path().to_path_buf()];
    let exec_paths = vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/lib"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
    ];

    let policy = SandboxPolicy {
        enabled: true,
        read_paths,
        write_paths,
        exec_paths,
        enforcement: SandboxEnforcement::Enforcing,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };

    let mut cmd = Command::new("/usr/bin/cat");
    cmd.arg(&secret);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "reading outside workspace should be blocked (stderr={stderr})"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_mount() {
    use std::process::Command;

    let config = SandboxConfig::default();
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("mount -t tmpfs none /mnt 2>&1; echo $?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("Operation not permitted")
            || combined.contains("EPERM")
            || combined.contains('1'),
        "mount should be blocked by seccomp: {combined}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn seccomp_allows_normal_operations() {
    use std::process::Command;

    let config = SandboxConfig::default();
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("echo");
    cmd.arg("hello sandbox");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "normal echo command should succeed under sandbox"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("hello sandbox"),
        "sandbox should not suppress stdout"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn permissive_mode_allows_access() {
    use std::process::Command;

    let config = SandboxConfig {
        enforcement: SandboxEnforcement::Permissive,
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("echo");
    cmd.arg("permissive test");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "command should succeed with permissive sandbox"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn sandbox_with_exec_tool_flow() {
    use std::process::Command;

    let config = SandboxConfig::default();
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("test.txt"), "sandbox test data").expect("write");

    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("cat");
    cmd.arg(dir.path().join("test.txt"));
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "reading a file inside workspace should succeed"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("sandbox test data"),
        "file content should be readable inside workspace"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn sandbox_write_in_workspace() {
    use std::process::Command;

    let config = SandboxConfig::default();
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let outfile = dir.path().join("output.txt");
    let cmd_str = format!("echo written > {}", outfile.display());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(output.status.success(), "writing in workspace should work");
    assert!(
        outfile.exists(),
        "output file should exist after write in workspace"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn sandbox_write_outside_workspace_blocked() {
    use std::process::Command;

    let workspace = tempfile::tempdir().expect("create workspace");
    let outside = tempfile::tempdir().expect("create outside dir");
    let policy = SandboxPolicy {
        enabled: true,
        read_paths: vec![
            PathBuf::from("/usr"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            PathBuf::from("/etc"),
            PathBuf::from("/proc"),
            PathBuf::from("/dev"),
            workspace.path().to_path_buf(),
        ],
        write_paths: vec![workspace.path().to_path_buf()],
        exec_paths: vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/lib"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
        ],
        enforcement: SandboxEnforcement::Enforcing,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };

    let outfile = outside.path().join("escape.txt");
    let cmd_str = format!("echo escape > {} 2>&1; echo $?", outfile.display());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        !outfile.exists() || {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.trim().ends_with('1')
        },
        "writing outside workspace should be blocked"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn exec_succeeds_under_sandbox_with_absolute_and_bare_paths() {
    use std::process::Command;

    if probe_landlock_abi().is_none() {
        return;
    }

    let config = SandboxConfig::default();
    let dir = tempfile::tempdir().expect("create temp dir");

    let policy = config.build_policy(dir.path(), &[]);
    let mut cmd = Command::new("/usr/bin/uname");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");
    let output = cmd.output().expect("spawn");
    assert!(
        output.status.success(),
        "absolute path exec must succeed under sandbox: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let policy = config.build_policy(dir.path(), &[]);
    let mut cmd = Command::new("uname");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");
    let output = cmd.output().expect("spawn");
    assert!(
        output.status.success(),
        "bare command exec must succeed under sandbox: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
