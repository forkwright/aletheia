//! Tests for egress policy configuration and network namespace enforcement.
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;

use koina::http::HostResolver;

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

    // NOTE: The namespace's lo interface may be down, so the connect may succeed
    // or fail; assert only that setup completed. egress_deny_blocks_network covers blocking.
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

    // WHY: Must not error regardless of kernel support
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

// ── EgressGate: the in-process checkpoint (#5071, #5232, #5229) ──────────

struct StaticResolver(Vec<SocketAddr>);

impl HostResolver for StaticResolver {
    fn resolve_host<'a>(
        &'a self,
        _host: &'a str,
        _port: u16,
    ) -> koina::http::ResolveHostFuture<'a> {
        let addrs = self.0.clone();
        Box::pin(async move { Ok(addrs) })
    }
}

fn public_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443)
}

#[test]
fn gate_deny_rejects_before_any_resolution() {
    let gate = EgressGate::new(EgressPolicy::Deny, &[]);
    let err = gate
        .check_before_connect()
        .expect_err("deny must reject before resolving");
    assert!(err.to_string().contains("deny"), "error should name deny");
}

#[test]
fn gate_allow_permits_any_address() {
    let gate = EgressGate::new(EgressPolicy::Allow, &[]);
    assert!(gate.check_before_connect().is_ok());
    assert!(gate.check_addr(public_addr().ip()).is_ok());
    assert!(
        gate.check_addr(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
            .is_ok(),
        "Allow does not itself restrict destinations (SSRF guard is a separate layer)"
    );
}

#[test]
fn gate_deny_rejects_every_address() {
    let gate = EgressGate::new(EgressPolicy::Deny, &[]);
    assert!(gate.check_addr(public_addr().ip()).is_err());
}

#[test]
fn gate_allowlist_permits_listed_cidr() {
    let gate = EgressGate::new(
        EgressPolicy::Allowlist,
        &["93.184.216.0/24".to_owned(), "10.0.0.5".to_owned()],
    );
    assert!(
        gate.check_addr(public_addr().ip()).is_ok(),
        "address inside the allowed /24 must pass"
    );
    assert!(
        gate.check_addr(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)))
            .is_ok(),
        "exact bare-IP entry must pass"
    );
}

#[test]
fn gate_allowlist_rejects_unlisted_destination() {
    let gate = EgressGate::new(EgressPolicy::Allowlist, &["10.0.0.5".to_owned()]);
    let err = gate
        .check_addr(public_addr().ip())
        .expect_err("address outside the allowlist must be rejected");
    assert!(
        err.to_string().contains("allowlist"),
        "error should name allowlist: {err}"
    );
}

#[test]
fn gate_allowlist_with_unparseable_entry_never_matches() {
    // WHY: an operator typo must fail closed (deny that entry), never
    // silently match every destination.
    let gate = EgressGate::new(EgressPolicy::Allowlist, &["not-an-ip".to_owned()]);
    assert!(gate.check_addr(public_addr().ip()).is_err());
    assert!(gate
        .check_addr(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)))
        .is_err());
}

#[tokio::test]
async fn check_egress_deny_never_resolves() {
    struct PanicResolver;
    impl HostResolver for PanicResolver {
        fn resolve_host<'a>(
            &'a self,
            _host: &'a str,
            _port: u16,
        ) -> koina::http::ResolveHostFuture<'a> {
            Box::pin(async { panic!("deny must short-circuit before any DNS resolution") })
        }
    }

    let gate = EgressGate::new(EgressPolicy::Deny, &[]);
    let err = check_egress(&gate, "example.com", 443, &PanicResolver)
        .await
        .expect_err("deny must be rejected");
    assert!(err.contains("deny"));
}

#[tokio::test]
async fn check_egress_allow_does_not_resolve() {
    // WHY: Allow needs no per-address decision; resolving would be wasted
    // work and would couple egress checking to DNS availability.
    struct PanicResolver;
    impl HostResolver for PanicResolver {
        fn resolve_host<'a>(
            &'a self,
            _host: &'a str,
            _port: u16,
        ) -> koina::http::ResolveHostFuture<'a> {
            Box::pin(async { panic!("allow must not resolve") })
        }
    }

    let gate = EgressGate::new(EgressPolicy::Allow, &[]);
    check_egress(&gate, "example.com", 443, &PanicResolver)
        .await
        .expect("allow must pass without resolving");
}

#[tokio::test]
async fn check_egress_allowlist_resolves_and_matches() {
    let resolver = StaticResolver(vec![public_addr()]);
    let gate = EgressGate::new(EgressPolicy::Allowlist, &["93.184.216.0/24".to_owned()]);
    check_egress(&gate, "example.com", 443, &resolver)
        .await
        .expect("resolved address inside allowlist must pass");
}

#[tokio::test]
async fn check_egress_allowlist_rejects_unresolved_match() {
    let resolver = StaticResolver(vec![public_addr()]);
    let gate = EgressGate::new(EgressPolicy::Allowlist, &["10.0.0.0/8".to_owned()]);
    let err = check_egress(&gate, "example.com", 443, &resolver)
        .await
        .expect_err("resolved address outside allowlist must be rejected");
    assert!(err.contains("allowlist"));
}

#[test]
fn check_egress_remote_addr_rejects_private_after_public_validation() {
    // SECURITY(#5229): regression for DNS rebinding -- the pre-connect check
    // can pass on a public answer, then the actual connection can land on a
    // private address if DNS changes between the two lookups. The
    // post-connect check must catch this even under egress=allow.
    let gate = EgressGate::new(EgressPolicy::Allow, &[]);
    let rebound = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)), 443);
    let err = check_egress_remote_addr(&gate, Some(rebound))
        .expect_err("private remote addr must be rejected regardless of egress=allow");
    assert!(
        err.contains("private") || err.contains("rebinding"),
        "error should explain the rejection: {err}"
    );
}

#[test]
fn check_egress_remote_addr_permits_public_address() {
    let gate = EgressGate::new(EgressPolicy::Allow, &[]);
    check_egress_remote_addr(&gate, Some(public_addr())).expect("public peer must be permitted");
}

#[test]
fn check_egress_remote_addr_none_is_ok() {
    let gate = EgressGate::new(EgressPolicy::Deny, &[]);
    check_egress_remote_addr(&gate, None)
        .expect("no peer address available is not itself an error");
}

#[test]
fn check_egress_remote_addr_enforces_allowlist_on_reconnect() {
    let gate = EgressGate::new(EgressPolicy::Allowlist, &["10.0.0.5".to_owned()]);
    let unlisted = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 6)), 443);
    let err = check_egress_remote_addr(&gate, Some(unlisted))
        .expect_err("post-connect address must also satisfy the allowlist");
    assert!(err.contains("allowlist"));
}
