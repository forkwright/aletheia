//! `aletheia add-nous`: scaffold a new nous agent directory.

use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

use aletheia_taxis::oikos::Oikos;

#[derive(Debug, Clone, Args)]
pub(crate) struct AddNousArgs {
    /// Agent identifier (alphanumeric and hyphens only).
    pub name: String,

    /// LLM provider.
    #[arg(long, default_value = "anthropic")]
    pub provider: String,

    /// Model identifier.
    #[arg(long, default_value = "claude-sonnet-4-20250514")]
    pub model: String,
}

pub(crate) async fn run(instance_root: Option<&PathBuf>, args: &AddNousArgs) -> Result<()> {
    validate_name(&args.name)?;
    validate_provider(&args.provider)?;

    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let nous_dir = oikos.nous_dir(&args.name);
    if nous_dir.exists() {
        whatever!(
            "nous directory already exists: {}\nRemove it first if you want to recreate this agent.",
            nous_dir.display()
        );
    }

    check_credential(&args.provider);

    scaffold_directory(&oikos, args)?;
    update_config(&oikos, args)?;
    try_register(&args.name).await;
    print_summary(&oikos, args);

    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        whatever!("agent name cannot be empty");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        whatever!("agent name must contain only alphanumeric characters and hyphens");
    }
    if name.starts_with('-') || name.ends_with('-') {
        whatever!("agent name cannot start or end with a hyphen");
    }
    Ok(())
}

fn validate_provider(provider: &str) -> Result<()> {
    match provider {
        "anthropic" | "openai" => Ok(()),
        other => whatever!("unsupported provider: {other}\nSupported providers: anthropic, openai"),
    }
}

fn check_credential(provider: &str) {
    let env_var = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => return,
    };

    let has_env = std::env::var(env_var)
        .ok()
        .filter(|v| !v.is_empty())
        .is_some();

    let has_auth_token = if provider == "anthropic" {
        std::env::var("ANTHROPIC_AUTH_TOKEN")
            .ok()
            .filter(|v| !v.is_empty())
            .is_some()
    } else {
        false
    };

    if !has_env && !has_auth_token {
        eprintln!(
            "Warning: no {provider} credential found.\n  \
             Set {env_var} environment variable, or configure credentials in\n  \
             config/credentials/{provider}.json before starting the server."
        );
    }
}

fn scaffold_directory(oikos: &Oikos, args: &AddNousArgs) -> Result<()> {
    let nous_dir = oikos.nous_dir(&args.name);
    let display_name = capitalize(&args.name);

    let subdirs = [
        nous_dir.join("memory"),
        nous_dir.join("workspace/drafts"),
        nous_dir.join("workspace/scripts"),
    ];

    for dir in &subdirs {
        std::fs::create_dir_all(dir)
            .with_whatever_context(|_| format!("failed to create directory: {}", dir.display()))?;
    }

    write_file(
        &nous_dir.join("SOUL.md"),
        &format!(
            "# {display_name}\n\n\
             You are {display_name}, an Aletheia cognitive agent.\n\n\
             You are helpful, thoughtful, and direct. Use the tools available to you\n\
             to assist with tasks.\n"
        ),
    )?;

    write_file(
        &nous_dir.join("IDENTITY.md"),
        &format!(
            "# Identity\n\n\
             - **Name:** {display_name}\n\
             - **Creature:** \n\
             - **Vibe:** \n\
             - **Emoji:** \n"
        ),
    )?;

    for filename in &[
        "AGENTS.md",
        "CONTEXT.md",
        "GOALS.md",
        "MEMORY.md",
        "PROSOCHE.md",
        "TOOLS.md",
        "USER.md",
        "WORKFLOWS.md",
    ] {
        let header = filename.trim_end_matches(".md");
        write_file(&nous_dir.join(filename), &format!("# {header}\n"))?;
    }

    for gitkeep in &[
        "memory/.gitkeep",
        "workspace/drafts/.gitkeep",
        "workspace/scripts/.gitkeep",
    ] {
        write_file(&nous_dir.join(gitkeep), "")?;
    }

    write_file(
        &nous_dir.join(".gitignore"),
        ".aletheia-index/manifest_*.json\n",
    )?;

    Ok(())
}

