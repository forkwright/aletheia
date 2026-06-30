#![expect(clippy::expect_used, reason = "test assertions")]

//! Unit tests for Claude Code credential path resolution in `credential/refresh.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use koina::system::Environment;

use super::claude_code_credential_path_with_env;

#[derive(Default)]
struct TestEnv {
    vars: HashMap<String, String>,
}

impl TestEnv {
    fn new() -> Self {
        Self::default()
    }

    fn with_env(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_owned(), value.to_owned());
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

#[test]
fn claude_code_path_has_no_implicit_home_default() {
    // WHY: clean Aletheia installs must not probe another agent's private
    // credential store unless the operator opts in.
    let env = TestEnv::new().with_env("HOME", "/home/alice");

    assert_eq!(claude_code_credential_path_with_env(None, &env), None);
}

#[test]
fn claude_code_path_uses_env_override_before_config() {
    // WHY: non-default Claude Code credential locations must not be silently
    // skipped when the operator supplied an explicit override.
    let env = TestEnv::new()
        .with_env("HOME", "/home/alice")
        .with_env("CLAUDE_CODE_CREDS", "~/cc/env.json");

    let resolved =
        claude_code_credential_path_with_env(Some("/srv/aletheia/configured.json"), &env)
            .expect("env override should resolve");

    assert_eq!(resolved, Path::new("/home/alice/cc/env.json"));
}

#[test]
fn claude_code_path_uses_configured_path_when_env_absent() {
    let env = TestEnv::new().with_env("HOME", "/home/alice");

    let resolved = claude_code_credential_path_with_env(Some("/srv/cc/credentials.json"), &env)
        .expect("configured path should resolve");

    assert_eq!(resolved, Path::new("/srv/cc/credentials.json"));
}

#[test]
fn claude_code_path_expands_configured_tilde_path() {
    let env = TestEnv::new().with_env("HOME", "/home/alice");

    let resolved = claude_code_credential_path_with_env(Some("~/.config/claude/creds.json"), &env)
        .expect("configured path should resolve");

    assert_eq!(resolved, Path::new("/home/alice/.config/claude/creds.json"));
}
