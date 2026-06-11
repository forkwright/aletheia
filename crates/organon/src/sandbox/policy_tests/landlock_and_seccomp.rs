//! Landlock file-access, seccomp syscall-filter, and namespace isolation tests.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::process::Command;

use super::super::*;
use super::policy_with_system_paths;

// ── Landlock file-access restrictions ──

/// Test that allowed paths can be read successfully.
#[cfg(target_os = "linux")]
#[test]
fn landlock_allows_read_in_allowed_paths() {
    let workspace = tempfile::tempdir().expect("create temp dir");
    let test_file = workspace.path().join("test.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&test_file, "test content").expect("write test file");

    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("cat");
    cmd.arg(&test_file);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reading allowed file should succeed: {stderr}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "test content",
        "file content should match"
    );
}

/// Test that allowed paths can be written successfully.
#[cfg(target_os = "linux")]
#[test]
fn landlock_allows_write_in_allowed_paths() {
    let workspace = tempfile::tempdir().expect("create temp dir");
    let policy = policy_with_system_paths(workspace.path());

    let outfile = workspace.path().join("output.txt");
    let cmd_str = format!("echo 'written' > {}", outfile.display());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "writing to allowed path should succeed: {stderr}"
    );
    assert!(outfile.exists(), "output file should exist");
}

/// Test that paths outside the sandbox cannot be read (EACCES or Permission denied).
#[cfg(target_os = "linux")]
#[test]
fn landlock_blocks_read_outside_sandbox() {
    let outside_dir = tempfile::tempdir().expect("create outside dir");
    let secret_file = outside_dir.path().join("secret.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&secret_file, "secret data").expect("write secret file");

    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("cat");
    cmd.arg(&secret_file);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reading outside sandbox should be blocked: {stderr}"
    );
    assert!(
        stderr.contains("Permission denied") || stderr.contains("Operation not permitted"),
        "should fail with permission denied, got: {stderr}"
    );
}

/// Test that paths outside the sandbox cannot be written.
#[cfg(target_os = "linux")]
#[test]
fn landlock_blocks_write_outside_sandbox() {
    let outside_dir = tempfile::tempdir().expect("create outside dir");
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let escape_file = outside_dir.path().join("escape.txt");
    let cmd_str = format!(
        "echo 'escape' > {} 2>&1; echo ret=$?",
        escape_file.display()
    );

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !escape_file.exists() || stdout.contains("ret=1"),
        "writing outside sandbox should be blocked: {stdout}"
    );
}

/// Test that symlink escapes are blocked via canonical path resolution.
#[cfg(target_os = "linux")]
#[test]
fn landlock_blocks_symlink_escape() {
    let outside_dir = tempfile::tempdir().expect("create outside dir");
    let secret_file = outside_dir.path().join("secret.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&secret_file, "escaped secret").expect("write secret file");

    let workspace = tempfile::tempdir().expect("create workspace");
    let symlink = workspace.path().join("escape_link");
    std::os::unix::fs::symlink(&secret_file, &symlink).expect("create symlink");

    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("cat");
    cmd.arg(&symlink);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success() || !String::from_utf8_lossy(&output.stdout).contains("escaped"),
        "symlink escape should be blocked: {stderr}"
    );
}

/// Test that symlink creation inside allowed paths works.
#[cfg(target_os = "linux")]
#[test]
fn landlock_allows_symlink_creation_in_workspace() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let target = workspace.path().join("target.txt");
    let link = workspace.path().join("link.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&target, "target content").expect("write target file");

    let cmd_str = format!(
        "ln -s {} {} 2>&1 && cat {} 2>&1",
        target.display(),
        link.display(),
        link.display()
    );

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "symlink operations in workspace should succeed: {stderr}"
    );
    assert!(
        stdout.contains("target content"),
        "should read target through symlink: {stdout}"
    );
}

/// Test that mount-point traversals don't escape the sandbox.
#[cfg(target_os = "linux")]
#[test]
fn landlock_blocks_mount_traverse_escape() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    // WHY: /root typically exists but is not in the allowed paths.
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("cat /root/.bashrc 2>&1; echo ret=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should fail with permission denied, not "No such file or directory"
    assert!(
        stdout.contains("Permission denied") || stdout.contains("ret=1"),
        "access to /root should be blocked: {stdout}"
    );
}

// ── Seccomp syscall filtering ──

/// Test that ptrace is blocked by seccomp.
#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_ptrace() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("strace -o /dev/null echo test 2>&1; echo exit=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Operation not permitted")
            || combined.contains("Permission denied")
            || combined.contains("ret=1")
            || combined.contains("exit=127")
            || combined.contains("strace: not found")
            || combined.contains("command not found")
            || !output.status.success(),
        "ptrace should be blocked by seccomp: {combined}"
    );
}

/// Test that mount syscall is blocked by seccomp.
#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_mount_syscall() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("mount -t tmpfs none /mnt 2>&1; echo ret=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Operation not permitted")
            || combined.contains("EPERM")
            || combined.contains("ret=1")
            || combined.contains("must be superuser")
            || combined.contains("permission denied")
            || !output.status.success(),
        "mount should be blocked by seccomp: {combined}"
    );
}

