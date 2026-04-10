//! Fork the server to a background process.

use std::path::PathBuf;

use anyhow::{Context, Result};

use koina::system::{Environment, RealSystem};

/// Fork the server to background by re-executing the binary without `--daemon`.
///
/// WHY: `fork()` is unsafe inside a running tokio multi-thread runtime. Re-executing
/// the binary avoids that hazard while still detaching from the terminal.
pub(crate) async fn do_daemon() -> Result<()> {
    let env = RealSystem;
    let exe = env.current_exe().context("failed to locate executable")?;

    let child_args: Vec<String> = env
        .args()
        .into_iter()
        .skip(1)
        .filter(|a| a != "--daemon")
        .collect();

    // WHY: Redirect stderr to a crash log so daemon startup failures are
    // visible. Previously stdout+stderr were both /dev/null, so if the child
    // crashed (e.g., schema version mismatch), the error was lost entirely.
    let instance_root = daemon_instance_root(&env);
    tokio::fs::create_dir_all(&instance_root)
        .await
        .with_context(|| format!("failed to create {}", instance_root.display()))?;
    let crash_log_path = instance_root.join("logs").join("daemon-stderr.log");
    if let Some(parent) = crash_log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stderr_file = std::fs::File::create(&crash_log_path)
        .context("failed to create daemon stderr log")?;

    let child = std::process::Command::new(&exe)
        .args(&child_args)
        .env("_ALETHEIA_DAEMON", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(stderr_file))
        .spawn()
        .context("failed to spawn background process")?;

    let pid = child.id();

    // NOTE: instance_root already created above (before spawn, for stderr log).
    let pid_path = instance_root.join("aletheia.pid");
    tokio::fs::write(&pid_path, pid.to_string())
        .await
        .with_context(|| format!("failed to write PID file at {}", pid_path.display()))?;

    println!(
        "aletheia started in background (PID: {pid}, PID file: {})",
        pid_path.display()
    );
    Ok(())
}

/// Resolve the instance root from CLI args or environment for PID file placement.
fn daemon_instance_root(env: &impl Environment) -> PathBuf {
    let args = env.args();
    for (i, arg) in args.iter().enumerate() {
        if arg == "-r" || arg == "--instance-root" {
            if let Some(path) = args.get(i + 1) {
                return PathBuf::from(path);
            }
        } else if let Some(path) = arg.strip_prefix("--instance-root=") {
            return PathBuf::from(path);
        }
    }
    env.var("ALETHEIA_ROOT")
        .map_or_else(|| PathBuf::from("instance"), PathBuf::from)
}
