//! Platform integration: window state and notifications.
//!
//! Each submodule provides framework-agnostic logic that the Dioxus integration
//! layer in `app.rs` wires into the reactive component tree.
//! [`native_notify::send_native`] dispatches freedesktop.org D-Bus
//! notifications on Linux, falling back gracefully if the daemon is absent.

/// D-Bus notification dispatch with graceful fallback.
pub(crate) mod native_notify;
/// Notification payload and urgency types.
pub(crate) mod notifications;
/// Window geometry and UI state persistence with debounced writes.
pub(crate) mod window_state;

#[cfg(test)]
mod tests {
    use std::path::Path;

    const UNSUPPORTED_NATIVE_SHELL_MODULES: &[&str] = &["hotkeys.rs", "menus.rs", "tray.rs"];

    #[test]
    fn unsupported_native_shell_modules_are_not_stubbed() {
        let platform_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/platform");

        for module in UNSUPPORTED_NATIVE_SHELL_MODULES {
            let path = platform_dir.join(module);
            assert!(
                !path.exists(),
                "{module} must not be reintroduced as a placeholder; implement the native runtime wiring before adding it back"
            );
        }
    }
}
