//! `aletheia service`: generate/install/verify a systemd user service unit
//! from resolved install paths.
//!
//! WHY(#5096): the checked-in `instance.example/services/aletheia.service`
//! hardcodes `%h/aletheia/instance` and `%h/.local/bin/aletheia` across four
//! independently-spelled directives (`ExecStart`, `EnvironmentFile`,
//! `ReadWritePaths`, `WorkingDirectory`) that must all stay in step with the
//! same underlying install layout. An operator with a non-default layout has
//! to hand-edit all four in lockstep, and a missed one produces a unit that
//! starts but cannot write its own data directory. This module derives every
//! path-bearing directive from the same resolved [`Oikos`] instance root and
//! binary path, so there is exactly one place those paths come from.
//!
//! Scope: systemd `--user` units only. launchd generation is tracked
//! separately (#5096 desired-outcome bullet); `--systemd-user` is required on
//! every action so the flag has a stable meaning to extend later rather than
//! defaulting to a target that stops being the only one.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use snafu::prelude::*;

use taxis::oikos::Oikos;

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Print the generated unit to stdout
    Print(ServiceArgs),
    /// Write the generated unit into the systemd user unit directory
    Install(ServiceArgs),
    /// Validate the generated unit with `systemd-analyze verify`
    Verify(ServiceArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ServiceArgs {
    /// Generate a systemd `--user` unit. Currently the only supported
    /// target; required so the flag keeps a stable meaning once other
    /// targets (e.g. launchd) exist rather than silently picking one.
    #[arg(long)]
    pub systemd_user: bool,

    /// Path to the aletheia binary the unit should exec.
    /// Defaults to the currently running executable's own path.
    #[arg(long)]
    pub binary: Option<PathBuf>,

    /// `install` only: overwrite an existing installed unit file.
    #[arg(long)]
    pub force: bool,
}

/// Resolved inputs to [`generate_unit`].
///
/// Kept separate from [`ServiceArgs`] so the generator is a pure function of
/// already-resolved paths, testable without CLI parsing or process state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceUnitSpec {
    /// Absolute path to the aletheia binary `ExecStart` should invoke.
    pub binary: PathBuf,
    /// Absolute instance root: source of `-r`, `ReadWritePaths`, and
    /// (joined with `config/env`) `EnvironmentFile`.
    pub instance_root: PathBuf,
    /// Absolute layout root (the instance root's parent) used as
    /// `WorkingDirectory`. Kept distinct from `instance_root` -- not because
    /// anything in the unit is itself cwd-relative, but because
    /// `DriftDetectionConfig::default()` resolves the sibling
    /// `instance.example` template relative to the process cwd (see the WHY
    /// on [`generate_unit`]).
    pub working_directory: PathBuf,
}

impl ServiceUnitSpec {
    fn env_file(&self) -> PathBuf {
        self.instance_root.join("config").join("env")
    }
}

