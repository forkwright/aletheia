//! Integration tests for Landlock sandbox fallback behavior.
//!
//! Exercises the public sandbox API against the running kernel to verify
//! ABI detection, permissive fallback, and strict enforcement error paths.

#[cfg(target_os = "linux")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod linux {
    use organon::sandbox::{
        SandboxConfig, SandboxConfigExt as _, SandboxEnforcement, apply_sandbox, probe_landlock_abi,
    };

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
                "ABI version must be at least 1 when Landlock is available"
            );
        }
    }

    /// Permissive mode must succeed and execute the tool regardless of whether
    /// the full Landlock baseline is available on the running kernel. This
    /// covers graceful degradation on absent or pre-v5 Landlock (#943).
    #[test]
    fn permissive_fallback_succeeds_regardless_of_landlock_availability() {
        let dir = tempfile::tempdir().expect("tempdir creation must succeed");
        let config = SandboxConfig {
            enabled: true,
            enforcement: SandboxEnforcement::Permissive,
            ..SandboxConfig::default()
        };
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = std::process::Command::new("echo");
        cmd.arg("permissive-fallback-ok");

        let result = apply_sandbox(&mut cmd, policy);
        assert!(
            result.is_ok(),
            "permissive mode must not error regardless of Landlock availability: {result:?}"
        );

        let output = cmd.output().expect("echo command must execute");
        assert!(
            output.status.success(),
            "tool must execute in permissive mode"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("permissive-fallback-ok"),
            "tool output must be captured"
        );
    }

    /// Strict enforcement must return a clear, named error when the full
    /// Landlock v5 filesystem-rights baseline is unavailable: never an opaque
    /// "Permission denied (os error 13)". At ABI v5+ the command executes.
    #[test]
    fn strict_enforcement_returns_clear_error_without_full_landlock_baseline() {
        let dir = tempfile::tempdir().expect("tempdir creation must succeed");
        let config = SandboxConfig {
            enabled: true,
            enforcement: SandboxEnforcement::Enforcing,
            ..SandboxConfig::default()
        };
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = std::process::Command::new("echo");
        cmd.arg("unreachable");

        if probe_landlock_abi().is_none_or(|abi| abi < 5) {
            let err = apply_sandbox(&mut cmd, policy)
                .expect_err("enforcing mode must fail without the full Landlock baseline");
            let msg = err.to_string();
            assert!(
                msg.contains("Landlock") || msg.contains("ABI"),
                "error must name Landlock or ABI rather than an opaque OS error: {msg}"
            );
            assert!(
                !msg.contains("Permission denied"),
                "opaque 'Permission denied' must not appear; error was: {msg}"
            );
        } else {
            let result = apply_sandbox(&mut cmd, policy);
            assert!(
                result.is_ok(),
                "enforcing mode must succeed with the full Landlock baseline: {result:?}"
            );
        }
    }
}
