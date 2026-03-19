//! Interactive instance setup wizard.

use std::path::PathBuf;

use snafu::{ResultExt, Snafu};

use aletheia_koina::secret::SecretString;

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
    pub api_key: Option<SecretString>,
    pub auth_mode: Option<String>,
    pub api_provider: Option<String>,
    pub model: Option<String>,
}

/// User choices collected during the interactive (or non-interactive) flow.
pub(super) struct Answers {
    pub(super) root: PathBuf,
    pub(super) api_key: Option<SecretString>,
    pub(super) api_provider: String,
    pub(super) model: String,
    pub(super) agent_id: String,
    pub(super) agent_name: String,
    pub(super) bind: String,
    pub(super) auth_mode: String,
    pub(super) timezone: String,
}

impl Default for Answers {
    fn default() -> Self {
        Self {
            root: PathBuf::from("./instance"),
            api_key: None,
            api_provider: "anthropic".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            agent_id: "pronoea".to_owned(),
            agent_name: "Pronoea".to_owned(),
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
            tracing::warn!(
                "no API key provided — set --api-key or ANTHROPIC_API_KEY; server will start in degraded mode"
            );
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
            tracing::warn!(
                "no API key provided — set --api-key or ANTHROPIC_API_KEY; server will start in degraded mode"
            );
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
            tracing::info!(
                path = %answers.root.display(),
                "instance already exists — skipping (delete config/aletheia.toml to re-initialize)"
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
        tracing::info!(path = %answers.root.display(), "instance created");
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

fn collect_credential() -> Result<Option<SecretString>, InitError> {
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
            Ok(Some(SecretString::from(key)))
        }
        "env" => {
            let key = std::env::var("ANTHROPIC_API_KEY").context(MissingApiKeySnafu)?;
            Ok(Some(SecretString::from(key)))
        }
        _ => Ok(None),
    }
}

mod helpers;
mod scaffold;

use helpers::{capitalize, detect_timezone};
use scaffold::scaffold;
