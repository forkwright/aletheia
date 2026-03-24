//! Instance directory scaffolding and config rendering.

use std::path::Path;

use snafu::ResultExt;

use super::helpers::set_permissions;
use super::{Answers, CreateDirSnafu, InitError, SerializeJsonSnafu, WriteFileSnafu};

pub(super) fn scaffold(answers: &Answers) -> Result<(), InitError> {
    let root = &answers.root;

    let dirs = [
        root.join("config/credentials"),
        root.join(format!("nous/{}", answers.agent_id)),
        root.join("data"),
        root.join("logs/traces"),
        root.join("shared/coordination"),
    ];
    for dir in &dirs {
        std::fs::create_dir_all(dir).context(CreateDirSnafu { path: dir.clone() })?;
    }

    let config_toml = render_config(answers);
    let config_path = root.join("config/aletheia.toml");
    #[expect(
        clippy::disallowed_methods,
        reason = "aletheia CLI commands use synchronous filesystem operations for config and certificate generation"
    )]
    std::fs::write(&config_path, config_toml).context(WriteFileSnafu {
        path: config_path.clone(),
    })?;
    set_permissions(&config_path, 0o600)?;

    if let Some(ref key) = answers.api_key {
        let cred_path = root.join(format!("config/credentials/{}.json", answers.api_provider));
        let cred_json = serde_json::json!({ "token": key.expose_secret() });
        let json_str = serde_json::to_string_pretty(&cred_json).context(SerializeJsonSnafu)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "aletheia CLI commands use synchronous filesystem operations for config and certificate generation"
        )]
        std::fs::write(&cred_path, json_str).context(WriteFileSnafu {
            path: cred_path.clone(),
        })?;
        set_permissions(&cred_path, 0o600)?;
    }

    scaffold_agent(root, &answers.agent_id, &answers.agent_name)?;

    Ok(())
}

/// Populate the nous agent directory with template files.
///
/// Tries `_default/` on disk first (Pronoea/Noe defaults), falls back to
/// `_template/` on disk, then to compiled-in `_default/` content.
pub(super) fn scaffold_agent(
    root: &Path,
    agent_id: &str,
    agent_name: &str,
) -> Result<(), InitError> {
    let nous_dir = root.join(format!("nous/{agent_id}"));

    // Try on-disk _default/ then _template/ (for development / custom deployments)
    let default_dir = root.join("nous/_default");
    let template_dir = root.join("nous/_template");

    if default_dir.is_dir() {
        copy_dir_recursive(&default_dir, &nous_dir)?;
    } else if template_dir.is_dir() {
        copy_dir_recursive(&template_dir, &nous_dir)?;
    } else {
        write_embedded_default(&nous_dir, agent_name)?;
    }

    Ok(())
}

/// Copy a template directory tree into the agent directory.
pub(super) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), InitError> {
    std::fs::create_dir_all(dst).context(CreateDirSnafu {
        path: dst.to_path_buf(),
    })?;

    let entries = std::fs::read_dir(src).context(CreateDirSnafu {
        path: src.to_path_buf(),
    })?;

    for entry in entries {
        let entry = entry.context(CreateDirSnafu {
            path: src.to_path_buf(),
        })?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).context(WriteFileSnafu {
                path: dst_path.clone(),
            })?;
        }
    }

    Ok(())
}

/// Write the compiled-in `_default/` (Pronoea) template files.
pub(super) fn write_embedded_default(nous_dir: &Path, agent_name: &str) -> Result<(), InitError> {
    // Pronoea-specific files are written as-is; they reference "Pronoea/Noe" by name.
    // If the user chose a different agent name, SOUL.md gets a generic fallback instead.
    let soul = if agent_name == "Pronoea" {
        pronoea_template::SOUL.to_owned()
    } else {
        format!(
            "# {agent_name}\n\n\
             You are {agent_name}, an Aletheia cognitive agent.\n\n\
             You are helpful, thoughtful, and direct. Use the tools available to you\n\
             to assist with tasks. Report what you observe about your environment\n\
             when asked.\n"
        )
    };

    let identity = if agent_name == "Pronoea" {
        pronoea_template::IDENTITY.to_owned()
    } else {
        format!(
            "# Identity\n\n\
             - **Name:** {agent_name}\n\
             - **Creature:** \n\
             - **Vibe:** \n\
             - **Emoji:** \n"
        )
    };

    let files: &[(&str, &str)] = &[
        ("SOUL.md", &soul),
        ("IDENTITY.md", &identity),
        ("AGENTS.md", pronoea_template::AGENTS),
        ("CONTEXT.md", pronoea_template::CONTEXT),
        ("GOALS.md", pronoea_template::GOALS),
        ("MEMORY.md", pronoea_template::MEMORY),
        ("PROSOCHE.md", pronoea_template::PROSOCHE),
        ("README.md", pronoea_template::README),
        ("TOOLS.md", pronoea_template::TOOLS),
        ("USER.md", pronoea_template::USER),
        ("VOICE.md", pronoea_template::VOICE),
        ("WORKFLOWS.md", pronoea_template::WORKFLOWS),
    ];

    for (filename, content) in files {
        let path = nous_dir.join(filename);
        #[expect(
            clippy::disallowed_methods,
            reason = "aletheia CLI commands use synchronous filesystem operations for config and certificate generation"
        )]
        std::fs::write(&path, content).context(WriteFileSnafu { path: path.clone() })?;
    }

    Ok(())
}

