#![expect(clippy::expect_used, reason = "test assertions")]

//! Unit tests for Claude Code credential path discovery in `credential/claude_code_path.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use koina::system::Environment;

use super::{ClaudeCodeCredentialSource, ConfigDirProvider, claude_code_credential_path_with_env};

#[derive(Default)]
struct TestEnv {
    vars: HashMap<String, String>,
    config_dir: Option<PathBuf>,
}

impl TestEnv {
    fn new() -> Self {
        Self::default()
    }

    fn with_env(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_owned(), value.to_owned());
        self
    }

    fn with_config_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(path.into());
        self
    }
}

impl Environment for TestEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn var_os(&self, name: &str) -> Option<std::ffi::OsString> {
        self.vars.get(name).map(Into::into)
    }

    fn vars(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    fn current_dir(&self) -> std::io::Result<PathBuf> {
        Ok(PathBuf::from("/test"))
    }

    fn temp_dir(&self) -> PathBuf {
        PathBuf::from("/tmp")
    }

    fn current_exe(&self) -> std::io::Result<PathBuf> {
        Ok(PathBuf::from("/test/bin/aletheia"))
    }

    fn args(&self) -> Vec<String> {
        vec!["aletheia".to_owned()]
    }
}

impl ConfigDirProvider for TestEnv {
    fn config_dir(&self) -> Option<PathBuf> {
        self.config_dir.clone()
    }
}

#[test]
fn claude_code_path_uses_platform_config_dir_default_when_home_missing() {
    // WHY: services and sanitized environments may not set HOME; discovery must
    // still work via the platform config directory.
    let env = TestEnv::new().with_config_dir("/test/config");

    let resolved = claude_code_credential_path_with_env(None, &env, &env)
        .expect("platform config dir default should resolve when HOME is missing");

    assert_eq!(
        resolved.path,
        Path::new("/test/config/.claude/.credentials.json")
    );
    assert_eq!(
        resolved.source,
        ClaudeCodeCredentialSource::PlatformConfigDir
    );
}

#[test]
fn claude_code_path_prefers_env_override_over_platform_default() {
    let env = TestEnv::new()
        .with_config_dir("/test/config")
        .with_env("CLAUDE_CODE_CREDS", "~/cc/env.json")
        .with_env("HOME", "/home/alice");

    let resolved = claude_code_credential_path_with_env(None, &env, &env)
        .expect("env override should resolve");

    assert_eq!(resolved.path, Path::new("/home/alice/cc/env.json"));
    assert_eq!(resolved.source, ClaudeCodeCredentialSource::Env);
}

#[test]
fn claude_code_path_uses_configured_path_when_env_absent() {
    let env = TestEnv::new().with_env("HOME", "/home/alice");

    let resolved =
        claude_code_credential_path_with_env(Some("/srv/cc/credentials.json"), &env, &env)
            .expect("configured path should resolve");

    assert_eq!(resolved.path, Path::new("/srv/cc/credentials.json"));
    assert_eq!(resolved.source, ClaudeCodeCredentialSource::Config);
}

#[test]
fn claude_code_path_expands_configured_tilde_path() {
    let env = TestEnv::new().with_env("HOME", "/home/alice");

    let resolved =
        claude_code_credential_path_with_env(Some("~/.config/claude/creds.json"), &env, &env)
            .expect("configured path should resolve");

    assert_eq!(
        resolved.path,
        Path::new("/home/alice/.config/claude/creds.json")
    );
    assert_eq!(resolved.source, ClaudeCodeCredentialSource::Config);
}

#[test]
fn claude_code_path_env_override_wins_over_configured_path() {
    let env = TestEnv::new()
        .with_env("HOME", "/home/alice")
        .with_env("CLAUDE_CODE_CREDS", "/cc/from-env.json");

    let resolved =
        claude_code_credential_path_with_env(Some("/srv/cc/credentials.json"), &env, &env)
            .expect("env override should take precedence");

    assert_eq!(resolved.path, Path::new("/cc/from-env.json"));
    assert_eq!(resolved.source, ClaudeCodeCredentialSource::Env);
}
