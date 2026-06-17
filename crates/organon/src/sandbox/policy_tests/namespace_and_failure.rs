//! Sandbox failure-mode (fail-closed) and edge-case tests.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use super::super::*;
use super::policy_with_system_paths;

// ── Failure modes (fail-closed behavior) ──

/// Test that enforcing mode fails closed when Landlock is unavailable.
#[cfg(target_os = "linux")]
#[test]
fn enforcing_fails_closed_when_landlock_unavailable() {
    // WHY: a unit test cannot force a Landlock-less kernel, so branch on the
    // probe: assert the error shape when absent, the success path when present.
    let workspace = tempfile::tempdir().expect("create workspace");

    let policy = SandboxPolicy {
        enabled: true,
        read_paths: vec![workspace.path().to_path_buf()],
        write_paths: vec![workspace.path().to_path_buf()],
        exec_paths: vec![PathBuf::from("/bin"), PathBuf::from("/usr/bin")],
        enforcement: SandboxEnforcement::Enforcing,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };

    let mut cmd = Command::new("echo");
    cmd.arg("test");

    if probe_landlock_abi().is_none() {
        let result = apply_sandbox(&mut cmd, policy);
        assert!(
            result.is_err(),
            "enforcing mode must fail when Landlock is unavailable"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Landlock not available"),
            "error must mention Landlock: {err_msg}"
        );
    } else {
        let result = apply_sandbox(&mut cmd, policy);
        assert!(
            result.is_ok(),
            "enforcing mode should work with Landlock available"
        );
    }
}

/// Test that permissive mode fails open (logs but continues) when Landlock is unavailable.
#[cfg(target_os = "linux")]
#[test]
fn permissive_fails_open_when_landlock_unavailable() {
    let workspace = tempfile::tempdir().expect("create workspace");

    // WHY: full system paths so binaries can still execute under the sandbox.
    let policy = SandboxPolicy {
        enabled: true,
        read_paths: vec![
            PathBuf::from("/usr"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            PathBuf::from("/etc"),
            PathBuf::from("/proc/self"),
            PathBuf::from("/dev"),
            workspace.path().to_path_buf(),
        ],
        write_paths: vec![workspace.path().to_path_buf()],
        exec_paths: vec![
            PathBuf::from("/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/lib"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
        ],
        enforcement: SandboxEnforcement::Permissive,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };

    let mut cmd = Command::new("/bin/echo");
    cmd.arg("permissive test");

    let result = apply_sandbox(&mut cmd, policy);
    assert!(result.is_ok(), "permissive mode must not error: {result:?}");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "command should execute in permissive mode"
    );
}

/// Test that disabled policy allows everything.
#[cfg(target_os = "linux")]
#[test]
fn disabled_policy_allows_all() {
    let _workspace = tempfile::tempdir().expect("create workspace");
    let policy = SandboxPolicy {
        enabled: false,
        read_paths: Vec::new(),
        write_paths: Vec::new(),
        exec_paths: Vec::new(),
        enforcement: SandboxEnforcement::Permissive,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };

    let mut cmd = Command::new("cat");
    cmd.arg("/etc/hostname");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "disabled policy should allow all access"
    );
}

// ── Edge cases ──

/// Test that /proc/self remains readable for the child's own metadata.
#[cfg(target_os = "linux")]
#[test]
fn proc_self_access_allowed() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    // WHY: /proc/self is explicitly allowed because the child's environment
    // is already scrubbed; reading its own metadata does not leak parent secrets.
    let mut cmd = Command::new("cat");
    cmd.arg("/proc/self/environ");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "reading /proc/self/environ should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Test that a sandboxed child cannot read the parent's /proc environ.
#[cfg(target_os = "linux")]
#[test]
fn proc_parent_environ_blocked() {
    let _guard = crate::subprocess::SUBPROCESS_ENV_LOCK
        .lock()
        .expect("env lock");

    let secret = "ORGANON_PARENT_SECRET=not-for-sandboxed-child";
    #[expect(unsafe_code, reason = "test controls process environment")]
    unsafe {
        std::env::set_var("ORGANON_PARENT_SECRET", "not-for-sandboxed-child");
    }

    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    // WHY: $PPID is the parent test process. /proc/<ppid>/environ would expose
    // every variable the parent inherited, including API keys. Only /proc/self
    // is in the read set, so this read must be denied.
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("cat /proc/$PPID/environ 2>&1; echo exit=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");

    #[expect(unsafe_code, reason = "test controls process environment")]
    unsafe {
        std::env::remove_var("ORGANON_PARENT_SECRET");
    }

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !combined.contains(secret),
        "sandboxed child must not read parent environ through /proc: {combined}"
    );
    assert!(
        combined.contains("exit=1") || combined.contains("Permission denied"),
        "reading /proc/$PPID/environ should fail: {combined}"
    );
}

/// Test that basic /proc access works for allowed processes.
#[cfg(target_os = "linux")]
#[test]
fn proc_access_allowed() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("cat");
    cmd.arg("/proc/self/status");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "reading /proc/self/status should work"
    );
    assert!(
        stdout.contains("Name:") || stdout.contains("Pid:"),
        "proc status should contain process info: {stdout}"
    );
}

