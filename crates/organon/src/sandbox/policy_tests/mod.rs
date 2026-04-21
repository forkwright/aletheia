//! Split from `policy_tests.rs` (1198 lines) to satisfy `RUST/file-too-long`.

use std::path::PathBuf;

use super::*;

mod edge_cases;
mod landlock_and_seccomp;
mod namespace_and_failure;

/// Create a minimal sandbox policy for testing with specified paths.
fn test_policy(
    read_paths: Vec<PathBuf>,
    write_paths: Vec<PathBuf>,
    exec_paths: Vec<PathBuf>,
) -> SandboxPolicy {
    SandboxPolicy {
        enabled: true,
        read_paths,
        write_paths,
        exec_paths,
        enforcement: SandboxEnforcement::Enforcing,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    }
}

/// Create a policy with system paths included (typical production setup).
fn policy_with_system_paths(workspace: &std::path::Path) -> SandboxPolicy {
    let mut read_paths = vec![
        PathBuf::from("/usr"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
        PathBuf::from("/etc"),
        PathBuf::from("/proc"),
        PathBuf::from("/dev"),
    ];
    let write_paths = vec![workspace.to_path_buf()];
    let exec_paths = vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/lib"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
        workspace.to_path_buf(),
    ];

    // Write paths are also readable
    for wp in &write_paths {
        if !read_paths.contains(wp) {
            read_paths.push(wp.clone());
        }
    }

    test_policy(read_paths, write_paths, exec_paths)
}
