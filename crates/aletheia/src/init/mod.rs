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
    #[cfg(feature = "tui")]
    #[snafu(display("setup wizard failed: {message}"))]
    TuiWizard {
        message: String,
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
    /// Credential resolution source: `"auto"`, `"api-key"`, or `"claude-code"`.
    pub(super) credential_source: String,
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
            credential_source: "auto".to_owned(),
        }
    }
}

fn build_non_interactive_answers(
    root: PathBuf,
    api_key: Option<SecretString>,
    api_provider: Option<String>,
    model: Option<String>,
    auth_mode: Option<String>,
) -> Answers {
    // WHY: explicit API key was provided; pin source to "api-key" so the
    // server loads the written credential file rather than falling through
    // the "auto" chain which may pick up a different credential.
    let credential_source = if api_key.is_some() {
        "api-key".to_owned()
    } else {
        tracing::warn!(
            "no API key provided — set --api-key or ANTHROPIC_API_KEY; server will start in degraded mode"
        );
        "auto".to_owned()
    };

    Answers {
        root,
        api_key,
        api_provider: api_provider.unwrap_or_else(|| "anthropic".to_owned()),
        model: model.unwrap_or_else(|| "claude-sonnet-4-6".to_owned()),
        auth_mode: auth_mode.unwrap_or_else(|| "none".to_owned()),
        credential_source,
        ..Answers::default()
    }
}

fn print_success_outro(root: &std::path::Path) -> Result<(), InitError> {
    cliclack::outro(format!(
        "Done! Start the server:\n\
         \n\
         \x1b[36m  aletheia -r {}\x1b[0m\n\
         \n\
         Then connect in another terminal:\n\
         \n\
         \x1b[36m  aletheia tui\x1b[0m",
        root.display()
    ))
    .context(PromptSnafu)
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
        build_non_interactive_answers(root, api_key, api_provider, model, auth_mode)
    } else if yes {
        // NOTE: lenient non-interactive: skip prompts, apply defaults for missing values
        let root = root.unwrap_or_else(|| PathBuf::from("./instance"));
        build_non_interactive_answers(root, api_key, api_provider, model, auth_mode)
    } else {
        let root = root.unwrap_or_else(|| PathBuf::from("./instance"));

        #[cfg(feature = "tui")]
        if theatron_tui::wizard::is_tty() {
            let preset_key = api_key.as_ref().map(|k| k.expose_secret().to_owned());
            let wa = theatron_tui::run_wizard(Some(root.clone()), preset_key).map_err(|e| {
                TuiWizardSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let root_path = wa.root.clone();
            let config_path = root_path.join("config/aletheia.toml");
            if config_path.exists() {
                let overwrite: bool = cliclack::confirm("Instance already exists. Overwrite?")
                    .initial_value(false)
                    .interact()
                    .context(PromptSnafu)?;
                if !overwrite {
                    cliclack::outro_cancel("Aborted.").context(PromptSnafu)?;
                    return Ok(());
                }
            }
            let answers = wizard_answers_to_answers(&wa);
            scaffold(&answers)?;
            write_user_profile_from_wizard(&wa)?;
            print_success_outro(&root_path)?;
            return Ok(());
        }

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
        print_success_outro(&answers.root)?;
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

#[cfg(feature = "tui")]
fn wizard_answers_to_answers(wa: &theatron_tui::wizard::WizardAnswers) -> Answers {
    use aletheia_koina::secret::SecretString;
    Answers {
        root: wa.root.clone(),
        api_key: wa.api_key.clone().map(SecretString::from),
        api_provider: wa.api_provider.clone(),
        model: wa.model.clone(),
        agent_id: wa.agent_id.clone(),
        agent_name: wa.agent_name.clone(),
        bind: wa.bind.clone(),
        auth_mode: wa.auth_mode.clone(),
        timezone: wa.timezone.clone(),
        credential_source: wa.credential_source.clone(),
    }
}

/// Write operator profile data collected by the TUI wizard into USER.md.
///
/// Replaces placeholder lines in the template with actual name, role, and
/// timezone so the agent has real context from first run.
// codequality:ignore — this function handles name/role/timezone only, not credentials
#[cfg(feature = "tui")]
#[expect(
    clippy::disallowed_methods,
    reason = "sync init wizard; no async runtime"
)]
fn write_user_profile_from_wizard(
    wa: &theatron_tui::wizard::WizardAnswers,
) -> Result<(), InitError> {
    if wa.user_name.is_empty() && wa.user_role.is_empty() {
        return Ok(());
    }

    let user_md_path = wa.root.join(format!("nous/{}/USER.md", wa.agent_id));
    if !user_md_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&user_md_path).context(WriteFileSnafu {
        path: user_md_path.clone(),
    })?;

    let updated = content
        .replace(
            "- **Name:** (learned from conversation)",
            &format!("- **Name:** {}", wa.user_name),
        )
        .replace(
            "- **Role:** (learned from conversation)",
            &format!("- **Role:** {}", wa.user_role),
        )
        .replace(
            "- **Timezone:** (detected from system or learned)",
            &format!("- **Timezone:** {}", wa.timezone),
        );

    std::fs::write(&user_md_path, &updated).context(WriteFileSnafu {
        path: user_md_path.clone(),
    })?;
    helpers::set_permissions(&user_md_path, 0o600)?;
    Ok(())
}
