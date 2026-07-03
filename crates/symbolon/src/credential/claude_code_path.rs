//! Claude Code credential path discovery: explicit overrides and portable defaults.
//!
//! WHY: `$HOME` is unreliable in containers, systemd services, and sanitized
//! environments. This module resolves explicit overrides first, then falls back
//! to the platform config directory.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use koina::system::{Environment, RealSystem};

const CLAUDE_CODE_CREDS_ENV: &str = "CLAUDE_CODE_CREDS";

/// Source of the resolved Claude Code credential path.
// kanon:ignore RUST/no-debug-derive-on-public-types — source label is a non-secret policy tag used in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeCodeCredentialSource {
    /// `CLAUDE_CODE_CREDS` environment variable.
    Env,
    /// `credential.claudeCodeCredentials` configuration value.
    Config,
    /// Platform-specific config directory default.
    PlatformConfigDir,
}

impl fmt::Display for ClaudeCodeCredentialSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Env => f.write_str("env"),
            Self::Config => f.write_str("config"),
            Self::PlatformConfigDir => f.write_str("platform-config-dir"),
        }
    }
}

/// A resolved Claude Code credential path and the rule that chose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCodeCredentialResolution {
    /// Resolved credential file path.
    pub path: PathBuf,
    /// Rule that selected the path.
    pub source: ClaudeCodeCredentialSource,
}

/// Provider of the platform config directory so tests can avoid the real filesystem.
pub(super) trait ConfigDirProvider {
    fn config_dir(&self) -> Option<PathBuf>;
}

impl ConfigDirProvider for RealSystem {
    fn config_dir(&self) -> Option<PathBuf> {
        platform_config_dir(self)
    }
}

fn non_empty_var(env: &impl Environment, name: &str) -> Option<OsString> {
    env.var_os(name)
        .filter(|value| !value.as_os_str().is_empty())
}

fn platform_config_dir(env: &impl Environment) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return non_empty_var(env, "APPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                non_empty_var(env, "USERPROFILE")
                    .map(|home| PathBuf::from(home).join("AppData").join("Roaming"))
            });
    }

    if cfg!(target_os = "macos") {
        return non_empty_var(env, "HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
        });
    }

    non_empty_var(env, "XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| non_empty_var(env, "HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn expand_tilde_path(path: &str, env: &impl Environment) -> PathBuf {
    if path == "~" {
        return env
            .var_os("HOME")
            .map_or_else(|| PathBuf::from(path), PathBuf::from);
    }

    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = env.var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }

    PathBuf::from(path)
}

pub(super) fn claude_code_credential_path_with_env(
    configured_path: Option<&str>,
    env: &impl Environment,
    config_dir: &impl ConfigDirProvider,
) -> Option<ClaudeCodeCredentialResolution> {
    if let Some(path) = env.var_os(CLAUDE_CODE_CREDS_ENV).filter(|p| !p.is_empty()) {
        let path = if let Some(path_str) = path.to_str() {
            expand_tilde_path(path_str, env)
        } else {
            PathBuf::from(path)
        };
        return Some(ClaudeCodeCredentialResolution {
            path,
            source: ClaudeCodeCredentialSource::Env,
        });
    }

    if let Some(path) = configured_path.map(str::trim).filter(|p| !p.is_empty()) {
        return Some(ClaudeCodeCredentialResolution {
            path: expand_tilde_path(path, env),
            source: ClaudeCodeCredentialSource::Config,
        });
    }

    config_dir.config_dir().map(|dir| {
        let path = dir.join(".claude").join(".credentials.json");
        ClaudeCodeCredentialResolution {
            path,
            source: ClaudeCodeCredentialSource::PlatformConfigDir,
        }
    })
}

/// Resolve a Claude Code credential path.
///
/// Precedence:
///
/// 1. `CLAUDE_CODE_CREDS`
/// 2. `credential.claudeCodeCredentials`
/// 3. Platform-specific config directory
#[must_use]
// kanon:ignore RUST/pub-visibility -- WHY: aletheia runtime setup consumes this re-export to resolve the configured Claude Code credential path.
pub fn claude_code_credential_path(configured_path: Option<&str>) -> Option<PathBuf> {
    claude_code_credential_path_with_env(configured_path, &RealSystem, &RealSystem).map(|r| r.path)
}

/// Resolve a Claude Code credential path, including the source that selected it.
#[must_use]
// kanon:ignore RUST/pub-visibility -- WHY: aletheia runtime setup consumes this re-export to log the resolved path source without exposing secrets.
pub fn claude_code_credential_path_resolution(
    configured_path: Option<&str>,
) -> Option<ClaudeCodeCredentialResolution> {
    claude_code_credential_path_with_env(configured_path, &RealSystem, &RealSystem)
}

/// Default Claude Code credential path lookup.
#[must_use]
// kanon:ignore RUST/pub-visibility -- WHY: pylon health checks and aletheia credential commands consume this re-export for Claude Code credential detection.
pub fn claude_code_default_path() -> Option<PathBuf> {
    claude_code_credential_path(None)
}

#[cfg(test)]
#[path = "claude_code_path_tests.rs"]
mod claude_code_path_tests;
