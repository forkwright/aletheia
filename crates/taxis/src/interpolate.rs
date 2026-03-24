//! Environment variable interpolation for TOML configuration strings.
//!
//! Supports two substitution forms processed before TOML deserialization:
//!
//! - `${VAR:-default}` -- substitutes `default` when `VAR` is unset
//! - `${VAR:?error message}` -- aborts startup with `error message` when `VAR` is unset
//!
//! Plain `${VAR}` without an operator substitutes the variable value or an empty
//! string if unset.
//!
//! Processing is a raw string pass before TOML parsing, so interpolation applies
//! to all positions in the file (values, comments, keys).

use std::env;

use crate::error::{EnvVarRequiredSnafu, EnvVarUnterminatedSnafu, Result};

/// Interpolate `${VAR:-default}` and `${VAR:?error}` expressions in a string.
///
/// Scans `content` left-to-right and replaces every `${}` expression.
/// Variable values are read from the real process environment.
///
/// # Substitution rules
///
/// | Syntax | `VAR` set | `VAR` unset |
/// |--------|-----------|-------------|
/// | `${VAR:-default}` | value of `VAR` | `default` string |
/// | `${VAR:?error message}` | value of `VAR` | abort with `error message` |
/// | `${VAR}` | value of `VAR` | empty string |
///
/// # Errors
///
/// Returns [`crate::error::Error::EnvVarRequired`] when a `${VAR:?message}` expression
/// is found and `VAR` is not set in the environment.
///
/// Returns [`crate::error::Error::EnvVarUnterminated`] when a `${` opener has no
/// matching `}`.
///
/// # Examples
///
/// ```
/// let out = aletheia_taxis::interpolate::interpolate_env_vars(
///     "[gateway]\nport = ${_TAXIS_UNSET_EXAMPLE:-18789}"
/// ).unwrap();
/// assert_eq!(out, "[gateway]\nport = 18789");
/// ```
#[expect(
    clippy::result_large_err,
    reason = "shared Error enum contains figment::Error; boxing would require a crate-wide change"
)]
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn interpolate_env_vars(content: &str) -> Result<String> {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;

    while let Some(dollar_pos) = rest.find("${") {
        // Copy everything before `${`.
        #[expect(
            clippy::string_slice,
            reason = "dollar_pos from str::find is a valid UTF-8 boundary"
        )]
        result.push_str(&rest[..dollar_pos]);
        #[expect(
            clippy::string_slice,
            reason = "dollar_pos + 2 skips ASCII '$' + '{', always a valid UTF-8 boundary"
        )]
        {
            rest = &rest[dollar_pos + 2..]; // skip `${`
        }

        // Find the closing `}`.
        let Some(close_pos) = rest.find('}') else {
            let excerpt: String = rest.chars().take(30).collect();
            return EnvVarUnterminatedSnafu { excerpt }.fail();
        };

        #[expect(
            clippy::string_slice,
            reason = "close_pos from str::find is a valid UTF-8 boundary"
        )]
        let expr = &rest[..close_pos];
        #[expect(
            clippy::string_slice,
            reason = "close_pos + 1 skips ASCII '}', always a valid UTF-8 boundary"
        )]
        {
            rest = &rest[close_pos + 1..]; // skip `}`
        }

        let substituted = resolve_expr(expr)?;
        result.push_str(&substituted);
    }

    // Copy the tail (no more `${` found).
    result.push_str(rest);
    Ok(result)
}

/// Resolve the expression body between `${` and `}`.
#[expect(
    clippy::result_large_err,
    reason = "shared Error enum contains figment::Error; boxing would require a crate-wide change"
)]
fn resolve_expr(expr: &str) -> Result<String> {
    if let Some(sep) = expr.find(":-") {
        // ${VAR:-default}: use default when VAR is unset.
        #[expect(
            clippy::string_slice,
            reason = "sep is a valid UTF-8 boundary returned by str::find on ASCII ':-'"
        )]
        let var = &expr[..sep];
        #[expect(
            clippy::string_slice,
            reason = "sep + 2 skips ASCII ':-', valid UTF-8 boundary"
        )]
        let default = &expr[sep + 2..];
        Ok(env::var(var).unwrap_or_else(|_| default.to_owned()))
    } else if let Some(sep) = expr.find(":?") {
        // ${VAR:?message}: abort when VAR is unset.
        #[expect(
            clippy::string_slice,
            reason = "sep is a valid UTF-8 boundary returned by str::find on ASCII ':?'"
        )]
        let var = &expr[..sep];
        #[expect(
            clippy::string_slice,
            reason = "sep + 2 skips ASCII ':?', valid UTF-8 boundary"
        )]
        let message = &expr[sep + 2..];
        env::var(var).map_err(|_env_err| {
            EnvVarRequiredSnafu {
                var: var.to_owned(),
                message: message.to_owned(),
            }
            .build()
        })
    } else {
        // ${VAR}: substitute value or empty string.
        Ok(env::var(expr).unwrap_or_default())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::result_large_err,
    reason = "figment::Jail closures return Box<dyn Error>; test error size doesn't matter"
)]
mod tests {
    use super::*;