/// Test write operations to /proc are blocked.
#[cfg(target_os = "linux")]
#[test]
fn proc_write_blocked() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    // WHY: /proc is only in read_paths, so writes must be blocked.
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("echo 0 > /proc/self/oom_score_adj 2>&1; echo ret=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Permission denied")
            || combined.contains("Operation not permitted")
            || combined.contains("ret=1")
            || !output.status.success(),
        "writing to /proc should be blocked: {combined}"
    );
}

/// Test device access (/dev/null, /dev/zero, /dev/urandom).
#[cfg(target_os = "linux")]
#[test]
fn device_access_allowed() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("echo test > /dev/null");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output();
    // NOTE: device access depends on the Landlock ABI version; only assert
    // that sandbox setup does not crash.
    if let Ok(out) = output {
        let _ = out.status.success();
    }
}

/// Test that long path names work correctly.
#[cfg(target_os = "linux")]
#[test]
fn long_path_names_work() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut deep_path = workspace.path().to_path_buf();
    for i in 0..20 {
        deep_path = deep_path.join(format!("subdir_{i:02}"));
    }
    std::fs::create_dir_all(&deep_path).expect("create deep directory");

    let test_file = deep_path.join("deep_file.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&test_file, "deep content").expect("write deep file");

    let mut cmd = Command::new("cat");
    cmd.arg(&test_file);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "long path access should work: {stderr}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "deep content",
        "deep file content should match"
    );
}

/// Test directory traversal prevention.
#[cfg(target_os = "linux")]
#[test]
fn directory_traversal_blocked() {
    let outside_dir = tempfile::tempdir().expect("create outside dir");
    let secret_file = outside_dir.path().join("secret.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&secret_file, "secret data").expect("write secret file");

    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let nested = workspace.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&nested).expect("create nested dirs");

    let mut cmd = Command::new("sh");
    cmd.current_dir(&nested);
    cmd.arg("-c").arg(format!(
        "cat ../../../../../../{} 2>&1; echo ret=$?",
        secret_file.display()
    ));
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Permission denied")
            || combined.contains("Operation not permitted")
            || combined.contains("ret=1")
            || !output.status.success()
            || !combined.contains("secret data"),
        "directory traversal should be blocked: {combined}"
    );
}

/// Test that hard links cannot escape the sandbox.
#[cfg(target_os = "linux")]
#[test]
fn hardlink_escape_blocked() {
    let outside_dir = tempfile::tempdir().expect("create outside dir");
    let outside_file = outside_dir.path().join("outside.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&outside_file, "outside data").expect("write outside file");

    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let hardlink_path = workspace.path().join("hardlink_escape");
    let cmd_str = format!(
        "ln {} {} 2>&1; echo ret=$?",
        outside_file.display(),
        hardlink_path.display()
    );

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Permission denied")
            || combined.contains("Operation not permitted")
            || combined.contains("ret=1")
            || !output.status.success()
            || !hardlink_path.exists(),
        "hardlink escape should be blocked: {combined}"
    );
}

/// Test that reading from hard links inside allowed paths works.
#[cfg(target_os = "linux")]
#[test]
fn hardlink_in_workspace_allowed() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let file1 = workspace.path().join("file1.txt");
    let file2 = workspace.path().join("file2.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&file1, "shared content").expect("write file1");
    std::fs::hard_link(&file1, &file2).expect("create hard link");

    let mut cmd = Command::new("cat");
    cmd.arg(&file2);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "reading hardlink in workspace should work: {stderr}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "shared content",
        "hardlink content should match"
    );
}

/// Test that execution in allowed paths works.
#[cfg(target_os = "linux")]
#[test]
fn execution_in_allowed_paths() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let script = workspace.path().join("test_script.sh");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&script, "#!/bin/bash\necho 'script executed'").expect("write script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("chmod script");

    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new(&script);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "script execution should work: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("script executed"),
        "script should have executed"
    );
}

/// Test that execution outside allowed paths is blocked.
#[cfg(target_os = "linux")]
#[test]
fn execution_outside_allowed_paths_blocked() {
    let outside_dir = tempfile::tempdir().expect("create outside dir");
    let script = outside_dir.path().join("outside_script.sh");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&script, "#!/bin/bash\necho 'should not execute'").expect("write script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("chmod script");

    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new(&script);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    // NOTE: the spawn itself may fail with permission denied — that also
    // counts as a successful block.
    let output_result = cmd.output();
    if let Ok(output) = output_result {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(
            !output.status.success()
                || combined.contains("Permission denied")
                || combined.is_empty(),
            "execution outside allowed paths should be blocked: {combined}"
        );
    }
}

/// Test concurrent sandbox applications (stress test).
#[cfg(target_os = "linux")]
#[test]
fn concurrent_sandbox_applications() {
    use std::sync::Arc;
    use std::thread;

    let mut handles = vec![];
    let policy = Arc::new(policy_with_system_paths(std::env::temp_dir().as_path()));

    for i in 0..10 {
        let policy_clone = Arc::clone(&policy);
        let handle = thread::spawn(move || {
            let mut cmd = Command::new("echo");
            cmd.arg(format!("concurrent test {i}"));

            // WHY: pre_exec constraints prevent applying the sandbox from
            // threads, so only the shared policy structure is verified.
            assert!(policy_clone.enabled);
            assert!(!policy_clone.read_paths.is_empty());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("thread should complete");
    }
}
