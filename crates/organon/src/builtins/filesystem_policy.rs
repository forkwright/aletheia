//! Shared filesystem mutation policy for built-in workspace tools.

use std::path::{Component, Path, PathBuf};

const RESERVED_WORKSPACE_PATHS: &[&str] = &[
    "IDENTITY.md",
    "SOUL.md",
    "GOALS.md",
    "TOOLS.md",
    "MEMORY.md",
    "standards",
];

const KEY_EXTENSIONS: &[&str] = &["key", "pem", "p12", "pfx"];

const SSH_KEY_PREFIXES: &[&str] = &["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa", "id_xmss"]; // pii-allow: SSH filename constants guarding access, not key material.

const PROVIDER_CONFIG_DIRS: &[&str] = &[".claude", ".codex"];

/// Classify a path protected by the built-in filesystem mutation policy.
///
/// The returned string is a stable policy class, not a path. Tool-facing errors
/// can include it without exposing an operator's private directory layout.
pub(crate) fn protected_path_class(path: &Path, workspace: &Path) -> Option<&'static str> {
    let relative = relative_to_workspace(path, workspace);
    let rel_str = path_to_slash(&relative);
    let rel_lower = rel_str.to_ascii_lowercase();
    let components = string_components(&relative);
    let lower_components = components
        .iter()
        .map(|component| component.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if RESERVED_WORKSPACE_PATHS
        .iter()
        .any(|protected| relative_path_matches(&rel_str, protected))
    {
        return Some("reserved workspace file");
    }

    if has_component(&lower_components, ".git") {
        return Some("git metadata");
    }

    if rel_lower == ".claude/settings.json" {
        return Some("provider configuration");
    }

    if lower_components
        .iter()
        .any(|component| PROVIDER_CONFIG_DIRS.contains(&component.as_str()))
    {
        return Some("provider credential store");
    }

    if has_component(&lower_components, "credentials") {
        return Some("credential store");
    }

    if is_instance_config_secret(&lower_components) {
        return Some("instance config secret");
    }

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let filename_lower = filename.to_ascii_lowercase();

    if filename_lower.starts_with(".env") {
        return Some("environment credential file");
    }

    if filename_lower.starts_with(".credentials") || filename_lower.contains(".credentials") {
        return Some("credential file");
    }

    if filename_lower == "known_hosts" {
        return Some("ssh known_hosts");
    }

    if SSH_KEY_PREFIXES
        .iter()
        .any(|prefix| filename.starts_with(prefix))
    {
        return Some("ssh key material");
    }

    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| KEY_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
    {
        return Some("key material");
    }

    None
}

fn relative_to_workspace(path: &Path, workspace: &Path) -> PathBuf {
    let workspace_canonical = workspace.canonicalize();
    let workspace_ref = workspace_canonical.as_deref().unwrap_or(workspace);
    path.strip_prefix(workspace_ref)
        .or_else(|_| path.strip_prefix(workspace))
        .unwrap_or(path)
        .to_path_buf()
}

fn path_to_slash(path: &Path) -> String {
    string_components(path).join("/")
}

fn string_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn relative_path_matches(relative: &str, protected: &str) -> bool {
    relative == protected
        || relative
            .strip_prefix(protected)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn has_component(components: &[String], needle: &str) -> bool {
    components.iter().any(|component| component == needle)
}

fn is_instance_config_secret(components: &[String]) -> bool {
    matches!(
        components,
        [config, file] if config == "config"
            && (file == "env"
                || file.starts_with("env.")
                || (file.starts_with("aletheia") && has_toml_extension(file)))
    )
}

fn has_toml_extension(file: &str) -> bool {
    Path::new(file)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
}