/// Compiled-in Pronoea (Noe) template files from `instance.example/nous/_default/`.
mod pronoea_template {
    pub(super) const SOUL: &str =
        include_str!("../../../../instance.example/nous/_default/SOUL.md");
    pub(super) const IDENTITY: &str =
        include_str!("../../../../instance.example/nous/_default/IDENTITY.md");
    pub(super) const AGENTS: &str =
        include_str!("../../../../instance.example/nous/_default/AGENTS.md");
    pub(super) const CONTEXT: &str =
        include_str!("../../../../instance.example/nous/_default/CONTEXT.md");
    pub(super) const GOALS: &str =
        include_str!("../../../../instance.example/nous/_default/GOALS.md");
    pub(super) const MEMORY: &str =
        include_str!("../../../../instance.example/nous/_default/MEMORY.md");
    pub(super) const PROSOCHE: &str =
        include_str!("../../../../instance.example/nous/_default/PROSOCHE.md");
    pub(super) const README: &str =
        include_str!("../../../../instance.example/nous/_default/README.md");
    pub(super) const TOOLS: &str =
        include_str!("../../../../instance.example/nous/_default/TOOLS.md");
    pub(super) const USER: &str =
        include_str!("../../../../instance.example/nous/_default/USER.md");
    pub(super) const VOICE: &str =
        include_str!("../../../../instance.example/nous/_default/VOICE.md");
    pub(super) const WORKFLOWS: &str =
        include_str!("../../../../instance.example/nous/_default/WORKFLOWS.md");
}

pub(super) fn render_config(a: &Answers) -> String {
    // WHY: workspace is stored relative to the instance root so the config
    // works regardless of where the instance directory is placed on disk.
    // Oikos::validate_workspace_path resolves relative paths against the root.
    let workspace = format!("nous/{}", a.agent_id);
    let mut config = format!(
        r#"# Aletheia Instance Configuration
# Config cascade: compiled defaults -> this file -> ALETHEIA_* env vars
# Full reference: docs/CONFIGURATION.md

# --- Gateway ---
[gateway]
port = 18789
bind = "{bind}"

[gateway.auth]
mode = "{auth_mode}"

# tls:
# [gateway.tls]
# enabled = true
# cert_path = "config/tls/cert.pem"
# key_path = "config/tls/key.pem"

# cors:
# [gateway.cors]
# allowed_origins = ["https://my-dashboard.local"]

# csrf:
# [gateway.csrf]
# enabled = true

# --- Agents ---
[agents.defaults]
context_tokens = 200000
max_output_tokens = 16384
user_timezone = "{timezone}"
timeout_seconds = 300
# thinking_enabled = false
# thinking_budget = 10000
# max_tool_iterations = 50

[agents.defaults.model]
primary = "{model}"

[[agents.list]]
id = "{agent_id}"
name = "{agent_name}"
default = true
workspace = "{workspace}"

# --- Channels ---
# [[channels.signal.accounts]]
# account = "+1XXXXXXXXXX"
# http_host = "localhost"
# http_port = 8080

# --- Bindings (route messages to agents) ---
# [[bindings]]
# channel = "signal"
# source = "*"
# nous_id = "{agent_id}"

# --- Embedding (for recall/knowledge search) ---
# [embedding]
# provider = "candle"       # mock | candle
# dimension = 384

# --- Data retention ---
# [data.retention]
# session_max_age_days = 90
# archive_before_delete = true

# --- Maintenance ---
# [maintenance.trace_rotation]
# max_age_days = 14
# [maintenance.db_monitoring]
# warn_threshold_mb = 100

# --- Cost tracking ---
[pricing.{model}]
inputCostPerMtok = 3.0
outputCostPerMtok = 15.0

# --- Credentials ---
[credential]
source = "{credential_source}"
"#,
        bind = a.bind,
        auth_mode = a.auth_mode,
        model = a.model,
        timezone = a.timezone,
        agent_id = a.agent_id,
        agent_name = a.agent_name,
        workspace = workspace,
        credential_source = a.credential_source,
    );
    // WHY: single-agent init always produces a single-agent config.
    // Append permissive sandbox defaults so the agent is functional on kernels
    // without Landlock and can execute scripts from HOME: closes #1247.
    config.push_str(SINGLE_AGENT_SANDBOX_TOML);
    config
}

/// Sandbox section appended to single-agent init configs.
///
/// WHY: Single-agent local deployments need to run scripts from the operator's
/// home directory and should not be blocked by strict Landlock enforcement
/// when kernels do not support it. `enforcement=permissive` keeps the agent
/// functional on older kernels; `extraExecPaths = ["~"]` grants exec access
/// to HOME so scripts installed there are reachable: closes #1247.
const SINGLE_AGENT_SANDBOX_TOML: &str = r#"
# --- Sandbox ---
# Single-agent permissive defaults: enforcement falls back gracefully on
# kernels without Landlock and HOME is added to exec paths for local scripts.
[sandbox]
enforcement = "permissive"
extraExecPaths = ["~"]
"#;
