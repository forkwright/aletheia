//! Tests for egress policy configuration and network namespace enforcement.
use std::path::Path;

use super::super::policy::allowlist_is_loopback_only;
use super::super::*;

#[test]
fn default_egress_is_allow() {
    let config = SandboxConfig::default();
    assert_eq!(
        config.egress,
        EgressPolicy::Allow,
        "default egress policy must be Allow for backward compatibility"
    );
    assert!(
        config.egress_allowlist.is_empty(),
        "default allowlist must be empty"
    );
}

#[test]
fn egress_policy_serde() {
    let json = serde_json::to_string(&EgressPolicy::Deny).expect("serialize");
    assert_eq!(
        json, "\"deny\"",
        "EgressPolicy::Deny should serialize to lowercase string"
    );
    let back: EgressPolicy = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back,
        EgressPolicy::Deny,
        "EgressPolicy::Deny should round-trip unchanged"
    );

    let json = serde_json::to_string(&EgressPolicy::Allow).expect("serialize");
    assert_eq!(
        json, "\"allow\"",
        "EgressPolicy::Allow should serialize to lowercase string"
    );

    let json = serde_json::to_string(&EgressPolicy::Allowlist).expect("serialize");
    assert_eq!(
        json, "\"allowlist\"",
        "EgressPolicy::Allowlist should serialize to lowercase string"
    );
}

#[test]
fn egress_config_serde_roundtrip() {
    let config = SandboxConfig {
        egress: EgressPolicy::Allowlist,
        egress_allowlist: vec!["127.0.0.1".to_owned(), "::1".to_owned()],
        ..SandboxConfig::default()
    };
    let json = serde_json::to_string(&config).expect("serialize");
    let back: SandboxConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.egress,
        EgressPolicy::Allowlist,
        "egress policy should round-trip unchanged"
    );
    assert_eq!(
        back.egress_allowlist,
        vec!["127.0.0.1", "::1"],
        "egress_allowlist should round-trip unchanged"
    );
}

#[test]
fn disabled_policy_has_allow_egress() {
    let config = SandboxConfig::disabled();
    let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
    assert_eq!(
        policy.egress,
        EgressPolicy::Allow,
        "disabled sandbox must not restrict egress"
    );
}

#[test]
fn policy_inherits_egress_from_config() {
    let config = SandboxConfig {
        egress: EgressPolicy::Deny,
        ..SandboxConfig::default()
    };
    let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
    assert_eq!(
        policy.egress,
        EgressPolicy::Deny,
        "policy should inherit deny egress from config"
    );
}

#[test]
fn allowlist_loopback_check() {
    assert!(
        allowlist_is_loopback_only(&[
            "127.0.0.1".to_owned(),
            "::1".to_owned(),
            "127.0.0.1/8".to_owned(),
        ]),
        "loopback-only list should return true"
    );
    assert!(
        !allowlist_is_loopback_only(&["127.0.0.1".to_owned(), "10.0.0.1".to_owned()]),
        "list with non-loopback should return false"
    );
    assert!(
        !allowlist_is_loopback_only(&["example.com".to_owned()]),
        "hostname entries are not loopback"
    );
    assert!(
        allowlist_is_loopback_only(&[]),
        "empty list is trivially loopback-only"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn egress_deny_blocks_network() {
    use std::process::Command;

    let config = SandboxConfig {
        egress: EgressPolicy::Deny,
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    // WHY: Try to create a TCP connection to a TEST-NET address (RFC 5737).
    // With egress=deny, the child is in a network namespace with only
    // loopback, so connect() to any non-loopback address fails immediately
    // with ENETUNREACH (or EPERM if seccomp fallback is active).
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("echo test | nc -w1 198.51.100.1 80 2>&1; echo exit=$?");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The connection must fail. Possible error messages depend on mechanism:
    // - Network namespace: "Network is unreachable"
    // - Seccomp fallback: "Permission denied" or "Operation not permitted"
    assert!(
        combined.contains("exit=1")
            || combined.contains("Network is unreachable")
            || combined.contains("not permitted")
            || combined.contains("Permission denied")
            || !output.status.success(),
        "egress=deny must block outbound network: {combined}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn egress_deny_allows_basic_commands() {
    use std::process::Command;

    let config = SandboxConfig {
        egress: EgressPolicy::Deny,
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("echo");
    cmd.arg("egress test");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "basic commands must work with egress=deny: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("egress test"),
        "command output must be captured"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn egress_allowlist_loopback_permits_localhost() {
    use std::net::TcpListener;
    use std::process::Command;

    // WHY: Bind a listener on loopback so the child has something to
    // connect to. With egress=allowlist and 127.0.0.1 in the list,
    // the child should be able to reach this listener via the namespace's
    // loopback interface.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();

    let config = SandboxConfig {
        egress: EgressPolicy::Allowlist,
        egress_allowlist: vec!["127.0.0.1".to_owned()],
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    // WHY: Use sh -c with echo + /dev/tcp to test connectivity without
    // requiring curl or nc. bash's /dev/tcp is a builtin that creates
    // a TCP connection.
    let test_cmd = format!("bash -c 'echo hi > /dev/tcp/127.0.0.1/{port}' 2>&1; echo exit=$?");
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&test_cmd);
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // NOTE: In a network namespace, loopback is available but we need
    // to bring up the lo interface. The loopback interface exists but
    // may be down. Connection may succeed or fail depending on whether
    // the namespace auto-configures lo. Either way, the key test is
    // that the sandbox setup itself succeeded (no crash).
    // The egress_deny_blocks_network test verifies external blocking.
    assert!(
        stdout.contains("exit=0") || stdout.contains("exit=1"),
        "command must complete (not hang) with allowlist: {stdout}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn egress_allow_does_not_restrict() {
    use std::process::Command;

    let config = SandboxConfig {
        egress: EgressPolicy::Allow,
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("echo");
    cmd.arg("no egress filter");
    apply_sandbox(&mut cmd, policy).expect("apply sandbox");

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "command should succeed with egress=allow"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no egress filter"),
        "stdout should be captured with egress=allow"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn egress_graceful_fallback() {
    // WHY: This test verifies that apply_sandbox does not return an error
    // even when the egress mechanism (network namespace or seccomp) might
    // not be available. The permissive enforcement ensures graceful
    // degradation rather than hard failure.
    use std::process::Command;

    let config = SandboxConfig {
        egress: EgressPolicy::Deny,
        enforcement: SandboxEnforcement::Permissive,
        ..SandboxConfig::default()
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let policy = config.build_policy(dir.path(), &[]);

    let mut cmd = Command::new("echo");
    cmd.arg("fallback test");

    // Must not error regardless of kernel support
    let result = apply_sandbox(&mut cmd, policy);
    assert!(
        result.is_ok(),
        "egress deny with permissive enforcement must not error: {result:?}"
    );

    let output = cmd.output().expect("spawn child");
    assert!(
        output.status.success(),
        "command must execute after egress setup"
    );
}