    // NOTE: All tests that touch env vars run inside figment::Jail to isolate
    // them from the process environment and each other.

    #[test]
    fn no_placeholders_returns_content_unchanged() {
        let input = "[gateway]\nport = 18789\n";
        assert_eq!(
            interpolate_env_vars(input).unwrap(),
            input,
            "input with no placeholders should pass through unchanged"
        );
    }

    #[test]
    fn plain_var_substitutes_value_when_set() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("_TAX_INTERP_TEST_PORT", "9999");
            let out = interpolate_env_vars("port = ${_TAX_INTERP_TEST_PORT}")
                .map_err(|e| e.to_string())?;
            assert_eq!(out, "port = 9999", "set env var should be substituted");
            Ok(())
        });
    }

    #[test]
    fn plain_var_substitutes_empty_when_unset() {
        figment::Jail::expect_with(|_jail| {
            // NOTE: _TAX_INTERP_UNSET_XYZ is not set in the jail
            let out = interpolate_env_vars("val = ${_TAX_INTERP_UNSET_XYZ}")
                .map_err(|e| e.to_string())?;
            assert_eq!(out, "val = ", "unset var should substitute empty string");
            Ok(())
        });
    }

    #[test]
    fn default_used_when_var_unset() {
        figment::Jail::expect_with(|_jail| {
            // NOTE: _TAX_INTERP_MISSING is not set in the jail
            let out = interpolate_env_vars("port = ${_TAX_INTERP_MISSING:-18789}")
                .map_err(|e| e.to_string())?;
            assert_eq!(
                out, "port = 18789",
                "default value should be used when var is unset"
            );
            Ok(())
        });
    }

    #[test]
    fn default_not_used_when_var_set() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("_TAX_INTERP_PRESENT", "42");
            let out = interpolate_env_vars("port = ${_TAX_INTERP_PRESENT:-99}")
                .map_err(|e| e.to_string())?;
            assert_eq!(out, "port = 42", "set var should override default value");
            Ok(())
        });
    }

    #[test]
    fn required_var_aborts_when_unset() {
        figment::Jail::expect_with(|_jail| {
            // NOTE: _TAX_INTERP_REQUIRED is not set in the jail
            let result = interpolate_env_vars("key = ${_TAX_INTERP_REQUIRED:?API key must be set}");
            assert!(result.is_err(), "expected an error for unset required var");
            Ok(())
        });
    }

    #[test]
    fn required_var_error_contains_var_name_and_message() {
        figment::Jail::expect_with(|_jail| {
            // NOTE: _TAX_INTERP_REQUIRED2 is not set in the jail
            let result =
                interpolate_env_vars("key = ${_TAX_INTERP_REQUIRED2:?API key must be set}");
            assert!(result.is_err(), "expected error for unset required var");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("_TAX_INTERP_REQUIRED2"),
                "error should name the variable: {msg}"
            );
            assert!(
                msg.contains("API key must be set"),
                "error should include the user message: {msg}"
            );
            Ok(())
        });
    }

    #[test]
    fn required_var_succeeds_when_set() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("_TAX_INTERP_PRESENT2", "secret");
            let out = interpolate_env_vars("key = ${_TAX_INTERP_PRESENT2:?must be set}")
                .map_err(|e| e.to_string())?;
            assert_eq!(
                out, "key = secret",
                "required var should substitute its value when set"
            );
            Ok(())
        });
    }

    #[test]
    fn unterminated_ref_returns_error() {
        let err = interpolate_env_vars("port = ${UNCLOSED").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unterminated"),
            "error should say unterminated: {msg}"
        );
    }

    #[test]
    fn multiple_substitutions_in_one_string() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("_TAX_INTERP_HOST", "localhost");
            jail.set_env("_TAX_INTERP_PORT2", "8080");
            let out = interpolate_env_vars("bind = \"${_TAX_INTERP_HOST}:${_TAX_INTERP_PORT2}\"")
                .map_err(|e| e.to_string())?;
            assert_eq!(
                out, "bind = \"localhost:8080\"",
                "multiple substitutions should all resolve"
            );
            Ok(())
        });
    }

    #[test]
    fn default_value_containing_colon_is_preserved() {
        figment::Jail::expect_with(|_jail| {
            // The first `:-` is the operator; the rest is the default value.
            let out = interpolate_env_vars("url = ${_TAX_INTERP_URL:-http://localhost:8080}")
                .map_err(|e| e.to_string())?;
            assert_eq!(
                out, "url = http://localhost:8080",
                "colon in default value should be preserved"
            );
            Ok(())
        });
    }
}
