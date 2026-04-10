//! Systemd notify integration for daemon lifecycle signaling.

use std::time::Duration;

use koina::system::{Environment, RealSystem};

/// Send `READY=1` to systemd via the `$NOTIFY_SOCKET`.
///
/// WHY: systemd `Type=notify` services need this to know initialization is
/// complete. No-op if `$NOTIFY_SOCKET` is not SET.
pub(super) fn sd_notify_ready() {
    sd_notify("READY=1");
}

/// Send `WATCHDOG=1` to systemd.
///
/// WHY: `WatchdogSec` integration enables automatic restart on hang.
pub(super) fn sd_notify_watchdog() {
    sd_notify("WATCHDOG=1");
}

/// Send `STOPPING=1` to systemd before shutdown cleanup.
pub(super) fn sd_notify_stopping() {
    sd_notify("STOPPING=1");
}

/// Parse `$WATCHDOG_USEC` to determine the systemd watchdog interval.
///
/// Returns `None` if the variable is not SET or unparseable. The recommended
/// notification interval is half the watchdog timeout.
pub(super) fn sd_watchdog_interval() -> Option<Duration> {
    let usec_str = RealSystem.var("WATCHDOG_USEC")?;
    let usec: u64 = usec_str.parse().ok()?;
    // WHY: notify at half the watchdog interval to avoid races.
    Some(Duration::from_micros(usec / 2))
}

/// Low-level `sd_notify`: write a message to `$NOTIFY_SOCKET` (Unix datagram).
///
/// No-op on non-Unix platforms or when `$NOTIFY_SOCKET` is not SET.
#[cfg(unix)]
fn sd_notify(msg: &str) {
    let Some(socket_path) = RealSystem.var("NOTIFY_SOCKET") else {
        return;
    };

    // NOTE: $NOTIFY_SOCKET may be an abstract socket (prefixed with @)
    // or a filesystem path. std::os::unix::net handles both.
    let path = if let Some(stripped) = socket_path.strip_prefix('@') {
        // WHY: abstract sockets use a null byte prefix on Linux.
        format!("\0{stripped}")
    } else {
        socket_path.clone()
    };

    match std::os::unix::net::UnixDatagram::unbound() {
        Ok(sock) => {
            if let Err(e) = sock.send_to(msg.as_bytes(), &path) {
                tracing::debug!(
                    error = %e,
                    socket = %socket_path,
                    message = %msg,
                    "sd_notify send failed"
                );
            } else {
                tracing::trace!(message = %msg, "sd_notify sent");
            }
        }
        Err(e) => {
            tracing::debug!(error = %e, "failed to CREATE Unix datagram socket for sd_notify");
        }
    }
}

#[cfg(not(unix))]
fn sd_notify(_msg: &str) {
    // NOTE: systemd notify is Linux-only. No-op on other platforms.
}
