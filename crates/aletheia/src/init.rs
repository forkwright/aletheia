//! Interactive instance setup wizard.

use std::path::{Path, PathBuf};

use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub(crate) enum InitError {
    #[snafu(display("interactive prompt failed"))]
    Prompt {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("ANTHROPIC_API_KEY not set"))]
    MissingApiKey {
        source: std::env::VarError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("failed to create directory {}", path.display()))]
    CreateDir {
        path: std::path::PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("failed to write {}", path.display()))]
    WriteFile {
        path: std::path::PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("failed to serialize credential JSON"))]
    SerializeJson {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("failed to set permissions on {}", path.display()))]
    SetPermissions {
        path: std::path::PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display(
        "--non-interactive requires {missing}\n\
         Set via flag or environment variable."
    ))]
    NonInteractiveMissingFlag {
        missing: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Arguments for [`run`].
pub(crate) struct RunArgs {
    pub root: Option<std::path::PathBuf>,
    /// Lenient non-interactive: skip prompts, apply defaults for missing values.
    pub yes: bool,
    /// Strict non-interactive: skip prompts, require --instance-path explicitly.
    pub non_interactive: bool,
    pub api_key: Option<String>,
    pub auth_mode: Option<String>,
    pub api_provider: Option<String>,
    pub model: Option<String>,
}

/// User choices collected during the interactive (or non-interactive) flow.
struct Answers {
    root: PathBuf,
    api_key: Option<String>,
    api_provider: String,
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
            api_provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            agent_id: "main".to_owned(),
            agent_name: "Main".to_owned(),
            bind: "localhost".to_owned(),
            auth_mode: "none".to_owned(),
            timezone: detect_timezone(),
        }
    }
}

pub(crate) fn run(args: RunArgs) -> Result<(), InitError> {
    let RunArgs {
        root,
        yes,
        non_interactive,
        api_key,
        auth_mode,
        api_provider,
        model,
    } = args;

    let is_non_interactive = non_interactive || yes;

    let answers = if non_interactive {
        // NOTE: strict non-interactive: --instance-path is required; everything else has defaults
        let root = root.ok_or_else(|| {
            NonInteractiveMissingFlagSnafu {
                missing: "--instance-path (or env ALETHEIA_INSTANCE_PATH)".to_owned(),
            }
            .build()
        })?;

        if api_key.is_none() {
            eprintln!("Warning: no API key provided. Set --api-key or ANTHROPIC_API_KEY.");
            eprintln!("         The server will start in degraded mode without credentials.");
        }

        Answers {
            root,
            api_key,
            api_provider: api_provider.unwrap_or_else(|| "anthropic".to_owned()),
            model: model.unwrap_or_else(|| "claude-sonnet-4-6".to_owned()),
            auth_mode: auth_mode.unwrap_or_else(|| "none".to_owned()),
            ..Answers::default()
        }
    } else if yes {
        // NOTE: lenient non-interactive: skip prompts, apply defaults for missing values
        let root = root.unwrap_or_else(|| PathBuf::from("./instance"));

        if api_key.is_none() {
            eprintln!("Warning: no API key provided. Set --api-key or ANTHROPIC_API_KEY.");
            eprintln!("         The server will start in degraded mode without credentials.");
        }

        Answers {
            root,
            api_key,
            api_provider: api_provider.unwrap_or_else(|| "anthropic".to_owned()),
            model: model.unwrap_or_else(|| "claude-sonnet-4-6".to_owned()),
            auth_mode: auth_mode.unwrap_or_else(|| "none".to_owned()),
            ..Answers::default()
        }
    } else {
        let root = root.unwrap_or_else(|| PathBuf::from("./instance"));
        collect_interactive(Answers {
            root,
            api_key,
            ..Answers::default()
        })?
    };

    let config_path = answers.root.join("config/aletheia.toml");
    if config_path.exists() {
        if is_non_interactive {
            println!(
                "Instance already exists at {}. Skipping (delete config/aletheia.toml to re-initialize).",
                answers.root.display()
            );
            return Ok(());
        }
        let overwrite: bool = cliclack::confirm("Instance already exists. Overwrite?")
            .initial_value(false)
            .interact()
            .context(PromptSnafu)?;
        if !overwrite {
            cliclack::outro_cancel("Aborted.").context(PromptSnafu)?;
            return Ok(());
        }
    }

    scaffold(&answers)?;

    if is_non_interactive {
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
        ))
        .context(PromptSnafu)?;
    }

    Ok(())
}

