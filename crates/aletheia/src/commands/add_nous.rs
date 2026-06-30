//! `aletheia add-nous`: scaffold a new nous agent directory.

use std::path::{Path, PathBuf};

use clap::Args;
use snafu::prelude::*;

use koina::system::{Environment, RealSystem};
use taxis::oikos::Oikos;

use crate::error::Result;
use crate::provider_config::{ensure_provider_model, parse_cli_provider, validate_cli_provider};

#[derive(Debug, Clone, Args)]
pub(crate) struct AddNousArgs {
    /// Agent identifier (alphanumeric and hyphens only).
    pub name: String,

    /// LLM provider.
    #[arg(long, default_value = "anthropic")]
    pub provider: String,

    /// Model identifier.
    #[arg(long, default_value = koina::defaults::DEFAULT_MODEL)]
    pub model: String,
}

pub(crate) async fn run(instance_root: Option<&PathBuf>, args: &AddNousArgs) -> Result<()> {
    validate_name(&args.name)?;
    validate_provider(&args.provider)?;
    validate_model(&args.model)?;

    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    // WHY: refuse to create partial structure in an uninitialized directory.
    ensure_initialized(&oikos)?;

    check_credential(&args.provider);

    // WHY: parse, validate, and prepare the config update before touching the
    // filesystem so bad TOML, duplicate IDs, or unwritable config fail fast.
    let prepared = prepare_config_update(&oikos, args)?;

    let nous_dir = oikos.nous_dir(&args.name);
    if nous_dir.exists() {
        whatever!(
            "nous directory already exists: {}\nRemove it first if you want to recreate this agent.",
            nous_dir.display()
        );
    }

    scaffold_directory(&oikos, args)?;

    // WHY: apply the validated config update; rollback the scaffold if it fails.
    if let Err(e) = apply_config_update(&prepared) {
        // kanon:ignore RUST/no-silent-result-swallow — best-effort rollback of partial scaffold
        let _ = std::fs::remove_dir_all(&nous_dir);
        return Err(e);
    }

    try_register(&oikos, &args.name).await;
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
    validate_cli_provider(provider)
}

fn validate_model(model: &str) -> Result<()> {
    if model.trim().is_empty() {
        whatever!("--model must not be empty");
    }
    Ok(())
}