/// Render the systemd `--user` unit text for `spec`.
///
/// Every path-bearing directive (`ExecStart`, `EnvironmentFile`,
/// `ReadWritePaths`, `WorkingDirectory`) is derived from the same
/// [`ServiceUnitSpec`], so a hardening path cannot drift from the path the
/// binary is actually told to use the way the hand-edited template could.
// WHY: Type=notify + WatchdogSec lets the binary signal READY=1 once the HTTP
// gateway is accepting connections and send WATCHDOG=1 heartbeats; two missed
// heartbeats trigger an automatic restart. Ported unchanged from the
// hand-written template (#5096 is about path derivation, not this contract).
//
// WHY: EnvironmentFile's leading `-` makes a missing env file non-fatal,
// matching the hand-written template's contract for a fresh instance with no
// credentials written yet.
//
// WHY(#5096): WorkingDirectory is the instance root's *parent* (the layout
// root), matching the hand-written template's cwd contract -- not because
// any directive in this unit is itself cwd-relative (every path here is
// already resolved and absolute), but because
// `DriftDetectionConfig::default()` (crates/daemon/src/maintenance/drift_detection.rs)
// resolves the sibling template `instance.example` relative to the running
// process's *cwd*. Setting WorkingDirectory to the instance root itself
// would silently break drift detection (`template_available=false`) the
// moment the service starts, because it would look for
// `<instance_root>/instance.example` instead of the actual sibling.
#[must_use]
pub(crate) fn generate_unit(spec: &ServiceUnitSpec) -> String {
    let binary = spec.binary.display();
    let root = spec.instance_root.display();
    let working_directory = spec.working_directory.display();
    let env_file = spec.env_file();
    let env_file = env_file.display();

    format!(
        "[Unit]\n\
         Description=Aletheia AI Gateway\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         # Managed by `aletheia service print/install --systemd-user`;\n\
         # re-run the command instead of hand-editing this file.\n\
         Type=notify\n\
         WatchdogSec=30\n\
         Environment=RUST_BACKTRACE=1\n\
         EnvironmentFile=-{env_file}\n\
         ExecStart={binary} -r {root}\n\
         WorkingDirectory={working_directory}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         StandardOutput=journal\n\
         StandardError=journal\n\
         LimitNOFILE=65536\n\
         MemoryHigh=6G\n\
         MemoryMax=8G\n\
         ProtectSystem=strict\n\
         ReadWritePaths={root}\n\
         ProtectHome=read-only\n\
         PrivateTmp=true\n\
         NoNewPrivileges=true\n\
         RestrictSUIDSGID=true\n\
         SystemCallFilter=@system-service\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

/// Resolve a [`ServiceUnitSpec`] from CLI args and the global `-r` flag.
///
/// # Errors
///
/// Fails if `--systemd-user` was not passed (the only supported target, kept
/// explicit rather than assumed); if `--binary` was given but does not exist,
/// is unreadable, or canonicalizes to a non-absolute path; if `--binary` was
/// omitted and the current executable's path cannot be determined; or if the
/// resolved instance root has no parent directory to derive a systemd
/// `WorkingDirectory` (layout root) from.
fn resolve_spec(args: &ServiceArgs, instance_root: Option<&PathBuf>) -> Result<ServiceUnitSpec> {
    if !args.systemd_user {
        whatever!(
            "aletheia service: pass --systemd-user (the only supported unit \
             target today; launchd generation is not yet implemented, see #5096)"
        );
    }

    // WHY: `Oikos::from_root` stores a not-yet-existing path as-is (only
    // canonicalizing when the directory already exists), so generating a
    // unit ahead of `aletheia init` still resolves a stable absolute path.
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let instance_root = oikos.root().to_path_buf();

    // WHY(#5096): see the WHY on `generate_unit` -- WorkingDirectory must be
    // the layout root (the instance root's parent), not the instance root
    // itself, or drift detection's cwd-relative `instance.example` lookup
    // breaks. Fail loud rather than falling back to the instance root: a
    // silent fallback here is exactly the bug this derivation exists to
    // avoid.
    let working_directory = instance_root
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            crate::error::Error::msg(format!(
                "aletheia service: instance root {} has no parent directory; \
             cannot derive a systemd WorkingDirectory (layout root) for \
             drift detection's sibling instance.example template",
                instance_root.display()
            ))
        })?;

    // WHY(#5096): `--binary` must resolve to a real, absolute, executable
    // path -- an unresolvable or relative ExecStart target is a unit that
    // `install` would happily write and `systemctl start` would only fail on
    // later, so this fails loud instead of falling back to the caller's
    // (possibly relative, possibly nonexistent) input.
    let binary = match &args.binary {
        Some(path) => {
            let canonical = std::fs::canonicalize(path).with_whatever_context(|_| {
                format!(
                    "aletheia service: --binary {} does not exist or is not \
                     readable (ExecStart requires an absolute, existing path)",
                    path.display()
                )
            })?;
            if !canonical.is_absolute() {
                whatever!(
                    "aletheia service: --binary {} resolved to a non-absolute \
                     path {} (ExecStart requires an absolute path)",
                    path.display(),
                    canonical.display()
                );
            }
            canonical
        }
        None => std::env::current_exe()
            .whatever_context("failed to resolve the current executable's path")?,
    };

    Ok(ServiceUnitSpec {
        binary,
        instance_root,
        working_directory,
    })
}

