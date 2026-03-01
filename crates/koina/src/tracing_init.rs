//! Tracing initialization for Aletheia.
//!
//! Sets up structured logging via `tracing-subscriber`. Supports JSON output
//! for production and human-readable output for development.

use tracing_subscriber::{fmt, EnvFilter};

/// Initialize tracing with sensible defaults.
///
/// Reads `RUST_LOG` env var for filter directives. Defaults to `info` level
/// for Aletheia crates, `warn` for dependencies.
///
/// # Panics
/// Panics if the subscriber cannot be set (should only happen if called twice).
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("aletheia=info,warn")
    });

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}

/// Initialize tracing with JSON output for structured log collection.
///
/// # Panics
/// Panics if the subscriber cannot be set.
pub fn init_json() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("aletheia=info,warn")
    });

    fmt()
        .with_env_filter(filter)
        .json()
        .with_target(true)
        .with_current_span(true)
        .init();
}

#[cfg(test)]
mod tests {
    // Tracing init is global state — can only test it doesn't panic.
    // Integration tests exercise actual output.

    #[test]
    fn env_filter_parses_default() {
        use tracing_subscriber::EnvFilter;
        let filter = EnvFilter::new("aletheia=info,warn");
        // Just verifying it doesn't panic
        drop(filter);
    }
}
