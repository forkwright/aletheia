//! Validation helpers for `RuntimeBuilder::validate`.

use aletheia_koina::system::{Environment, RealSystem};
use aletheia_taxis::config::AletheiaConfig;
use aletheia_taxis::oikos::Oikos;

/// Validate JWT key configuration. Returns `true` if valid.
pub(super) fn validate_jwt(config: &AletheiaConfig) -> bool {
    let jwt_key = config
        .gateway
        .auth
        .signing_key
        .as_ref()
        .map(|s| s.expose_secret().to_owned())
        .or_else(|| RealSystem.var("ALETHEIA_JWT_SECRET"));
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
                return false;
            }
            Some(_) => println!("  [pass] {jwt_check_label}"),
        }
    } else {
        println!("  [pass] {jwt_check_label} (auth mode '{auth_mode}' -- JWT not required)");
    }
    true
}

/// Validate external tools configuration. Returns `true` if valid.
pub(super) fn validate_external_tools(oikos: &Oikos) -> bool {
    let tools_config = crate::external_tools::load_tools_config(oikos);
    let total = tools_config.required.len() + tools_config.optional.len();
    if total == 0 {
        return true;
    }

    let mut tools_ok = true;
    for (name, entry) in &tools_config.required {
        if entry.kind != crate::external_tools::ExternalToolKind::Builtin
            && entry.endpoint.is_none()
        {
            println!("  [FAIL] tools.required.{name}: missing endpoint");
            tools_ok = false;
        }
    }
    for (name, entry) in &tools_config.optional {
        if entry.kind != crate::external_tools::ExternalToolKind::Builtin
            && entry.endpoint.is_none()
        {
            println!("  [warn] tools.optional.{name}: missing endpoint");
        }
    }
    if tools_ok {
        println!(
            "  [pass] tools ({} required, {} optional)",
            tools_config.required.len(),
            tools_config.optional.len()
        );
    }
    tools_ok
}
