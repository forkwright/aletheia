//! Interactive instance setup wizard.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// User choices collected during the interactive (or non-interactive) flow.
struct Answers {
    root: PathBuf,
    api_key: Option<String>,
    model: String,
    agent_id: String,
    agent_name: String,
    bind: String,
    auth_mode: String,
    timezone: String,
}

impl Default for Answers {
    fn default() -> Self {
        Self {
            root: PathBuf::from("./instance"),
            api_key: None,
            model: "claude-sonnet-4-6".to_owned(),
            agent_id: "main".to_owned(),
            agent_name: "Main".to_owned(),
            bind: "localhost".to_owned(),
            auth_mode: "none".to_owned(),
            timezone: detect_timezone(),
        }
    }
}

pub(crate) fn run(root: PathBuf, non_interactive: bool, api_key: Option<String>) -> Result<()> {
    let mut answers = Answers {
        root,
        api_key,
        ..Answers::default()
    };

    if non_interactive {
        if answers.api_key.is_none() {
            eprintln!("Warning: no API key provided. Set --api-key or ANTHROPIC_API_KEY.");
            eprintln!("         The server will start in degraded mode without credentials.");
        }
    } else {
        answers = collect_interactive(answers)?;
    }

    // Pre-check: existing instance
    let config_path = answers.root.join("config/aletheia.yaml");
    if config_path.exists() && !non_interactive {
        let overwrite: bool = cliclack::confirm("Instance already exists. Overwrite?")
            .initial_value(false)
            .interact()?;
        if !overwrite {
            cliclack::outro_cancel("Aborted.")?;
            return Ok(());
        }
    }

    scaffold(&answers)?;

    if non_interactive {
        println!("Instance created at {}", answers.root.display());
    } else {
        cliclack::outro(format!(
            "Done! Start the server:\n\
             \n\
             \x1b[36m  aletheia -r {}\x1b[0m\n\
             \n\
             Then connect in another terminal:\n\
             \n\
             \x1b[36m  aletheia tui\x1b[0m",
            answers.root.display()
        ))?;
    }

    Ok(())
}

fn collect_interactive(mut answers: Answers) -> Result<Answers> {
    cliclack::intro("Aletheia Instance Setup")?;

    let root: String = cliclack::input("Instance root")
        .default_input(&answers.root.display().to_string())
        .interact()?;
    answers.root = PathBuf::from(root);

    answers.api_key = collect_credential()?;

    let model: &str = cliclack::select("Default model")
        .item("claude-sonnet-4-6", "claude-sonnet-4-6 (recommended)", "")
        .item("claude-opus-4-6", "claude-opus-4-6", "")
        .item("claude-haiku-4-5", "claude-haiku-4-5", "")
        .interact()?;
    model.clone_into(&mut answers.model);

    let agent_id: String = cliclack::input("Agent ID")
        .default_input(&answers.agent_id)
        .validate(|input: &String| {
            if input.is_empty() {
                Err("Agent ID cannot be empty")
            } else if !input.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                Err("Agent ID must be alphanumeric (hyphens and underscores allowed)")
            } else {
                Ok(())
            }
        })
        .interact()?;
    answers.agent_id = agent_id;

    let default_name = capitalize(&answers.agent_id);
    let agent_name: String = cliclack::input("Agent display name")
        .default_input(&default_name)
        .interact()?;
    answers.agent_name = agent_name;

    let bind: &str = cliclack::select("Gateway bind")
        .item("localhost", "localhost (this machine only)", "")
        .item("lan", "lan (network/Tailscale accessible)", "")
        .interact()?;
    bind.clone_into(&mut answers.bind);

    let auth: &str = cliclack::select("Auth mode")
        .item("none", "none (no authentication — single user)", "")
        .item("token", "token (API key required to connect)", "")
        .interact()?;
    auth.clone_into(&mut answers.auth_mode);

    let tz: String = cliclack::input("Timezone")
        .default_input(&answers.timezone)
        .interact()?;
    answers.timezone = tz;

    Ok(answers)
}

