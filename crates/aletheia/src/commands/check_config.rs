//! `aletheia check-config`: validate configuration without starting any services.

use std::path::PathBuf;

use anyhow::Result;

use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;
use aletheia_taxis::validate::validate_section;

/// Run `aletheia check-config`: load config, validate all sections, report and exit.
///
/// Exits 0 on success, 1 if any check fails.
pub fn run(instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    println!("Instance root: {}", oikos.root().display());

    let mut all_ok = true;

    match oikos.validate() {
        Ok(()) => println!("  [pass] instance layout"),
        Err(e) => {
            println!("  [FAIL] instance layout: {e}");
            all_ok = false;
        }
    }

    let config = match load_config(&oikos) {
        Ok(c) => {
            println!("  [pass] config loaded");
            c
        }
        Err(e) => {
            println!("  [FAIL] config load: {e}");
            anyhow::bail!("config validation aborted: could not load config");
        }
    };

    let config_value = match serde_json::to_value(&config) {
        Ok(v) => v,
        Err(e) => {
            println!("  [FAIL] config serialization: {e}");
            anyhow::bail!("config validation aborted: could not serialize config");
        }
    };

    for section in &[
        "agents",
        "gateway",
        "maintenance",
        "data",
        "embedding",
        "channels",
        "bindings",
    ] {
        if let Some(section_value) = config_value.get(section) {
            match validate_section(section, section_value) {
                Ok(()) => println!("  [pass] {section}"),
                Err(e) => {
                    println!("  [FAIL] {section}: {e}");
                    all_ok = false;
                }
            }
        } else {
            println!("  [pass] {section} (using defaults)");
        }
    }

    for agent in &config.agents.list {
        match oikos.validate_workspace_path(&agent.workspace) {
            Ok(()) => println!("  [pass] agent '{}' workspace", agent.id),
            Err(e) => {
                println!("  [FAIL] agent '{}' workspace: {e}", agent.id);
                all_ok = false;
            }
        }
    }

    let jwt_key = config
        .gateway
        .auth
        .signing_key
        .as_deref()
        .map(str::to_owned)
        .or_else(|| std::env::var("ALETHEIA_JWT_SECRET").ok());
    let auth_mode = config.gateway.auth.mode.as_str();
    let jwt_check_label = "gateway.auth JWT key";
    if matches!(auth_mode, "token" | "jwt") {
        match jwt_key.as_deref() {
            Some("CHANGE-ME-IN-PRODUCTION") | None => {
                println!(
                    "  [FAIL] {jwt_check_label}: key is still the default placeholder\n         \
                     Set gateway.auth.signingKey in aletheia.toml or ALETHEIA_JWT_SECRET env var.\n         \
                     Generate one with: openssl rand -hex 32"
                );
                all_ok = false;
            }
            Some(_) => println!("  [pass] {jwt_check_label}"),
        }
    } else {
        println!("  [pass] {jwt_check_label} (auth mode '{auth_mode}' — JWT not required)");
    }

    println!();
    if all_ok {
        println!("Configuration OK");
        Ok(())
    } else {
        anyhow::bail!("Configuration has errors — see above");
    }
}