/// Modify the TOML config in-place using `toml_edit` to preserve comments and structure.
fn update_config(oikos: &Oikos, args: &AddNousArgs) -> Result<()> {
    let config_path = oikos.config().join("aletheia.toml");
    let config_dir = oikos.config();

    let existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .with_whatever_context(|_| format!("failed to read {}", config_path.display()))?
    } else {
        String::new()
    };

    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .with_whatever_context(|_| format!("failed to parse {}", config_path.display()))?;

    let already_listed = doc
        .get("agents")
        .and_then(|a| a.as_table())
        .and_then(|a| a.get("list"))
        .and_then(|l| l.as_array_of_tables())
        .is_some_and(|list| {
            list.iter()
                .any(|t| t.get("id").and_then(|v| v.as_str()) == Some(args.name.as_str()))
        });

    if already_listed {
        whatever!(
            "agent '{}' already exists in the configuration file.\n\
             Remove the existing entry first, or choose a different name.",
            args.name
        );
    }

    // WHY: workspace is relative to ALETHEIA_ROOT so the config stays portable.
    let workspace = format!("nous/{}", args.name);
    let mut entry = toml_edit::Table::new();
    entry.insert("id", toml_edit::value(args.name.clone()));
    entry.insert("name", toml_edit::value(capitalize(&args.name)));
    entry.insert("workspace", toml_edit::value(workspace));
    entry.insert("default", toml_edit::value(false));

    let mut model_table = toml_edit::Table::new();
    model_table.insert("primary", toml_edit::value(args.model.clone()));
    model_table.insert(
        "fallbacks",
        toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new())),
    );
    entry.insert("model", toml_edit::Item::Table(model_table));

    if doc.get("agents").and_then(|i| i.as_table()).is_none() {
        doc.insert("agents", toml_edit::Item::Table(toml_edit::Table::new()));
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "key 'agents' was just inserted if absent, so indexing is valid"
    )]
    let agents = doc["agents"]
        .as_table_mut()
        .ok_or_else(|| crate::error::Error::msg("[agents] in config is not a table"))?;

    if agents
        .get("list")
        .and_then(|i| i.as_array_of_tables())
        .is_none()
    {
        agents.insert(
            "list",
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
        );
    }

    let list = agents["list"].as_array_of_tables_mut().ok_or_else(|| {
        crate::error::Error::msg("agents.list in config is not an array of tables")
    })?;

    list.push(entry);

    // WHY: atomic write: write to .tmp then rename, preserving existing comments in the file
    std::fs::create_dir_all(&config_dir)
        .with_whatever_context(|_| format!("failed to create {}", config_dir.display()))?;
    let tmp = config_dir.join("aletheia.toml.tmp");
    #[expect(
        clippy::disallowed_methods,
        reason = "aletheia CLI commands use synchronous filesystem operations for config and certificate generation"
    )]
    std::fs::write(&tmp, doc.to_string())
        .with_whatever_context(|_| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &config_path)
        .with_whatever_context(|_| format!("failed to rename {}", tmp.display()))?;

    Ok(())
}

/// Check if the server is reachable by hitting its health endpoint.
async fn try_register(name: &str) {
    let url = "http://127.0.0.1:18789/api/health";
    let server_running = reqwest::get(url).await.is_ok();

    if server_running {
        println!(
            "Server is running. Restart the server to load agent '{name}':\n  \
             aletheia"
        );
    } else {
        println!("Server not running. The new agent will be loaded on next server start.");
    }
}

fn print_summary(oikos: &Oikos, args: &AddNousArgs) {
    let nous_dir = oikos.nous_dir(&args.name);
    println!();
    println!("Created nous agent '{}'", args.name);
    println!("  Directory:  {}", nous_dir.display());
    println!("  Provider:   {}", args.provider);
    println!("  Model:      {}", args.model);
    println!();
    println!("Next steps:");
    println!(
        "  1. Edit {}/SOUL.md to define the agent's identity",
        nous_dir.display()
    );
    println!(
        "  2. Edit {}/TOOLS.md to configure available tools",
        nous_dir.display()
    );
    println!("  3. Start the server: aletheia");
}