fn collect_interactive(mut answers: Answers) -> Result<Answers, InitError> {
    cliclack::intro("Aletheia Instance Setup").context(PromptSnafu)?;

    let root: String = cliclack::input("Instance root")
        .default_input(&answers.root.to_string_lossy())
        .interact()
        .context(PromptSnafu)?;
    answers.root = PathBuf::from(root);

    answers.api_key = collect_credential()?;

    let model: &str = cliclack::select("Default model")
        .item("claude-sonnet-4-6", "claude-sonnet-4-6 (recommended)", "")
        .item("claude-opus-4-6", "claude-opus-4-6", "")
        .item("claude-haiku-4-5", "claude-haiku-4-5", "")
        .interact()
        .context(PromptSnafu)?;
    model.clone_into(&mut answers.model);

    let agent_id: String = cliclack::input("Agent ID")
        .default_input(&answers.agent_id)
        .validate(|input: &String| {
            if input.is_empty() {
                Err("Agent ID cannot be empty")
            } else if !input
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                Err("Agent ID must be alphanumeric (hyphens and underscores allowed)")
            } else {
                Ok(())
            }
        })
        .interact()
        .context(PromptSnafu)?;
    answers.agent_id = agent_id;

    let default_name = capitalize(&answers.agent_id);
    let agent_name: String = cliclack::input("Agent display name")
        .default_input(&default_name)
        .interact()
        .context(PromptSnafu)?;
    answers.agent_name = agent_name;

    let bind: &str = cliclack::select("Gateway bind")
        .item("localhost", "localhost (this machine only)", "")
        .item("lan", "lan (network/Tailscale accessible)", "")
        .interact()
        .context(PromptSnafu)?;
    bind.clone_into(&mut answers.bind);

    let auth: &str = cliclack::select("Auth mode")
        .item("none", "none (no authentication — single user)", "")
        .item("token", "token (API key required to connect)", "")
        .interact()
        .context(PromptSnafu)?;
    auth.clone_into(&mut answers.auth_mode);

    let tz: String = cliclack::input("Timezone")
        .default_input(&answers.timezone)
        .interact()
        .context(PromptSnafu)?;
    answers.timezone = tz;

    Ok(answers)
}

fn collect_credential() -> Result<Option<String>, InitError> {
    let cred_choice: &str = cliclack::select("Anthropic API credential")
        .item("paste", "Paste API key", "")
        .item("env", "Use ANTHROPIC_API_KEY env var", "")
        .item("skip", "Skip (configure later)", "")
        .interact()
        .context(PromptSnafu)?;

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
                .interact()
                .context(PromptSnafu)?;
            Ok(Some(key))
        }
        "env" => {
            let key = std::env::var("ANTHROPIC_API_KEY").context(MissingApiKeySnafu)?;
            Ok(Some(key))
        }
        _ => Ok(None),
    }
}

fn scaffold(answers: &Answers) -> Result<(), InitError> {
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
    std::fs::write(&config_path, config_toml).context(WriteFileSnafu {
        path: config_path.clone(),
    })?;

    if let Some(ref key) = answers.api_key {
        let cred_path = root.join(format!("config/credentials/{}.json", answers.api_provider));
        let cred_json = serde_json::json!({ "token": key });
        let json_str = serde_json::to_string_pretty(&cred_json).context(SerializeJsonSnafu)?;
        std::fs::write(&cred_path, json_str).context(WriteFileSnafu {
            path: cred_path.clone(),
        })?;
        set_permissions(&cred_path, 0o600)?;
    }

    let soul = format!(
        "# {name}\n\n\
         You are {name}, an Aletheia cognitive agent.\n\n\
         You are helpful, thoughtful, and direct. Use the tools available to you\n\
         to assist with tasks. Report what you observe about your environment\n\
         when asked.\n",
        name = answers.agent_name
    );
    let soul_path = root.join(format!("nous/{}/SOUL.md", answers.agent_id));
    std::fs::write(&soul_path, soul).context(WriteFileSnafu {
        path: soul_path.clone(),
    })?;

    Ok(())
}