/// Resolve the systemd user unit directory: `$XDG_CONFIG_HOME/systemd/user`,
/// falling back to `$HOME/.config/systemd/user`.
///
/// A pure function of the two candidate env var values (rather than reading
/// them itself) so it is directly testable without mutating process env vars.
///
/// # Errors
///
/// Fails if neither is set -- there is no silent third fallback location for
/// a per-user unit directory.
fn systemd_user_unit_dir(xdg_config_home: Option<&str>, home: Option<&str>) -> Result<PathBuf> {
    if let Some(dir) = xdg_config_home.filter(|d| !d.trim().is_empty()) {
        return Ok(PathBuf::from(dir).join("systemd").join("user"));
    }
    if let Some(home) = home.filter(|d| !d.trim().is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join("systemd")
            .join("user"));
    }
    whatever!(
        "aletheia service install: neither XDG_CONFIG_HOME nor HOME is set; \
         cannot resolve the systemd user unit directory"
    );
}

/// Refuse to overwrite an existing installed unit unless `force` is set.
///
/// Pure function of the destination path and the `--force` flag so it is
/// directly testable without touching the real systemd user unit directory.
///
/// # Errors
///
/// Fails if `dest` already exists and `force` is `false`.
fn ensure_install_destination(dest: &Path, force: bool) -> Result<()> {
    if dest.exists() && !force {
        whatever!(
            "aletheia service install: {} already exists (pass --force to overwrite)",
            dest.display()
        );
    }
    Ok(())
}

pub(crate) async fn run(action: &Action, instance_root: Option<&PathBuf>) -> Result<()> {
    match action {
        Action::Print(args) => {
            let spec = resolve_spec(args, instance_root)?;
            print!("{}", generate_unit(&spec));
            Ok(())
        }
        Action::Install(args) => run_install(args, instance_root).await,
        Action::Verify(args) => {
            let spec = resolve_spec(args, instance_root)?;
            verify_with_systemd_analyze(&generate_unit(&spec))
        }
    }
}

async fn run_install(args: &ServiceArgs, instance_root: Option<&PathBuf>) -> Result<()> {
    let spec = resolve_spec(args, instance_root)?;
    let unit = generate_unit(&spec);
    let dir = systemd_user_unit_dir(
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )?;
    let dest = dir.join("aletheia.service");
    ensure_install_destination(&dest, args.force)?;

    // WHY: `std::fs::write` is disallowed in this crate (crates/aletheia/clippy.toml)
    // in favor of `tokio::fs` -- this handler is async so the write does not block
    // the runtime the way the other CLI commands' synchronous fs calls would.
    tokio::fs::create_dir_all(&dir)
        .await
        .whatever_context(format!("failed to create {}", dir.display()))?;
    tokio::fs::write(&dest, &unit)
        .await
        .whatever_context(format!("failed to write {}", dest.display()))?;

    println!("installed {}", dest.display());
    println!("Next steps:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable --now aletheia");
    println!("  loginctl enable-linger   # persist across logout");
    Ok(())
}

/// Write `unit_text` to a temp file and validate it with
/// `systemd-analyze verify`, streaming that command's own diagnostics
/// (which name specific directives) to the operator on failure.
fn verify_with_systemd_analyze(unit_text: &str) -> Result<()> {
    let mut file = tempfile::Builder::new()
        .prefix("aletheia-")
        .suffix(".service")
        .tempfile()
        .whatever_context("failed to create a temp file for `systemd-analyze verify`")?;
    file.write_all(unit_text.as_bytes())
        .whatever_context("failed to write the generated unit to a temp file")?;
    file.flush()
        .whatever_context("failed to flush the generated unit to a temp file")?;

    let output = std::process::Command::new("systemd-analyze")
        .arg("verify")
        .arg(file.path())
        .output()
        .whatever_context(
            "failed to run `systemd-analyze verify` -- is systemd installed on this host?",
        )?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        whatever!("systemd-analyze verify failed for the generated unit (see output above)");
    }
    println!("systemd-analyze verify: OK");
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test setup requires synchronous filesystem access"
)]
mod tests {
    use super::*;

