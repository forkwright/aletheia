//! Validation helpers for `RuntimeBuilder::validate`.

use std::path::{Path, PathBuf};

use koina::system::{Environment, RealSystem};
use taxis::config::{AletheiaConfig, LlmProviderConfig, ProviderKind};
use taxis::oikos::Oikos;

/// Validate JWT key configuration. Returns `true` if valid.
pub(super) fn validate_jwt(config: &AletheiaConfig) -> bool {
    let jwt_key = config
        .gateway
        .auth
        .signing_key
        .as_ref()
        .map(|s| s.expose_secret().to_owned())
        .or_else(|| RealSystem.var("ALETHEIA_JWT_SECRET"));
    let auth_mode = config.gateway.auth.mode.as_str();
    let jwt_check_label = "gateway.auth JWT key";
    if matches!(auth_mode, "token" | "jwt") {
        match jwt_key.as_deref() {
            Some("CHANGE-ME-IN-PRODUCTION") | None => {
                // kanon:ignore RUST/println-in-lib — CLI user-facing output, not log
                println!(
                    "  [FAIL] {jwt_check_label}: key is still the default placeholder\n         \
                     Set gateway.auth.signingKey in aletheia.toml or ALETHEIA_JWT_SECRET env var.\n         \
                     Generate one with: openssl rand -hex 32"
                );
                return false;
            }
            Some(_) => {
                // kanon:ignore RUST/println-in-lib — CLI user-facing output, not log
                println!("  [pass] {jwt_check_label}");
            }
        }
    } else {
        // kanon:ignore RUST/println-in-lib — CLI user-facing output, not log
        println!("  [pass] {jwt_check_label} (auth mode '{auth_mode}' -- JWT not required)");
    }
    true
}

pub(super) fn provider_runtime_errors(config: &AletheiaConfig, oikos: &Oikos) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, entry) in config.providers.iter().enumerate() {
        match entry.kind {
            ProviderKind::ClaudeCode => validate_claude_code_provider(i, entry, oikos, &mut errors),
            ProviderKind::CodexOauth => validate_codex_provider(i, entry, oikos, &mut errors),
            _ => {}
        }
    }
    errors
}

fn validate_claude_code_provider(
    i: usize,
    entry: &LlmProviderConfig,
    oikos: &Oikos,
    errors: &mut Vec<String>,
) {
    #[cfg(feature = "cc-provider")]
    {
        validate_enabled_subprocess_provider(
            i,
            entry,
            oikos,
            "claude-code",
            "claude",
            &[".local/bin/claude", ".claude/bin/claude"],
            errors,
        );
    }

    #[cfg(not(feature = "cc-provider"))]
    {
        let _ = oikos;
        errors.push(format!(
            "providers[{i}] '{}' uses providerType = claude-code, but this aletheia binary was built without the cc-provider feature; rebuild with --features cc-provider or remove the entry",
            entry.name
        ));
    }
}

fn validate_codex_provider(
    i: usize,
    entry: &LlmProviderConfig,
    oikos: &Oikos,
    errors: &mut Vec<String>,
) {
    #[cfg(feature = "codex-provider")]
    {
        validate_enabled_subprocess_provider(
            i,
            entry,
            oikos,
            "codex-oauth",
            "codex",
            &[".local/bin/codex", ".codex/bin/codex"],
            errors,
        );
    }

    #[cfg(not(feature = "codex-provider"))]
    {
        let _ = oikos;
        errors.push(format!(
            "providers[{i}] '{}' uses providerType = codex-oauth, but this aletheia binary was built without the codex-provider feature; rebuild with --features codex-provider or remove the entry",
            entry.name
        ));
    }
}

