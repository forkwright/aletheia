//! `aletheia add-nous` — scaffold a new nous agent directory.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;

use aletheia_taxis::config::{AletheiaConfig, ModelSpec, NousDefinition};
use aletheia_taxis::loader;
use aletheia_taxis::oikos::Oikos;

#[derive(Debug, Clone, Args)]
pub struct AddNousArgs {
    /// Agent identifier (alphanumeric and hyphens only).
    pub name: String,

    /// LLM provider.
    #[arg(long, default_value = "anthropic")]
    pub provider: String,

    /// Model identifier.
    #[arg(long, default_value = "claude-sonnet-4-20250514")]
    pub model: String,
}

pub fn run(instance_root: Option<&PathBuf>, args: &AddNousArgs) -> Result<()> {
    validate_name(&args.name)?;
    validate_provider(&args.provider)?;

    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let nous_dir = oikos.nous_dir(&args.name);
    if nous_dir.exists() {
        bail!(
            "nous directory already exists: {}\nRemove it first if you want to recreate this agent.",
            nous_dir.display()
        );
    }

    check_credential(&args.provider);

    scaffold_directory(&oikos, args)?;
    update_config(&oikos, args)?;
    try_register(&args.name);
    print_summary(&oikos, args);

    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("agent name cannot be empty");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("agent name must contain only alphanumeric characters and hyphens");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("agent name cannot start or end with a hyphen");
    }
    Ok(())
}

fn validate_provider(provider: &str) -> Result<()> {
    match provider {
        "anthropic" | "openai" => Ok(()),
        other => bail!("unsupported provider: {other}\nSupported providers: anthropic, openai"),
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
            .with_context(|| format!("failed to create directory: {}", dir.display()))?;
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

fn update_config(oikos: &Oikos, args: &AddNousArgs) -> Result<()> {
    let config_result = loader::load_config(oikos);
    let mut config: AletheiaConfig = config_result.unwrap_or_default();

    let already_listed = config.agents.list.iter().any(|a| a.id == args.name);
    if already_listed {
        bail!(
            "agent '{}' already exists in the configuration file.\n\
             Remove the existing entry first, or choose a different name.",
            args.name
        );
    }

    let workspace = format!("{}/nous/{}", oikos.root().display(), args.name);

    config.agents.list.push(NousDefinition {
        id: args.name.clone(),
        name: Some(capitalize(&args.name)),
        model: Some(ModelSpec {
            primary: args.model.clone(),
            fallbacks: Vec::new(),
        }),
        workspace,
        thinking_enabled: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
    });

    loader::write_config(oikos, &config)
        .map_err(|e| anyhow::anyhow!("failed to write config: {e}"))?;

    Ok(())
}

fn try_register(name: &str) {
    // WHY: reqwest::blocking panics inside a tokio runtime, so we use a raw
    // TCP connect probe to check if the server is listening.
    use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};

    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 18789);
    let server_running =
        TcpStream::connect_timeout(&addr.into(), std::time::Duration::from_secs(1)).is_ok();

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
    std::fs::write(path, content).with_context(|| format!("failed to write: {}", path.display()))
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
        assert!(validate_name("chiron").is_ok());
        assert!(validate_name("my-agent").is_ok());
        assert!(validate_name("agent42").is_ok());
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn validate_name_rejects_special_chars() {
        assert!(validate_name("my_agent").is_err());
        assert!(validate_name("my agent").is_err());
        assert!(validate_name("my.agent").is_err());
    }

    #[test]
    fn validate_name_rejects_leading_trailing_hyphen() {
        assert!(validate_name("-agent").is_err());
        assert!(validate_name("agent-").is_err());
    }

    #[test]
    fn validate_provider_accepts_known() {
        assert!(validate_provider("anthropic").is_ok());
        assert!(validate_provider("openai").is_ok());
    }

    #[test]
    fn validate_provider_rejects_unknown() {
        assert!(validate_provider("google").is_err());
    }

    #[test]
    fn capitalize_first_letter() {
        assert_eq!(capitalize("chiron"), "Chiron");
        assert_eq!(capitalize("my-agent"), "My-agent");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("A"), "A");
    }

    #[test]
    fn scaffold_creates_expected_structure() {
        let dir = tempfile::tempdir().unwrap();
        let _oikos = Oikos::from_root(dir.path());

        let args = AddNousArgs {
            name: "test-agent".to_owned(),
            provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
        };

        scaffold_directory(&oikos, &args).unwrap();

        let nous_dir = dir.path().join("nous/test-agent");
        assert!(nous_dir.join("SOUL.md").exists());
        assert!(nous_dir.join("IDENTITY.md").exists());
        assert!(nous_dir.join("AGENTS.md").exists());
        assert!(nous_dir.join("CONTEXT.md").exists());
        assert!(nous_dir.join("GOALS.md").exists());
        assert!(nous_dir.join("MEMORY.md").exists());
        assert!(nous_dir.join("PROSOCHE.md").exists());
        assert!(nous_dir.join("TOOLS.md").exists());
        assert!(nous_dir.join("USER.md").exists());
        assert!(nous_dir.join("WORKFLOWS.md").exists());
        assert!(nous_dir.join(".gitignore").exists());
        assert!(nous_dir.join("memory").is_dir());
        assert!(nous_dir.join("workspace/drafts").is_dir());
        assert!(nous_dir.join("workspace/scripts").is_dir());

        let soul = std::fs::read_to_string(nous_dir.join("SOUL.md")).unwrap();
        assert!(soul.contains("Test-agent"));
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

        let result = run(Some(&dir.path().to_path_buf()), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("already exists"),
            "expected 'already exists' in: {msg}"
        );
    }
}