    /// Build a spec directly (bypassing `resolve_spec`), deriving
    /// `working_directory` from `root`'s parent the same way `resolve_spec`
    /// does, falling back to `root` itself for the degenerate root-less
    /// paths a couple of these tests use.
    fn spec(binary: &str, root: &str) -> ServiceUnitSpec {
        let instance_root = PathBuf::from(root);
        let working_directory = instance_root
            .parent()
            .map_or_else(|| instance_root.clone(), Path::to_path_buf);
        ServiceUnitSpec {
            binary: PathBuf::from(binary),
            instance_root,
            working_directory,
        }
    }

    // ── generator ────────────────────────────────────────────────────────

    #[test]
    fn generate_unit_derives_every_path_directive_from_the_spec() {
        let unit = generate_unit(&spec(
            "/opt/aletheia/bin/aletheia",
            "/srv/aletheia/instance",
        ));

        assert!(
            unit.contains("ExecStart=/opt/aletheia/bin/aletheia -r /srv/aletheia/instance"),
            "ExecStart should use the resolved binary and instance root:\n{unit}"
        );
        assert!(
            unit.contains("EnvironmentFile=-/srv/aletheia/instance/config/env"),
            "EnvironmentFile should be under the resolved instance root:\n{unit}"
        );
        assert!(
            unit.contains("ReadWritePaths=/srv/aletheia/instance"),
            "ReadWritePaths should be the resolved instance root:\n{unit}"
        );
        assert!(
            unit.contains("WorkingDirectory=/srv/aletheia\n"),
            "WorkingDirectory should be the instance root's parent (the \
             layout root), not the instance root itself -- drift detection's \
             sibling instance.example lookup depends on this:\n{unit}"
        );
    }

    #[test]
    fn generate_unit_never_leaves_a_systemd_specifier_in_a_resolved_path() {
        // WHY(#5096): the whole point is that every path is already resolved
        // -- a leftover `%h` would mean a directive silently fell back to the
        // hand-written template's assumption instead of the given spec.
        let unit = generate_unit(&spec(
            "/home/op/.local/bin/aletheia",
            "/home/op/aletheia/instance",
        ));
        assert!(
            !unit.contains('%'),
            "generated unit must not contain systemd specifiers:\n{unit}"
        );
    }

    #[test]
    fn generate_unit_keeps_the_hardening_contract() {
        let unit = generate_unit(&spec("/bin/aletheia", "/instance"));
        for directive in [
            "Type=notify",
            "ProtectSystem=strict",
            "ProtectHome=read-only",
            "NoNewPrivileges=true",
            "PrivateTmp=true",
        ] {
            assert!(unit.contains(directive), "missing `{directive}`:\n{unit}");
        }
    }

    // ── flag / resolution ───────────────────────────────────────────────

    #[test]
    fn resolve_spec_fails_loud_without_systemd_user_flag() {
        let args = ServiceArgs {
            systemd_user: false,
            binary: Some(PathBuf::from("/bin/aletheia")),
            force: false,
        };
        let err = resolve_spec(&args, None).unwrap_err();
        assert!(err.to_string().contains("--systemd-user"));
    }

    #[test]
    fn resolve_spec_uses_explicit_instance_root_and_binary() {
        let dir = tempfile::tempdir().unwrap();
        // WHY: `resolve_spec` now canonicalizes `--binary` and fails loud if
        // that fails, so this test needs a binary path that actually exists
        // rather than the placeholder `/bin/aletheia` the fallback-behavior
        // version of this test used to accept unchecked.
        let binary_dir = tempfile::tempdir().unwrap();
        let binary_path = binary_dir.path().join("aletheia");
        std::fs::write(&binary_path, b"").unwrap();
        let canonical_binary = std::fs::canonicalize(&binary_path).unwrap();

        let args = ServiceArgs {
            systemd_user: true,
            binary: Some(binary_path),
            force: false,
        };
        let root = dir.path().to_path_buf();
        let resolved = resolve_spec(&args, Some(&root)).unwrap();
        assert_eq!(resolved.instance_root, root);
        assert_eq!(resolved.binary, canonical_binary);
    }

