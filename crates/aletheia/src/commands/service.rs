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
use std::path::PathBuf;

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
    /// Absolute instance root: source of `-r`, `ReadWritePaths`,
    /// `WorkingDirectory`, and (joined with `config/env`) `EnvironmentFile`.
    pub instance_root: PathBuf,
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
// WHY: WorkingDirectory is the instance root itself rather than a sibling
// "layout root" the way the hand-written template used it -- every path here
// is already resolved and absolute, so nothing in this unit depends on the
// process cwd to find a relative sibling.
#[must_use]
pub(crate) fn generate_unit(spec: &ServiceUnitSpec) -> String {
    let binary = spec.binary.display();
    let root = spec.instance_root.display();
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
         WorkingDirectory={root}\n\
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
/// explicit rather than assumed) or if `--binary` was omitted and the current
/// executable's path cannot be determined.
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

    let binary = match &args.binary {
        Some(path) => std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()),
        None => std::env::current_exe()
            .whatever_context("failed to resolve the current executable's path")?,
    };

    Ok(ServiceUnitSpec {
        binary,
        instance_root: oikos.root().to_path_buf(),
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

    if dest.exists() && !args.force {
        whatever!(
            "aletheia service install: {} already exists (pass --force to overwrite)",
            dest.display()
        );
    }

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
mod tests {
    use super::*;

    fn spec(binary: &str, root: &str) -> ServiceUnitSpec {
        ServiceUnitSpec {
            binary: PathBuf::from(binary),
            instance_root: PathBuf::from(root),
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
            unit.contains("WorkingDirectory=/srv/aletheia/instance"),
            "WorkingDirectory should be the resolved instance root:\n{unit}"
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
        let args = ServiceArgs {
            systemd_user: true,
            binary: Some(PathBuf::from("/bin/aletheia")),
            force: false,
        };
        let root = dir.path().to_path_buf();
        let resolved = resolve_spec(&args, Some(&root)).unwrap();
        assert_eq!(resolved.instance_root, root);
        assert_eq!(resolved.binary, PathBuf::from("/bin/aletheia"));
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
}
