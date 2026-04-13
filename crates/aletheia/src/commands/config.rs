//! `aletheia config`: encryption key management, config encryption, and parameter description.

use std::path::PathBuf;

use clap::Subcommand;
use snafu::prelude::*;

use taxis::encrypt;
use taxis::oikos::Oikos;
use taxis::registry::{self, ParameterTier};

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Generate a new primary encryption key
    InitKey,
    /// Encrypt sensitive plaintext values in aletheia.toml
    Encrypt,
    /// List tunable parameters with metadata, bounds, and tuning guidance
    Describe {
        /// Filter by config section (e.g. "agents.defaults.behavior", "knowledge")
        #[arg(long)]
        section: Option<String>,
        /// Filter by affected subsystem (e.g. "distillation", "competence")
        #[arg(long)]
        affects: Option<String>,
        /// Show only self-tunable parameters
        #[arg(long)]
        self_tunable: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(action: &Action, instance_root: Option<&PathBuf>) -> Result<()> {
    match action {
        Action::InitKey => run_init_key(),
        Action::Encrypt => run_encrypt(instance_root),
        Action::Describe {
            section,
            affects,
            self_tunable,
            json,
        } => {
            run_describe(section.as_deref(), affects.as_deref(), *self_tunable, *json);
            Ok(())
        }
    }
}

fn run_init_key() -> Result<()> {
    let key_path = encrypt::primary_key_path()
        .ok_or_else(|| crate::error::Error::msg("cannot determine key path: HOME not set"))?;

    println!("Generating primary key at {}", key_path.display());
    encrypt::generate_primary_key(&key_path).whatever_context("failed to generate primary key")?;
    println!("Primary key generated.");
    println!("  Permissions: 0600 (owner read/write only)");
    println!(
        "  Back up this file securely. Without it, encrypted config values cannot be recovered."
    );
    Ok(())
}

fn run_encrypt(instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let key_path = encrypt::primary_key_path()
        .ok_or_else(|| crate::error::Error::msg("cannot determine key path: HOME not set"))?;

    let primary_key = encrypt::load_primary_key(&key_path)
        .whatever_context("failed to load primary key")?
        .ok_or_else(|| {
            crate::error::Error::msg(format!(
                "no primary key found at {}\n  Run `aletheia config init-key` first.",
                key_path.display()
            ))
        })?;

    let toml_path = oikos.config().join("aletheia.toml");
    if !toml_path.exists() {
        whatever!("config file not found: {}", toml_path.display());
    }

    let count = encrypt::encrypt_config_file(&toml_path, &primary_key)
        .whatever_context("failed to encrypt config file")?;

    if count == 0 {
        println!("No plaintext sensitive values found to encrypt.");
    } else {
        println!(
            "Encrypted {count} sensitive value(s) in {}",
            toml_path.display()
        );
    }
    Ok(())
}

fn run_describe(section: Option<&str>, affects: Option<&str>, self_tunable: bool, json: bool) {
    let all = registry::all_specs();

    let filtered: Vec<_> = all
        .iter()
        .filter(|s| {
            if let Some(sec) = section
                && !s.section.contains(sec)
            {
                return false;
            }
            if let Some(aff) = affects
                && !s.affects.contains(aff)
            {
                return false;
            }
            if self_tunable && s.tier != ParameterTier::SelfTuning {
                return false;
            }
            true
        })
        .collect();

    if json {
        // WHY: serde_json::to_string_pretty cannot fail on Vec<&ParameterSpec>
        // since all fields are serializable static types.
        let json_output = serde_json::to_string_pretty(&filtered)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
        println!("{json_output}");
        return;
    }

    if filtered.is_empty() {
        println!("No parameters match the given filters.");
        return;
    }

    println!("{} parameter(s) found:\n", filtered.len());

    for spec in &filtered {
        println!("{}", spec.key);
        println!("  Section:        {}", spec.section);
        println!("  Tier:           {}", spec.tier);
        println!("  Default:        {}", spec.default);
        if let Some((min, max)) = spec.bounds {
            println!("  Bounds:         [{min}, {max}]");
        }
        println!(
            "  Hot-reloadable: {}",
            if spec.hot_reloadable { "yes" } else { "no" }
        );
        println!("  Description:    {}", spec.description);
        println!("  Affects:        {}", spec.affects);
        println!("  Outcome signal: {}", spec.outcome_signal);
        println!("  Evidence:       {}", spec.evidence_required);
        println!("  Direction:      {}", spec.direction_hint);
        println!();
    }
}