    #[test]
    fn resolve_spec_fails_loud_on_relative_or_missing_binary() {
        let dir = tempfile::tempdir().unwrap();
        let args = ServiceArgs {
            systemd_user: true,
            binary: Some(PathBuf::from("./target/does-not-exist-5096")),
            force: false,
        };
        let root = dir.path().to_path_buf();
        let err = resolve_spec(&args, Some(&root)).unwrap_err();
        assert!(
            err.to_string().contains("does-not-exist-5096"),
            "expected the error to name the offending --binary path: {err}"
        );
    }

    #[test]
    fn systemd_user_unit_dir_prefers_xdg_config_home() {
        let dir = systemd_user_unit_dir(Some("/home/op/.config"), Some("/home/op")).unwrap();
        assert_eq!(dir, PathBuf::from("/home/op/.config/systemd/user"));
    }

    #[test]
    fn systemd_user_unit_dir_falls_back_to_home() {
        let dir = systemd_user_unit_dir(None, Some("/home/op")).unwrap();
        assert_eq!(dir, PathBuf::from("/home/op/.config/systemd/user"));
    }

    #[test]
    fn systemd_user_unit_dir_fails_loud_with_neither_var_set() {
        let err = systemd_user_unit_dir(None, None).unwrap_err();
        assert!(err.to_string().contains("XDG_CONFIG_HOME"));
    }

    // ── systemd-analyze verify ───────────────────────────────────────────

    /// `true` if `systemd-analyze` is runnable on this host, so the
    /// validator test degrades to a skip (with a printed reason) rather than
    /// a spurious CI failure on a host without systemd installed.
    fn systemd_analyze_available() -> bool {
        std::process::Command::new("systemd-analyze")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn generated_unit_passes_systemd_analyze_verify() {
        if !systemd_analyze_available() {
            eprintln!("skipping: systemd-analyze not available on this host");
            return;
        }

        // WHY: `systemd-analyze verify` checks that ExecStart's binary
        // actually exists and is executable, so the spec must point at a
        // real file -- the test binary's own path always exists.
        let real_binary = std::env::current_exe().unwrap();
        let unit = generate_unit(&spec(
            real_binary.to_str().unwrap(),
            "/tmp/aletheia-service-unit-test-instance",
        ));

        verify_with_systemd_analyze(&unit).expect(
            "generated unit should pass `systemd-analyze verify` \
             (see stdout/stderr printed above for the specific directive)",
        );
    }

    #[test]
    fn verify_with_systemd_analyze_fails_loud_on_rejected_unit() {
        if !systemd_analyze_available() {
            eprintln!("skipping: systemd-analyze not available on this host");
            return;
        }

        // WHY: a nonexistent ExecStart target is something systemd-analyze
        // verify actively rejects (as opposed to merely warning about), so
        // this exercises the fail-loud path deterministically rather than
        // relying on some other directive systemd may or may not enforce.
        let unit = generate_unit(&spec(
            "/nonexistent/aletheia",
            "/tmp/aletheia-service-unit-test-instance",
        ));

        let err = verify_with_systemd_analyze(&unit).unwrap_err();
        assert!(
            err.to_string().contains("systemd-analyze verify failed"),
            "expected a fail-loud error naming systemd-analyze verify: {err}"
        );
    }

    // ── install destination ──────────────────────────────────────────────

    #[test]
    fn ensure_install_destination_refuses_an_existing_dest_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("aletheia.service");
        std::fs::write(&dest, "existing unit").unwrap();

        let err = ensure_install_destination(&dest, false).unwrap_err();
        assert!(
            err.to_string().contains("--force"),
            "expected the error to mention --force: {err}"
        );
    }

    #[test]
    fn ensure_install_destination_allows_an_existing_dest_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("aletheia.service");
        std::fs::write(&dest, "existing unit").unwrap();

        ensure_install_destination(&dest, true).expect("--force should permit overwrite");
    }

    #[test]
    fn ensure_install_destination_allows_a_dest_that_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("aletheia.service");

        ensure_install_destination(&dest, false).expect("a fresh install needs no --force");
    }
}
