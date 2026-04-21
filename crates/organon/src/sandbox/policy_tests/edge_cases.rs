//! (Split from `policy_tests.rs` — see parent mod.)

#![expect(clippy::expect_used, reason = "test assertions")]

use std::path::PathBuf;
use std::process::Command;

use super::super::*;
use super::policy_with_system_paths;

#[test]
fn temp_directory_access() {
    let workspace = tempfile::tempdir().expect("create workspace");
    let policy = policy_with_system_paths(workspace.path());

    // Use the workspace temp directory, not system /tmp
    let temp_file = workspace.path().join("sandbox_temp_test.txt");
    let cmd_str = format!(
        "echo 'temp test' > {} && cat {} && rm {}",
        temp_file.display(),
        temp_file.display(),
        temp_file.display()
    );

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "temp directory access should work: {stderr}"
    );
    assert!(
        stdout.contains("temp test"),
        "temp file content should match: {stdout}"
    );
}

/// Test read-only paths are actually read-only.
// WHY(#3707): assert on the filesystem invariant, not the shell exit
// string. The Landlock restriction we're testing is "file-under-
// read_paths is not modified". Different kernel versions surface that
// as different shell exit codes (1 on fedora 43's 6.19 kernel, 2 on
// GitHub's ubuntu-latest), and the test used to branch on a brittle
// string match. The security invariant is: file contents unchanged +
// child did not report success.
/// Original contents written to the read-only fixture before the
/// sandboxed child attempts to overwrite it. Declared at module scope
/// so the body of `read_only_paths_cannot_be_written` has no item-
/// after-statements (`clippy::items_after_statements` on stable).
const READONLY_TEST_ORIGINAL: &str = "original";

#[cfg(target_os = "linux")]
#[test]
fn read_only_paths_cannot_be_written() {
    let read_only_dir = tempfile::tempdir().expect("create read-only dir");
    let test_file = read_only_dir.path().join("readonly.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup requires direct filesystem access"
    )]
    std::fs::write(&test_file, READONLY_TEST_ORIGINAL).expect("write test file");

    let workspace = tempfile::tempdir().expect("create workspace");
    let mut policy = policy_with_system_paths(workspace.path());
    // Add read-only dir to read_paths but NOT write_paths
    policy.read_paths.push(read_only_dir.path().to_path_buf());

    let cmd_str = format!("echo 'modified' > {}", test_file.display());

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&cmd_str);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");

    // Invariant 1: the file on disk is unchanged. Landlock enforcement
    // is visible here regardless of shell exit conventions.
    let after = std::fs::read_to_string(&test_file).expect("read test file");
    assert_eq!(
        after,
        READONLY_TEST_ORIGINAL,
        "read-only path was modified despite Landlock policy; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Invariant 2: the child reported a non-zero exit (some signal of
    // failure). We don't depend on *which* non-zero code — the shell
    // picks 1 on some kernels and 2 on others for redirection failure.
    assert!(
        !output.status.success(),
        "child reported success despite read-only Landlock policy; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Test sandbox apply function directly (unit test for the apply method).
#[cfg(target_os = "linux")]
#[test]
fn sandbox_policy_apply_unit() {
    let workspace = tempfile::tempdir().expect("create workspace");

    // Create a policy with minimal paths
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
            workspace.path().to_path_buf(),
        ],
        enforcement: SandboxEnforcement::Permissive,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };

    // Verify the policy structure
    assert!(policy.enabled);
    assert!(!policy.read_paths.is_empty());
    assert!(!policy.write_paths.is_empty());
    assert!(!policy.exec_paths.is_empty());
}
