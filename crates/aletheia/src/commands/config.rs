//! `aletheia config`: encryption key management and config encryption.

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;

use aletheia_taxis::encrypt;
use aletheia_taxis::oikos::Oikos;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Generate a new primary encryption key
    InitKey,
    /// Encrypt sensitive plaintext values in aletheia.toml
    Encrypt,
}

pub(crate) fn run(action: &Action, instance_root: Option<&PathBuf>) -> Result<()> {
    match action {
        Action::InitKey => run_init_key(),
        Action::Encrypt => run_encrypt(instance_root),
    }
}

fn run_init_key() -> Result<()> {
    let key_path = encrypt::primary_key_path()
        .ok_or_else(|| anyhow::anyhow!("cannot determine key path: HOME not set"))?;

    println!("Generating primary key at {}", key_path.display());
    encrypt::generate_primary_key(&key_path)?;
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
        .ok_or_else(|| anyhow::anyhow!("cannot determine key path: HOME not set"))?;

    let primary_key = encrypt::load_primary_key(&key_path)?.ok_or_else(|| {
        anyhow::anyhow!(
            "no primary key found at {}\n  Run `aletheia config init-key` first.",
            key_path.display()
        )
    })?;

    let toml_path = oikos.config().join("aletheia.toml");
    if !toml_path.exists() {
        anyhow::bail!("config file not found: {}", toml_path.display());
    }

    let count = encrypt::encrypt_config_file(&toml_path, &primary_key)?;

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
