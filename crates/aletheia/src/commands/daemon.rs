//! Fork the server to a background process.

use std::path::PathBuf;

use aletheia_taxis::oikos::Oikos;
use anyhow::{Context, Result};

/// Fork the server to background by re-executing the binary without `--daemon`.
///
/// WHY: `fork()` is unsafe inside a running tokio multi-thread runtime. Re-executing
/// the binary avoids that hazard while still detaching from the terminal.
pub(crate) async fn do_daemon() -> Result<()> {
    let exe = std::env::current_exe().context("failed to locate executable")?;

    let child_args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--daemon")
        .collect();

    // WHY: Redirect stderr to a crash log so daemon startup failures are
    // visible. Previously stdout+stderr were both /dev/null, so if the child
    // crashed (e.g., schema version mismatch), the error was lost entirely.
    let instance_root = daemon_instance_root();
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

/// Resolve the instance root for PID file placement.
///
/// Uses Oikos discovery which checks ALETHEIA_ROOT env var and falls back to
/// ./instance.
fn daemon_instance_root() -> PathBuf {
    Oikos::discover().root().to_path_buf()
}