fn write_file(path: &std::path::Path, content: &str) -> Result<()> {
    #[expect(
        clippy::disallowed_methods,
        reason = "aletheia CLI commands use synchronous filesystem operations for config and certificate generation"
    )]
    std::fs::write(path, content)
        .with_whatever_context(|_| format!("failed to write: {}", path.display()))
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut result: String = c.to_uppercase().collect();
            result.push_str(chars.as_str());
            result
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_alphanumeric() {
        assert!(
            validate_name("chiron").is_ok(),
            "simple lowercase name should be valid"
        );
        assert!(
            validate_name("my-agent").is_ok(),
            "name with hyphen should be valid"
        );
        assert!(
            validate_name("agent42").is_ok(),
            "alphanumeric name should be valid"
        );
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert!(validate_name("").is_err(), "empty name should be rejected");
    }

    #[test]
    fn validate_name_rejects_special_chars() {
        assert!(
            validate_name("my_agent").is_err(),
            "underscore in name should be rejected"
        );
        assert!(
            validate_name("my agent").is_err(),
            "space in name should be rejected"
        );
        assert!(
            validate_name("my.agent").is_err(),
            "dot in name should be rejected"
        );
    }

    #[test]
    fn validate_name_rejects_leading_trailing_hyphen() {
        assert!(
            validate_name("-agent").is_err(),
            "leading hyphen in name should be rejected"
        );
        assert!(
            validate_name("agent-").is_err(),
            "trailing hyphen in name should be rejected"
        );
    }

    #[test]
    fn validate_provider_accepts_known() {
        assert!(
            validate_provider("anthropic").is_ok(),
            "anthropic should be a valid provider"
        );
        assert!(
            validate_provider("openai").is_ok(),
            "openai should be a valid provider"
        );
    }

    #[test]
    fn validate_provider_rejects_unknown() {
        assert!(
            validate_provider("google").is_err(),
            "unknown provider should be rejected"
        );
    }

    #[test]
    fn capitalize_first_letter() {
        assert_eq!(
            capitalize("chiron"),
            "Chiron",
            "first letter should be uppercased"
        );
        assert_eq!(
            capitalize("my-agent"),
            "My-agent",
            "only first letter should be uppercased"
        );
        assert_eq!(capitalize(""), "", "empty string should remain empty");
        assert_eq!(
            capitalize("A"),
            "A",
            "already capitalized single letter should be unchanged"
        );
    }

    #[test]
    fn scaffold_creates_expected_structure() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());

        let args = AddNousArgs {
            name: "test-agent".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };

        scaffold_directory(&oikos, &args).unwrap();

        let nous_dir = dir.path().join("nous/test-agent");
        assert!(
            nous_dir.join("SOUL.md").exists(),
            "SOUL.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("IDENTITY.md").exists(),
            "IDENTITY.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("AGENTS.md").exists(),
            "AGENTS.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("CONTEXT.md").exists(),
            "CONTEXT.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("GOALS.md").exists(),
            "GOALS.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("MEMORY.md").exists(),
            "MEMORY.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("PROSOCHE.md").exists(),
            "PROSOCHE.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("TOOLS.md").exists(),
            "TOOLS.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("USER.md").exists(),
            "USER.md should be created by scaffold"
        );
        assert!(
            nous_dir.join("WORKFLOWS.md").exists(),
            "WORKFLOWS.md should be created by scaffold"
        );
        assert!(
            nous_dir.join(".gitignore").exists(),
            ".gitignore should be created by scaffold"
        );
        assert!(
            nous_dir.join("memory").is_dir(),
            "memory subdirectory should be created by scaffold"
        );
        assert!(
            nous_dir.join("workspace/drafts").is_dir(),
            "workspace/drafts subdirectory should be created by scaffold"
        );
        assert!(
            nous_dir.join("workspace/scripts").is_dir(),
            "workspace/scripts subdirectory should be created by scaffold"
        );

        let soul = std::fs::read_to_string(nous_dir.join("SOUL.md")).unwrap();
        assert!(
            soul.contains("Test-agent"),
            "SOUL.md should contain the capitalized agent name"
        );
    }

    #[test]
    fn update_config_appends_without_destroying_comments() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();

        let original = "# My custom config\n\
            # This comment must survive\n\
            [gateway]\n\
            port = 9999\n\
            \n";
        #[expect(
            clippy::disallowed_methods,
            reason = "aletheia CLI commands use synchronous filesystem operations for config and certificate generation"
        )]
        std::fs::write(dir.path().join("config/aletheia.toml"), original).unwrap();

        let args = AddNousArgs {
            name: "alice".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };
        update_config(&oikos, &args).unwrap();

        let result = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        assert!(
            result.contains("# My custom config"),
            "comment must survive"
        );
        assert!(
            result.contains("# This comment must survive"),
            "comment must survive"
        );
        assert!(
            result.contains("port = 9999"),
            "existing config must survive"
        );
        assert!(result.contains(r#"id = "alice""#), "new agent must appear");
        assert!(
            result.contains(r#"workspace = "nous/alice""#),
            "workspace must be relative"
        );
    }

    #[test]
    fn update_config_workspace_path_is_relative() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();

        let args = AddNousArgs {
            name: "bob".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };
        update_config(&oikos, &args).unwrap();

        let result = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        assert!(
            result.contains(r#"workspace = "nous/bob""#),
            "workspace path must be relative, got:\n{result}"
        );
        assert!(
            !result.contains("/nous/bob"),
            "workspace must not be absolute"
        );
    }

    #[test]
    fn update_config_rejects_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();

        let args = AddNousArgs {
            name: "charlie".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };
        update_config(&oikos, &args).unwrap();
        let result = update_config(&oikos, &args);
        assert!(
            result.is_err(),
            "adding a duplicate agent should return an error"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("already exists"), "got: {msg}");
    }

    #[test]
    fn scaffold_errors_when_directory_exists() {
        let dir = tempfile::tempdir().unwrap();
        let _oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("nous/existing")).unwrap();

        let args = AddNousArgs {
            name: "existing".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };

        // NOTE: use a blocking executor since run() is async
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run(Some(&dir.path().to_path_buf()), &args));
        assert!(
            result.is_err(),
            "run should fail when the nous directory already exists"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("already exists"),
            "expected 'already exists' in: {msg}"
        );
    }
}
