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
fn landlock_active_requires_every_v5_filesystem_right() {
    use landlock::{ABI, Access, AccessFs, RulesetStatus};

    // IoctlDev is a right this policy actually requests for /dev. ABI v4
    // cannot represent it; ABI v5 can. This pins the mechanism behind the
    // admission threshold rather than merely fabricating a status enum.
    assert!(!AccessFs::from_all(ABI::V4).contains(AccessFs::IoctlDev));
    assert!(AccessFs::from_all(ABI::V5).contains(AccessFs::IoctlDev));

    assert_eq!(
        landlock_guarantee_status(
            Some(REQUIRED_LANDLOCK_ABI - 1),
            SandboxEnforcement::Enforcing
        ),
        GuaranteeStatus::Unavailable
    );
    assert_eq!(
        landlock_guarantee_status(
            Some(REQUIRED_LANDLOCK_ABI - 1),
            SandboxEnforcement::Permissive
        ),
        GuaranteeStatus::Degraded
    );
    assert_eq!(
        landlock_guarantee_status(Some(REQUIRED_LANDLOCK_ABI), SandboxEnforcement::Enforcing),
        GuaranteeStatus::Active
    );
    assert_eq!(
        landlock_guarantee_status(None, SandboxEnforcement::Enforcing),
        GuaranteeStatus::Unavailable
    );
    assert_eq!(
        landlock_guarantee_status(
            Some(REQUIRED_LANDLOCK_ABI + 1),
            SandboxEnforcement::Permissive
        ),
        GuaranteeStatus::Active
    );

    assert!(
        require_full_landlock_status(
            RulesetStatus::PartiallyEnforced,
            SandboxEnforcement::Enforcing
        )
        .is_err(),
        "child-side partial enforcement must fail closed even after preflight"
    );
    assert!(
        require_full_landlock_status(
            RulesetStatus::PartiallyEnforced,
            SandboxEnforcement::Permissive
        )
        .is_ok(),
        "permissive mode may continue but is classified degraded by preflight"
    );
    assert!(
        require_full_landlock_status(RulesetStatus::FullyEnforced, SandboxEnforcement::Enforcing)
            .is_ok(),
        "a fully enforced ruleset satisfies the child-side gate"
    );
    assert!(
        require_full_landlock_status(RulesetStatus::NotEnforced, SandboxEnforcement::Enforcing)
            .is_err(),
        "enforcing mode must reject a ruleset that was not installed"
    );
    assert!(
        require_full_landlock_status(RulesetStatus::NotEnforced, SandboxEnforcement::Permissive)
            .is_ok(),
        "permissive mode may continue without a child ruleset"
    );
}

#[test]
fn probe_guarantees_reflects_landlock_probe() {
    let full_landlock_baseline =
        probe_landlock_abi().is_some_and(|abi| abi >= REQUIRED_LANDLOCK_ABI);

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
    if full_landlock_baseline {
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
    if full_landlock_baseline {
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

/// The diagnostic entry point must not depend on which workspace it is asked about.
///
/// WHY(#5232) this is a test and not a comment: `diagnostic_guarantees` passes a
/// placeholder path to `build_policy`, which is correct only because the classification
/// reads `enforcement`, `egress` and `egress_allowlist` and never the filesystem. That
/// is a property of code elsewhere, and nothing re-checks it -- exactly the shape of
/// assumption that goes stale silently. If a future change makes the classification
/// path-sensitive, this fails here, rather than the health endpoint quietly reporting a
/// verdict about a directory nobody asked about.
#[test]
fn diagnostic_guarantees_do_not_depend_on_the_workspace() {
    for config in [SandboxConfig::default(), SandboxConfig::disabled()] {
        let from_root = probe_guarantees(&config.build_policy(std::path::Path::new("/"), &[]));
        let from_elsewhere = probe_guarantees(
            &config.build_policy(std::path::Path::new("/nonexistent/workspace"), &[]),
        );
        let via_entry_point = diagnostic_guarantees(&config);

        assert_eq!(
            (from_root.landlock, from_root.seccomp, from_root.egress),
            (
                from_elsewhere.landlock,
                from_elsewhere.seccomp,
                from_elsewhere.egress
            ),
            "the guarantee classification must not vary with the workspace path"
        );
        assert_eq!(
            (
                via_entry_point.landlock,
                via_entry_point.seccomp,
                via_entry_point.egress
            ),
            (from_root.landlock, from_root.seccomp, from_root.egress),
            "diagnostic_guarantees must agree with probe_guarantees"
        );
    }
}