fn collect_credential() -> Result<Option<String>> {
    let cred_choice: &str = cliclack::select("Anthropic API credential")
        .item("paste", "Paste API key", "")
        .item("env", "Use ANTHROPIC_API_KEY env var", "")
        .item("skip", "Skip (configure later)", "")
        .interact()?;

    match cred_choice {
        "paste" => {
            let key: String = cliclack::password("API key")
                .mask('*')
                .validate(|input: &String| {
                    if input.is_empty() {
                        Err("Key cannot be empty")
                    } else {
                        Ok(())
                    }
                })
                .interact()?;
            Ok(Some(key))
        }
        "env" => {
            let key = std::env::var("ANTHROPIC_API_KEY")
                .context("ANTHROPIC_API_KEY not set")?;
            Ok(Some(key))
        }
        _ => Ok(None),
    }
}

fn scaffold(answers: &Answers) -> Result<()> {
    let root = &answers.root;

    // Create directories
    let dirs = [
        root.join("config/credentials"),
        root.join(format!("nous/{}", answers.agent_id)),
        root.join("data"),
    ];
    for dir in &dirs {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
    }

    // Write config
    let config_yaml = render_config(answers);
    let config_path = root.join("config/aletheia.yaml");
    std::fs::write(&config_path, config_yaml)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    // Write credential
    if let Some(ref key) = answers.api_key {
        let cred_path = root.join("config/credentials/anthropic.json");
        let cred_json = serde_json::json!({ "token": key });
        std::fs::write(&cred_path, serde_json::to_string_pretty(&cred_json)?)
            .with_context(|| format!("failed to write {}", cred_path.display()))?;
        set_permissions(&cred_path, 0o600)?;
    }

    // Write SOUL.md
    let soul = format!(
        "# {name}\n\n\
         You are {name}, an Aletheia cognitive agent.\n\n\
         You are helpful, thoughtful, and direct. Use the tools available to you\n\
         to assist with tasks. Report what you observe about your environment\n\
         when asked.\n",
        name = answers.agent_name
    );
    let soul_path = root.join(format!("nous/{}/SOUL.md", answers.agent_id));
    std::fs::write(&soul_path, soul)
        .with_context(|| format!("failed to write {}", soul_path.display()))?;

    Ok(())
}

fn render_config(a: &Answers) -> String {
    let workspace = format!("{}/nous/{}", a.root.display(), a.agent_id);
    format!(
        r#"# Aletheia Instance Configuration
# Config cascade: compiled defaults -> this file -> ALETHEIA_* env vars
# Full reference: docs/CONFIGURATION.md

# --- Gateway ---
gateway:
  port: 18789
  bind: {bind}
  auth:
    mode: {auth_mode}
  # tls:
  #   enabled: true
  #   certPath: config/tls/cert.pem
  #   keyPath: config/tls/key.pem
  # cors:
  #   allowedOrigins: ["https://my-dashboard.local"]
  # csrf:
  #   enabled: true

# --- Agents ---
agents:
  defaults:
    model:
      primary: {model}
    contextTokens: 200000
    maxOutputTokens: 16384
    userTimezone: {timezone}
    timeoutSeconds: 300
    # thinkingEnabled: false
    # thinkingBudget: 10000
    # maxToolIterations: 50
    # toolTimeouts:
    #   defaultMs: 120000

  list:
    - id: {agent_id}
      name: {agent_name}
      default: true
      workspace: {workspace}

# --- Channels ---
# channels:
#   signal:
#     enabled: true
#     accounts:
#       default:
#         account: "+1XXXXXXXXXX"
#         httpHost: localhost
#         httpPort: 8080

# --- Bindings (route messages to agents) ---
# bindings:
#   - channel: signal
#     source: "*"
#     nousId: {agent_id}

# --- Embedding (for recall/knowledge search) ---
# embedding:
#   provider: fastembed    # mock | fastembed
#   dimension: 384

# --- Data retention ---
# data:
#   retention:
#     sessionMaxAgeDays: 90
#     archiveBeforeDelete: true

# --- Maintenance ---
# maintenance:
#   traceRotation:
#     maxAgeDays: 14
#   dbMonitoring:
#     warnThresholdMb: 100

# --- Cost tracking ---
pricing:
  {model}:
    inputCostPerMtok: 3.0
    outputCostPerMtok: 15.0
"#,
        bind = a.bind,
        auth_mode = a.auth_mode,
        model = a.model,
        timezone = a.timezone,
        agent_id = a.agent_id,
        agent_name = a.agent_name,
        workspace = workspace,
    )
}