fn render_config(a: &Answers) -> String {
    let workspace = format!("{}/nous/{}", a.root.display(), a.agent_id);
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
# certPath = "config/tls/cert.pem"
# keyPath = "config/tls/key.pem"

# cors:
# [gateway.cors]
# allowedOrigins = ["https://my-dashboard.local"]

# csrf:
# [gateway.csrf]
# enabled = true

# --- Agents ---
[agents.defaults]
contextTokens = 200000
maxOutputTokens = 16384
userTimezone = "{timezone}"
timeoutSeconds = 300
# thinkingEnabled = false
# thinkingBudget = 10000
# maxToolIterations = 50
# toolTimeouts.defaultMs = 120000

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
# httpHost = "localhost"
# httpPort = 8080

# --- Bindings (route messages to agents) ---
# [[bindings]]
# channel = "signal"
# source = "*"
# nousId = "{agent_id}"

# --- Embedding (for recall/knowledge search) ---
# [embedding]
# provider = "candle"       # mock | candle
# dimension = 384

# --- Data retention ---
# [data.retention]
# sessionMaxAgeDays = 90
# archiveBeforeDelete = true

# --- Maintenance ---
# [maintenance.traceRotation]
# maxAgeDays = 14
# [maintenance.dbMonitoring]
# warnThresholdMb = 100

# --- Cost tracking ---
[pricing.{model}]
inputCostPerMtok = 3.0
outputCostPerMtok = 15.0
"#,
        bind = a.bind,
        auth_mode = a.auth_mode,
        model = a.model,
        timezone = a.timezone,
        agent_id = a.agent_id,
        agent_name = a.agent_name,
        workspace = workspace,
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
fn set_permissions(path: &Path, mode: u32) -> Result<(), InitError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).context(
        SetPermissionsSnafu {
            path: path.to_path_buf(),
        },
    )
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> Result<(), InitError> {
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_answers_produce_valid_config() {
        let answers = Answers::default();
        let toml_str = render_config(&answers);
        let value: toml::Value =
            toml::from_str(&toml_str).expect("rendered config should be valid TOML");
        let gateway = value.get("gateway").expect("has gateway");
        assert_eq!(
            gateway.get("port").and_then(toml::Value::as_integer),
            Some(18789)
        );
        assert_eq!(
            gateway.get("bind").and_then(toml::Value::as_str),
            Some("localhost")
        );
        assert_eq!(
            gateway
                .get("auth")
                .and_then(|v| v.get("mode"))
                .and_then(toml::Value::as_str),
            Some("none")
        );

        let agents = value.get("agents").expect("has agents");
        assert_eq!(
            agents
                .get("defaults")
                .and_then(|v| v.get("model"))
                .and_then(|v| v.get("primary"))
                .and_then(toml::Value::as_str),
            Some("claude-sonnet-4-6")
        );
        let list = agents
            .get("list")
            .and_then(toml::Value::as_array)
            .expect("list should be array");
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].get("id").and_then(toml::Value::as_str),
            Some("main")
        );
        assert_eq!(
            list[0].get("name").and_then(toml::Value::as_str),
            Some("Main")
        );
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

        assert!(dir.path().join("config/aletheia.toml").exists());
        assert!(
            dir.path()
                .join("config/credentials/anthropic.json")
                .exists()
        );
        assert!(dir.path().join("nous/main/SOUL.md").exists());
        assert!(dir.path().join("data").is_dir());
        assert!(dir.path().join("logs/traces").is_dir());
        assert!(dir.path().join("shared/coordination").is_dir());

        let cred: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("config/credentials/anthropic.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cred["token"].as_str(), Some("sk-ant-test-key"));

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

        assert!(dir.path().join("config/aletheia.toml").exists());
        assert!(
            !dir.path()
                .join("config/credentials/anthropic.json")
                .exists()
        );
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

    #[test]
    fn non_interactive_creates_valid_instance() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(RunArgs {
            root: Some(dir.path().to_path_buf()),
            yes: false,
            non_interactive: true,
            api_key: None,
            auth_mode: None,
            api_provider: None,
            model: None,
        });
        assert!(
            result.is_ok(),
            "non-interactive init should succeed: {result:?}"
        );

        assert!(dir.path().join("config/aletheia.toml").exists());
        assert!(dir.path().join("data").is_dir());
        assert!(dir.path().join("nous/main").is_dir());

        let config_str = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        let config: toml::Value = toml::from_str(&config_str).unwrap();
        assert_eq!(
            config["gateway"]["auth"]["mode"].as_str(),
            Some("none"),
            "default auth_mode should be none"
        );
        assert_eq!(
            config["agents"]["defaults"]["model"]["primary"].as_str(),
            Some("claude-sonnet-4-6"),
            "default model should be claude-sonnet-4-6"
        );
    }

    #[test]
    fn non_interactive_respects_explicit_flags() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(RunArgs {
            root: Some(dir.path().to_path_buf()),
            yes: false,
            non_interactive: true,
            api_key: Some("sk-ant-test-key".to_owned()),
            auth_mode: Some("token".to_owned()),
            api_provider: Some("anthropic".to_owned()),
            model: Some("claude-opus-4-6".to_owned()),
        });
        assert!(
            result.is_ok(),
            "non-interactive init should succeed: {result:?}"
        );

        let config_str = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        let config: toml::Value = toml::from_str(&config_str).unwrap();
        assert_eq!(
            config["gateway"]["auth"]["mode"].as_str(),
            Some("token"),
            "auth_mode should reflect --auth-mode flag"
        );
        assert_eq!(
            config["agents"]["defaults"]["model"]["primary"].as_str(),
            Some("claude-opus-4-6"),
            "model should reflect --model flag"
        );

        let cred: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("config/credentials/anthropic.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cred["token"].as_str(), Some("sk-ant-test-key"));
    }

    #[test]
    fn non_interactive_without_instance_path_returns_error() {
        let result = run(RunArgs {
            root: None,
            yes: false,
            non_interactive: true,
            api_key: None,
            auth_mode: None,
            api_provider: None,
            model: None,
        });
        assert!(
            result.is_err(),
            "missing --instance-path should be an error"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("--instance-path") || msg.contains("ALETHEIA_INSTANCE_PATH"),
            "error should name the missing flag: {msg}"
        );
    }

    #[test]
    fn render_config_includes_permissive_sandbox_defaults() {
        let answers = Answers::default();
        let toml_str = render_config(&answers);
        let value: toml::Value =
            toml::from_str(&toml_str).expect("rendered config should be valid TOML");

        let sandbox = value
            .get("sandbox")
            .expect("sandbox section must be present");
        assert_eq!(
            sandbox.get("enforcement").and_then(toml::Value::as_str),
            Some("permissive"),
            "single-agent init must use permissive enforcement"
        );
        let exec_paths = sandbox
            .get("extraExecPaths")
            .and_then(toml::Value::as_array)
            .expect("extraExecPaths must be an array");
        assert!(
            exec_paths.iter().any(|v| v.as_str() == Some("~")),
            "extraExecPaths must include ~ for home directory exec access"
        );
    }

    #[test]
    fn yes_flag_uses_default_instance_path() {
        let dir = tempfile::tempdir().unwrap();
        // NOTE: provide explicit path to avoid writing to real cwd
        let result = run(RunArgs {
            root: Some(dir.path().to_path_buf()),
            yes: true,
            non_interactive: false,
            api_key: None,
            auth_mode: None,
            api_provider: None,
            model: None,
        });
        assert!(result.is_ok(), "-y init should succeed: {result:?}");
        assert!(dir.path().join("config/aletheia.toml").exists());
    }
}
