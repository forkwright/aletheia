//! JWT validation tuning parameters.

use serde::{Deserialize, Serialize};

/// Deployment-tunable JWT validation parameters.
///
/// WHY configurable: clock drift between the issuer and validator (or NTP
/// jumps on the validator) can invalidate freshly issued tokens. Operators
/// running across multiple hosts or behind proxies may need to widen or
/// tighten the leeway. Mirrors
/// `symbolon::jwt::DEFAULT_CLOCK_SKEW_LEEWAY_SECS`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct JwtSettings {
    /// Clock skew tolerance in seconds applied when checking the `exp`
    /// claim. A token whose `exp` lies up to this many seconds in the past
    /// is still accepted. Valid range: 0–300. Default: 30.
    pub clock_skew_leeway_secs: u64,
}

impl Default for JwtSettings {
    fn default() -> Self {
        Self {
            // WHY: mirrors symbolon::jwt::DEFAULT_CLOCK_SKEW_LEEWAY_SECS.
            // Kept numeric rather than re-exporting the constant to avoid a
            // taxis -> symbolon dependency; the jwt_settings_default test
            // in config_tests.rs guards the two values against drift.
            clock_skew_leeway_secs: 30,
        }
    }
}
