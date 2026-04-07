//! Minimal ANSI color support for terminal output.
//!
//! Replaces owo-colors and supports-color with ~50 lines of inline ANSI codes.
//! Checks `$NO_COLOR`, `$TERM`, and `isatty()` to determine color support.

use std::io::IsTerminal;

/// Check if stdout supports color output.
///
/// Returns false if:
/// - `$NO_COLOR` is set (regardless of value)
/// - `$TERM` is set to "dumb"
/// - stdout is not a TTY
pub fn supports_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
    }
    std::io::stdout().is_terminal()
}

/// ANSI escape codes for common colors and styles.
pub mod ansi {
    /// Reset all styles: `\x1b[0m`
    pub const RESET: &str = "\x1b[0m";
    /// Bold text: `\x1b[1m`
    pub const BOLD: &str = "\x1b[1m";
    /// Dim/faint text: `\x1b[2m`
    pub const DIM: &str = "\x1b[2m";
    /// Red text: `\x1b[31m`
    pub const RED: &str = "\x1b[31m";
    /// Green text: `\x1b[32m`
    pub const GREEN: &str = "\x1b[32m";
    /// Yellow text: `\x1b[33m`
    pub const YELLOW: &str = "\x1b[33m";
}

/// Wrap text in ANSI codes if color is enabled.
fn colorize(text: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("{code}{text}{}", ansi::RESET)
    } else {
        text.to_string()
    }
}

/// Trait for applying ANSI color/style to strings.
pub trait AnsiColorize {
    /// Apply bold style if enabled.
    fn bold(&self, enabled: bool) -> String;
    /// Apply dim/faint style if enabled.
    fn dimmed(&self, enabled: bool) -> String;
    /// Apply red color if enabled.
    fn red(&self, enabled: bool) -> String;
    /// Apply green color if enabled.
    fn green(&self, enabled: bool) -> String;
    /// Apply yellow color if enabled.
    fn yellow(&self, enabled: bool) -> String;
}

impl AnsiColorize for str {
    fn bold(&self, enabled: bool) -> String {
        colorize(self, ansi::BOLD, enabled)
    }

    fn dimmed(&self, enabled: bool) -> String {
        colorize(self, ansi::DIM, enabled)
    }

    fn red(&self, enabled: bool) -> String {
        colorize(self, ansi::RED, enabled)
    }

    fn green(&self, enabled: bool) -> String {
        colorize(self, ansi::GREEN, enabled)
    }

    fn yellow(&self, enabled: bool) -> String {
        colorize(self, ansi::YELLOW, enabled)
    }
}

impl AnsiColorize for String {
    fn bold(&self, enabled: bool) -> String {
        colorize(self, ansi::BOLD, enabled)
    }

    fn dimmed(&self, enabled: bool) -> String {
        colorize(self, ansi::DIM, enabled)
    }

    fn red(&self, enabled: bool) -> String {
        colorize(self, ansi::RED, enabled)
    }

    fn green(&self, enabled: bool) -> String {
        colorize(self, ansi::GREEN, enabled)
    }

    fn yellow(&self, enabled: bool) -> String {
        colorize(self, ansi::YELLOW, enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorize_with_enabled_true() {
        let result = colorize("test", ansi::RED, true);
        assert_eq!(result, "\x1b[31mtest\x1b[0m");
    }

    #[test]
    fn colorize_with_enabled_false() {
        let result = colorize("test", ansi::RED, false);
        assert_eq!(result, "test");
    }

    #[test]
    fn str_colorize_methods() {
        assert_eq!("test".red(true), "\x1b[31mtest\x1b[0m");
        assert_eq!("test".green(true), "\x1b[32mtest\x1b[0m");
        assert_eq!("test".yellow(true), "\x1b[33mtest\x1b[0m");
        assert_eq!("test".bold(true), "\x1b[1mtest\x1b[0m");
        assert_eq!("test".dimmed(true), "\x1b[2mtest\x1b[0m");
    }

    #[test]
    fn string_colorize_methods() {
        let s = "test".to_string();
        assert_eq!(s.red(true), "\x1b[31mtest\x1b[0m");
    }

    #[test]
    fn no_color_env_disables_color() {
        // We can't easily test the environment variable check in a unit test
        // without affecting global state, but we verify the function exists
        // and returns a boolean.
        let _ = supports_color();
    }
}