fn check_credential(provider: &str) {
    let env_var = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => return,
    };

    let env = RealSystem;
    let has_env = env.var(env_var).as_ref().is_some_and(|v| !v.is_empty());

    let has_auth_token = if provider == "anthropic" {
        env.var("ANTHROPIC_AUTH_TOKEN")
            .as_ref()
            .is_some_and(|v| !v.is_empty())
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

/// Require an initialized Aletheia instance before mutating state.
///
/// WHY: `add-nous` is not `init`; running it in an empty directory should not
/// create a partial instance layout. An initialized root has the config and
/// data directories and the main TOML config file produced by `aletheia init`.
fn ensure_initialized(oikos: &Oikos) -> Result<()> {
    let root = oikos.root();
    if !root.exists() {
        whatever!(
            "instance not found at {}\n  \
             Use -r /path/to/instance or set ALETHEIA_ROOT.\n  \
             To create a new instance: aletheia init",
            root.display()
        );
    }

    for dir in &[oikos.config(), oikos.data()] {
        if !dir.exists() {
            whatever!(
                "instance at {} is not initialized (missing {} directory)\n  \
                 Run `aletheia init` first.",
                root.display(),
                dir.file_name().map_or_else(
                    || dir.as_os_str().to_string_lossy(),
                    |n| n.to_string_lossy()
                )
            );
        }
    }

    let config_file = oikos.config().join("aletheia.toml");
    if !config_file.exists() {
        whatever!(
            "instance at {} is not initialized (missing {})\n  \
             Run `aletheia init` first.",
            root.display(),
            config_file.display()
        );
    }

    Ok(())
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

/// A config update that has been validated but not yet written.
struct PreparedConfig {
    config_path: PathBuf,
    content: String,
}

/// Validate the target config and build the updated TOML without writing it.
///
/// WHY: This lets `run` detect bad TOML, duplicate IDs, and unwritable config
/// before scaffolding the agent directory, keeping failures transactional.
fn prepare_config_update(oikos: &Oikos, args: &AddNousArgs) -> Result<PreparedConfig> {
    let config_path = oikos.config().join("aletheia.toml");

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

    let provider = parse_cli_provider(&args.provider)?;

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

    ensure_provider_model(&mut doc, provider, &args.model)?;

    // WHY: prove the config directory is writable before scaffolding.
    ensure_writable(&config_path)?;

    Ok(PreparedConfig {
        config_path,
        content: doc.to_string(),
    })
}

/// Write a prepared config update to disk.
fn apply_config_update(prepared: &PreparedConfig) -> Result<()> {
    koina::fs::write_restricted(&prepared.config_path, prepared.content.as_bytes())
        .with_whatever_context(|_| format!("failed to write {}", prepared.config_path.display()))
}

/// Modify the TOML config in-place using `toml_edit` to preserve comments and structure.
///
/// WHY: Convenience wrapper for direct callers/tests; `run` uses the split
/// prepare/apply path so it can validate before scaffolding.
#[cfg(test)]
fn update_config(oikos: &Oikos, args: &AddNousArgs) -> Result<()> {
    let prepared = prepare_config_update(oikos, args)?;
    apply_config_update(&prepared)
}

/// Verify that the parent directory of `path` is writable.
fn ensure_writable(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(crate::error::Error::msg("path has no parent directory"));
    };

    let test_file = parent.join(".aletheia-write-test");
    let result = koina::fs::write_restricted(&test_file, b"ok");
    // kanon:ignore RUST/no-silent-result-swallow — best-effort cleanup of transient test file
    let _ = std::fs::remove_file(&test_file);

    result.with_whatever_context(|_| {
        format!("config directory is not writable: {}", parent.display())
    })
}

/// Check if the server is reachable by hitting its health endpoint.
///
/// Reads `gateway.bind` and `gateway.port` from the instance config to
/// construct the URL dynamically instead of assuming the default address.
async fn try_register(oikos: &Oikos, name: &str) {
    let config = taxis::loader::load_config(oikos).ok(); // WHY: best-effort; fallback to defaults if instance config unavailable
    let (bind, port) = config.as_ref().map_or(("127.0.0.1", 18789), |c| {
        (c.gateway.bind.as_str(), c.gateway.port)
    });
    let host = match bind {
        "lan" | "0.0.0.0" | "localhost" => "127.0.0.1",
        other => other,
    };
    let url = format!("http://{host}:{port}/api/health"); // SAFE: localhost-only, no network traversal
    let server_running = reqwest::get(&url).await.is_ok();

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
    koina::fs::write_restricted(path, content.as_bytes())
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
            validate_name("analyst").is_ok(),
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
    fn validate_model_rejects_empty() {
        let err = validate_model("").unwrap_err();
        assert!(
            err.to_string().contains("--model must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_model_rejects_whitespace_only() {
        let err = validate_model("   ").unwrap_err();
        assert!(
            err.to_string().contains("--model must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_model_accepts_well_formed() {
        assert!(validate_model("claude-sonnet-4-6").is_ok());
        assert!(validate_model("gpt-4o").is_ok());
    }

    #[test]
    fn capitalize_first_letter() {
        assert_eq!(
            capitalize("analyst"),
            "Analyst",
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
    fn update_config_openai_appends_provider_entry() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();

        let args = AddNousArgs {
            name: "alice".to_owned(),
            provider: "openai".to_owned(),
            model: "gpt-5".to_owned(),
        };
        update_config(&oikos, &args).unwrap();

        let result = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        let config: taxis::config::AletheiaConfig = toml::from_str(&result).unwrap();
        assert_eq!(config.providers.len(), 1);
        let provider = config.providers.first().unwrap();
        assert_eq!(provider.kind, taxis::config::ProviderKind::OpenAi);
        assert_eq!(provider.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
        assert_eq!(provider.models, ["gpt-5"]);
    }

    #[test]
    fn update_config_openai_reuses_provider_entry_for_new_model() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();

        update_config(
            &oikos,
            &AddNousArgs {
                name: "alice".to_owned(),
                provider: "openai".to_owned(),
                model: "gpt-5".to_owned(),
            },
        )
        .unwrap();
        update_config(
            &oikos,
            &AddNousArgs {
                name: "bob".to_owned(),
                provider: "openai".to_owned(),
                model: "gpt-4.1".to_owned(),
            },
        )
        .unwrap();

        let result = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        let config: taxis::config::AletheiaConfig = toml::from_str(&result).unwrap();
        assert_eq!(
            config.providers.len(),
            1,
            "OpenAI models should share one provider entry"
        );
        let provider = config.providers.first().unwrap();
        assert_eq!(provider.models, ["gpt-5", "gpt-4.1"]);
    }

    #[test]
    fn run_openai_provider_updates_registry_config() {
        let dir = tempfile::tempdir().unwrap();
        let _oikos = init_instance(&dir);

        let args = AddNousArgs {
            name: "openai-agent".to_owned(),
            provider: "openai".to_owned(),
            model: "gpt-5".to_owned(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(run(Some(&dir.path().to_path_buf()), &args))
            .unwrap();

        let result = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        let config: taxis::config::AletheiaConfig = toml::from_str(&result).unwrap();
        assert!(
            config.providers.iter().any(|provider| provider.kind
                == taxis::config::ProviderKind::OpenAi
                && provider.models == ["gpt-5"]),
            "add-nous --provider openai must create a routable provider entry: {result}"
        );
        assert!(
            dir.path().join("nous/openai-agent/SOUL.md").exists(),
            "run should scaffold the new agent"
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

    /// Set up a minimal initialized instance layout for `run()` tests.
    fn init_instance(dir: &tempfile::TempDir) -> Oikos {
        let root = dir.path();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup writes a minimal initialized config"
        )]
        std::fs::write(
            root.join("config/aletheia.toml"),
            "[gateway]\nport = 18789\n",
        )
        .unwrap();
        Oikos::from_root(root)
    }

    #[test]
    fn scaffold_errors_when_directory_exists() {
        let dir = tempfile::tempdir().unwrap();
        let _oikos = init_instance(&dir);
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

    #[test]
    fn run_rejects_uninitialized_instance() {
        let dir = tempfile::tempdir().unwrap();
        // NOTE: only create nous/; the instance lacks config/ and data/.
        std::fs::create_dir_all(dir.path().join("nous")).unwrap();

        let args = AddNousArgs {
            name: "uninitialized".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run(Some(&dir.path().to_path_buf()), &args));
        assert!(
            result.is_err(),
            "run should fail when the instance is not initialized"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not initialized"),
            "expected 'not initialized' in: {msg}"
        );
        assert!(
            !dir.path().join("nous/uninitialized").exists(),
            "must not create partial scaffold in uninitialized directory"
        );
    }

    #[test]
    fn run_rejects_bad_toml_before_scaffolding() {
        let dir = tempfile::tempdir().unwrap();
        let _oikos = init_instance(&dir);
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup writes intentionally broken TOML"
        )]
        std::fs::write(
            dir.path().join("config/aletheia.toml"),
            "[[agents.list\nid = ",
        )
        .unwrap();

        let args = AddNousArgs {
            name: "bad-toml".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run(Some(&dir.path().to_path_buf()), &args));
        assert!(
            result.is_err(),
            "run should fail when the config file contains invalid TOML"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("parse") || msg.contains("TOML"),
            "expected parse error, got: {msg}"
        );
        assert!(
            !dir.path().join("nous/bad-toml").exists(),
            "must not scaffold when config parsing fails"
        );
    }

    #[test]
    fn run_rejects_duplicate_id_before_scaffolding() {
        let dir = tempfile::tempdir().unwrap();
        let _oikos = init_instance(&dir);

        let args = AddNousArgs {
            name: "duplicate".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(run(Some(&dir.path().to_path_buf()), &args))
            .unwrap();

        let result = rt.block_on(run(Some(&dir.path().to_path_buf()), &args));
        assert!(
            result.is_err(),
            "run should fail when the agent id already exists"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("already exists"), "got: {msg}");
    }

    #[test]
    fn run_rejects_unwritable_config_before_scaffolding() {
        let dir = tempfile::tempdir().unwrap();
        let _oikos = init_instance(&dir);
        let config_dir = dir.path().join("config");

        let mut perms = std::fs::metadata(&config_dir).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&config_dir, perms).unwrap();

        let args = AddNousArgs {
            name: "unwritable".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run(Some(&dir.path().to_path_buf()), &args));

        // WHY: restore writable permissions so tempfile can clean up the directory.
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&config_dir).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&config_dir, perms).unwrap();
        }

        assert!(
            result.is_err(),
            "run should fail when the config directory is not writable"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("writable") || msg.contains("Permission denied"),
            "expected writable/permission error, got: {msg}"
        );
        assert!(
            !dir.path().join("nous/unwritable").exists(),
            "must not scaffold when config directory is not writable"
        );
    }
}
