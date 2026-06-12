//! Validation helpers for `RuntimeBuilder::validate`.

use koina::system::{Environment, RealSystem};
use taxis::config::AletheiaConfig;

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
                // kanon:ignore RUST/println-in-lib — CLI user-facing output, not log
                println!(
                    "  [FAIL] {jwt_check_label}: key is still the default placeholder\n         \
                     Set gateway.auth.signingKey in aletheia.toml or ALETHEIA_JWT_SECRET env var.\n         \
                     Generate one with: openssl rand -hex 32"
                );
                return false;
            }
            Some(_) => {
                // kanon:ignore RUST/println-in-lib — CLI user-facing output, not log
                println!("  [pass] {jwt_check_label}");
            }
        }
    } else {
        // kanon:ignore RUST/println-in-lib — CLI user-facing output, not log
        println!("  [pass] {jwt_check_label} (auth mode '{auth_mode}' -- JWT not required)");
    }
    true
}