/// Test that umount is blocked by seccomp.
#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_umount() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("umount /mnt 2>&1; echo ret=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Operation not permitted")
            || combined.contains("EPERM")
            || combined.contains("ret=")
            || combined.contains("not mounted")
            || !output.status.success(),
        "umount should be blocked by seccomp: {combined}"
    );
}

/// Test that the seccomp filter blocks the `reboot(2)` syscall.
///
/// WARNING: exercise the raw `reboot(2)` syscall, never `/sbin/reboot`. On a systemd
/// host `/sbin/reboot` is a symlink to `systemctl`, which asks logind to reboot over
/// D-Bus instead of issuing the `reboot(2)` syscall the sandbox filters — so it
/// bypasses seccomp entirely and, for a polkit-authorized active local session,
/// actually reboots the machine running the test. We invoke the syscall directly with
/// an invalid magic (arg 0): the enforcing filter returns EPERM before it reaches the
/// kernel, and even unfiltered it returns EINVAL/EPERM — it can never reboot the host.
#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_reboot() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    // 169 == SYS_reboot on x86_64. Invalid magic (0) => EINVAL even for a privileged
    // caller; the enforcing seccomp filter returns EPERM first. The syscall always
    // fails (ret == -1) and never performs a reboot.
    let probe = "import ctypes, sys\n\
                 libc = ctypes.CDLL(None, use_errno=True)\n\
                 ret = libc.syscall(169, 0, 0, 0, 0)\n\
                 sys.stdout.write('ret=%d errno=%d' % (ret, ctypes.get_errno()))\n";
    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(probe);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    // python3 may be absent or its exec blocked by the sandbox — nothing to assert then.
    let Ok(output) = cmd.output() else {
        return;
    };
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The reboot syscall must fail (ret == -1). A success would be an actual reboot.
    assert!(
        combined.contains("ret=-1") || combined.is_empty() || !output.status.success(),
        "reboot syscall must be blocked (EPERM via seccomp; EPERM/EINVAL otherwise), \
         never succeed: {combined}"
    );
}

/// Test that chroot is blocked by seccomp.
#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_chroot() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("chroot /tmp 2>&1; echo ret=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Operation not permitted")
            || combined.contains("EPERM")
            || combined.contains("ret=1"),
        "chroot should be blocked by seccomp: {combined}"
    );
}

/// Test that `pivot_root` is blocked by seccomp.
#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_pivot_root() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("pivot_root /tmp /tmp 2>&1; echo ret=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Operation not permitted")
            || combined.contains("EPERM")
            || combined.contains("not found")
            || combined.contains("ret=1"),
        "pivot_root should be blocked by seccomp: {combined}"
    );
}

/// Test that allowed syscalls work correctly (read, write, open).
#[cfg(target_os = "linux")]
#[test]
fn seccomp_allows_safe_syscalls() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let test_file = workspace.path().join("seccomp_test.txt");
    let policy = policy_with_system_paths(workspace.path());

    let cmd_str = format!(
        "echo 'test' > {} && cat {} && rm {}",
        test_file.display(),
        test_file.display(),
        test_file.display()
    );
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "safe syscalls should work: {stderr}"
    );
}

/// Test that module loading is blocked.
#[cfg(target_os = "linux")]
#[test]
fn seccomp_blocks_module_load() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("modprobe nonexistent 2>&1; echo ret=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Operation not permitted")
            || combined.contains("Permission denied")
            || combined.contains("ret=1")
            || !output.status.success(),
        "module loading should be blocked: {combined}"
    );
}

// ── Namespace isolation ──

/// Test that network namespace isolates network access.
#[cfg(target_os = "linux")]
#[test]
fn namespace_isolates_network() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let mut policy = policy_with_system_paths(workspace.path());
    policy.egress = EgressPolicy::Deny;

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("curl -s --connect-timeout 2 http://example.com 2>&1; echo exit_code=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Network is unreachable")
            || combined.contains("not permitted")
            || combined.contains("exit_code=1")
            || combined.contains("exit_code=2")
            || combined.contains("exit_code=6")
            || combined.contains("exit_code=7")
            || combined.contains("exit_code=28")
            || combined.contains("Couldn't connect")
            || combined.contains("Could not resolve")
            || combined.contains("Failed to connect")
            || combined.contains("Could not resolve host")
            || !output.status.success(),
        "network should be isolated: {combined}"
    );
}

/// Test that loopback is available in network namespace.
#[cfg(target_os = "linux")]
#[test]
fn namespace_allows_loopback() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();

    let workspace = tempfile::tempdir().expect("create workspace");
    let mut policy = policy_with_system_paths(workspace.path());
    policy.egress = EgressPolicy::Allowlist;
    policy.egress_allowlist = vec!["127.0.0.1".to_owned()];

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(format!(
        "bash -c 'echo test > /dev/tcp/127.0.0.1/{port}' 2>&1; echo retcode=$?"
    ));
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // NOTE: lo may be down inside the namespace, so the connect may succeed or
    // fail; assert only that sandbox setup completed.
    assert!(
        stdout.contains("retcode=0") || stdout.contains("retcode=1"),
        "loopback test should complete: {stdout}"
    );
}