#[cfg(any(feature = "cc-provider", feature = "codex-provider"))]
fn validate_enabled_subprocess_provider(
    i: usize,
    entry: &LlmProviderConfig,
    oikos: &Oikos,
    provider_type: &str,
    binary_name: &str,
    home_candidates: &[&str],
    errors: &mut Vec<String>,
) {
    if let Some(binary) = entry.binary.as_deref() {
        let binary = resolve_config_path(oikos, binary);
        if !binary.is_file() {
            errors.push(format!(
                "providers[{i}] '{}' providerType = {provider_type} configured binary '{}' does not exist or is not a file",
                entry.name,
                binary.display()
            ));
        }
    } else if find_subprocess_binary(binary_name, home_candidates).is_none() {
        errors.push(format!(
            "providers[{i}] '{}' providerType = {provider_type} requires '{binary_name}' on PATH or a configured binary",
            entry.name
        ));
    }

    if let Some(workdir) = entry.workdir.as_deref() {
        let workdir = resolve_config_path(oikos, workdir);
        if !workdir.is_dir() {
            errors.push(format!(
                "providers[{i}] '{}' providerType = {provider_type} configured workdir '{}' does not exist or is not a directory",
                entry.name,
                workdir.display()
            ));
        }
    }
}

#[cfg(any(feature = "cc-provider", feature = "codex-provider"))]
fn resolve_config_path(oikos: &Oikos, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        oikos.root().join(path)
    }
}

#[cfg(any(feature = "cc-provider", feature = "codex-provider"))]
fn find_subprocess_binary(binary_name: &str, home_candidates: &[&str]) -> Option<PathBuf> {
    let paths = RealSystem.var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let home = RealSystem.var_os("HOME").map(PathBuf::from)?;
    home_candidates
        .iter()
        .map(|subdir| home.join(subdir))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use taxis::config::DeploymentTarget;

    fn subprocess_entry(kind: ProviderKind, name: &str) -> LlmProviderConfig {
        LlmProviderConfig {
            name: name.to_owned(),
            kind,
            base_url: None,
            api_key_env: None,
            api_family: None,
            binary: None,
            workdir: None,
            timeout_secs: None,
            deployment_target: DeploymentTarget::Cloud,
            models: Vec::new(),
        }
    }

    #[cfg(feature = "cc-provider")]
    #[expect(
        clippy::disallowed_methods,
        reason = "WHY(#4889): test fixture writes a fake CLI binary under a temp instance"
    )]
    #[test]
    fn provider_runtime_accepts_declared_claude_code_with_configured_binary_and_workdir() {
        let root = tempfile::tempdir().expect("create temp instance");
        let binary = root.path().join("bin/claude");
        let workdir = root.path().join("work");
        std::fs::create_dir_all(binary.parent().expect("binary has parent"))
            .expect("create bin dir");
        std::fs::create_dir_all(&workdir).expect("create workdir");
        std::fs::write(&binary, "#!/bin/sh\n").expect("write fake claude binary");

        let mut config = AletheiaConfig::default();
        let mut entry = subprocess_entry(ProviderKind::ClaudeCode, "cc-seat");
        entry.binary = Some(PathBuf::from("bin/claude"));
        entry.workdir = Some(PathBuf::from("work"));
        config.providers.push(entry);

        let oikos = Oikos::from_root(root.path());
        assert_eq!(
            provider_runtime_errors(&config, &oikos),
            Vec::<String>::new()
        );
    }

    #[cfg(feature = "cc-provider")]
    #[test]
    fn provider_runtime_rejects_declared_claude_code_missing_configured_binary() {
        let root = tempfile::tempdir().expect("create temp instance");
        let mut config = AletheiaConfig::default();
        let mut entry = subprocess_entry(ProviderKind::ClaudeCode, "cc-seat");
        entry.binary = Some(PathBuf::from("bin/missing-claude"));
        config.providers.push(entry);

        let oikos = Oikos::from_root(root.path());
        let errors = provider_runtime_errors(&config, &oikos);

        assert!(
            errors
                .iter()
                .any(|error| error.contains("configured binary")),
            "missing binary should be a provider runtime error: {errors:?}"
        );
    }

    #[cfg(not(feature = "codex-provider"))]
    #[test]
    fn provider_runtime_rejects_codex_when_feature_is_disabled() {
        let root = tempfile::tempdir().expect("create temp instance");
        let mut config = AletheiaConfig::default();
        config
            .providers
            .push(subprocess_entry(ProviderKind::CodexOauth, "codex-seat"));

        let oikos = Oikos::from_root(root.path());
        let errors = provider_runtime_errors(&config, &oikos);

        assert!(
            errors
                .iter()
                .any(|error| error.contains("codex-provider feature")),
            "disabled feature should be a provider runtime error: {errors:?}"
        );
    }
}