fn detect_timezone() -> String {
    jiff::tz::TimeZone::system()
        .iana_name()
        .map_or_else(|| "UTC".to_owned(), ToOwned::to_owned)
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

#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_answers_produce_valid_config() {
        let answers = Answers::default();
        let yaml = render_config(&answers);
        // Should be valid YAML that can be parsed
        let value: serde_yaml::Value = serde_yaml::from_str(&yaml)
            .expect("rendered config should be valid YAML");
        let gateway = &value["gateway"];
        assert_eq!(gateway["port"].as_u64(), Some(18789));
        assert_eq!(gateway["bind"].as_str(), Some("localhost"));
        assert_eq!(gateway["auth"]["mode"].as_str(), Some("none"));

        let agents = &value["agents"];
        assert_eq!(
            agents["defaults"]["model"]["primary"].as_str(),
            Some("claude-sonnet-4-6")
        );
        let list = agents["list"].as_sequence().expect("list should be sequence");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["id"].as_str(), Some("main"));
        assert_eq!(list[0]["name"].as_str(), Some("Main"));
    }

    #[test]
    fn scaffold_creates_structure() {
        let dir = tempfile::tempdir().unwrap();
        let answers = Answers {
            root: dir.path().to_path_buf(),
            api_key: Some("sk-ant-test-key".to_owned()),
            ..Answers::default()
        };
        scaffold(&answers).expect("scaffold should succeed");

        assert!(dir.path().join("config/aletheia.yaml").exists());
        assert!(dir.path().join("config/credentials/anthropic.json").exists());
        assert!(dir.path().join("nous/main/SOUL.md").exists());
        assert!(dir.path().join("data").is_dir());

        // Credential should contain the key
        let cred: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("config/credentials/anthropic.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cred["token"].as_str(), Some("sk-ant-test-key"));

        // SOUL.md should contain agent name
        let soul = std::fs::read_to_string(dir.path().join("nous/main/SOUL.md")).unwrap();
        assert!(soul.contains("Main"));
    }

    #[test]
    fn scaffold_without_api_key_skips_credential() {
        let dir = tempfile::tempdir().unwrap();
        let answers = Answers {
            root: dir.path().to_path_buf(),
            ..Answers::default()
        };
        scaffold(&answers).expect("scaffold should succeed");

        assert!(dir.path().join("config/aletheia.yaml").exists());
        assert!(!dir.path().join("config/credentials/anthropic.json").exists());
    }

    #[test]
    fn capitalize_works() {
        assert_eq!(capitalize("main"), "Main");
        assert_eq!(capitalize("test-agent"), "Test-agent");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("A"), "A");
    }

    #[cfg(unix)]
    #[test]
    fn credential_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let answers = Answers {
            root: dir.path().to_path_buf(),
            api_key: Some("sk-ant-test".to_owned()),
            ..Answers::default()
        };
        scaffold(&answers).unwrap();

        let cred_path = dir.path().join("config/credentials/anthropic.json");
        let mode = std::fs::metadata(&cred_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "credential file should be 0600");
    }
}
