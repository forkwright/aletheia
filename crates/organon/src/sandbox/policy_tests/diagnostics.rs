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
