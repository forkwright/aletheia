//! Sandbox diagnostics and guarantee-status tests.

use super::super::*;

#[test]
fn guarantee_status_display_is_lowercase_ascii() {
    assert_eq!(GuaranteeStatus::Active.to_string(), "active");
    assert_eq!(GuaranteeStatus::Degraded.to_string(), "degraded");
    assert_eq!(GuaranteeStatus::Unavailable.to_string(), "unavailable");
    assert_eq!(GuaranteeStatus::Unrestricted.to_string(), "unrestricted");
}

#[test]
fn probe_guarantees_reflects_landlock_probe() {
    let landlock_available = probe_landlock_abi().is_some();

    let enforcing_policy = SandboxPolicy {
        enabled: true,
        read_paths: Vec::new(),
        write_paths: Vec::new(),
        exec_paths: Vec::new(),
        enforcement: SandboxEnforcement::Enforcing,
        egress: EgressPolicy::Allow,
        egress_allowlist: Vec::new(),
    };
    let guarantees = probe_guarantees(&enforcing_policy);
    if landlock_available {
        assert_eq!(guarantees.landlock, GuaranteeStatus::Active);
    } else {
        assert_eq!(guarantees.landlock, GuaranteeStatus::Unavailable);
    }
    assert_eq!(guarantees.seccomp, GuaranteeStatus::Active);
    assert_eq!(guarantees.egress, GuaranteeStatus::Unrestricted);

    let permissive_policy = SandboxPolicy {
        enforcement: SandboxEnforcement::Permissive,
        ..enforcing_policy.clone()
    };
    let guarantees = probe_guarantees(&permissive_policy);
    if landlock_available {
        assert_eq!(guarantees.landlock, GuaranteeStatus::Active);
    } else {
        assert_eq!(guarantees.landlock, GuaranteeStatus::Degraded);
    }
}

#[test]
fn probe_guarantees_reflects_egress_policy() {
    let base = SandboxPolicy {
        enabled: true,
        read_paths: Vec::new(),
        write_paths: Vec::new(),
        exec_paths: Vec::new(),
        enforcement: SandboxEnforcement::Enforcing,
        egress: EgressPolicy::Deny,
        egress_allowlist: Vec::new(),
    };

    assert_eq!(probe_guarantees(&base).egress, GuaranteeStatus::Active);

    let mut allowlist = base.clone();
    allowlist.egress = EgressPolicy::Allowlist;
    assert_eq!(probe_guarantees(&allowlist).egress, GuaranteeStatus::Active);

    let mut allow = base.clone();
    allow.egress = EgressPolicy::Allow;
    assert_eq!(
        probe_guarantees(&allow).egress,
        GuaranteeStatus::Unrestricted
    );
}

#[test]
fn probe_guarantees_reflects_unenforceable_allowlist() {
    // SECURITY(#4997): regression test. The previous `_ => seccomp` match reported
    // `egress = "active"` for ANY non-Allow policy, including an Allowlist
    // with non-loopback entries -- `apply_egress` can only isolate a child
    // to loopback or block it entirely, so those entries could never be
    // reached even though the diagnostic claimed the guarantee was active.
    // This pins the honest status: unenforceable under enforcing, degraded
    // under permissive, never "active".
    let enforcing = SandboxPolicy {
        enabled: true,
        read_paths: Vec::new(),
        write_paths: Vec::new(),
        exec_paths: Vec::new(),
        enforcement: SandboxEnforcement::Enforcing,
        egress: EgressPolicy::Allowlist,
        egress_allowlist: vec!["93.184.216.34".to_owned()],
    };
    assert_eq!(
        probe_guarantees(&enforcing).egress,
        GuaranteeStatus::Unavailable,
        "a non-loopback allowlist entry must not be reported as enforced under enforcing"
    );

    let permissive = SandboxPolicy {
        enforcement: SandboxEnforcement::Permissive,
        ..enforcing.clone()
    };
    assert_eq!(
        probe_guarantees(&permissive).egress,
        GuaranteeStatus::Degraded,
        "permissive mode degrades rather than blocks, but must not claim active either"
    );

    // A loopback-only allowlist IS within what apply_egress can provide (see
    // allowlist_is_loopback_only), so it keeps tracking the same seccomp/
    // netns capability status `deny` does -- this must NOT regress to
    // Unavailable/Degraded alongside the non-loopback case above.
    let loopback_only = SandboxPolicy {
        egress_allowlist: vec!["127.0.0.1".to_owned()],
        ..enforcing.clone()
    };
    assert_eq!(
        probe_guarantees(&loopback_only).egress,
        GuaranteeStatus::Active,
        "a loopback-only allowlist entry is within the mechanism's real capability"
    );
}
