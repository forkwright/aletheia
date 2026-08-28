//! Sensitive value redaction for log output.
//!
//! Strips API keys (Anthropic `sk-ant-*`, generic `sk-*`), bearer tokens,
//! JWTs, and password-like key=value pairs from strings before they reach logs.

use std::sync::LazyLock;

use regex::Regex;

/// Compile a static regex from a literal pattern.
macro_rules! static_regex {
    ($name:ident, $pattern:expr) => {
        // WHY (#5603): these patterns are compile-time constants. A regex that
        // fails to compile is a programmer error; failing closed (panic on
        // first access) prevents silent credential leakage through logs.
        #[allow(
            clippy::expect_used,
            reason = "static regex patterns are compile-time constants and must be valid"
        )]
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pattern).expect("BUG: static regex must compile"));
    };
}

static_regex!(RE_ANTHROPIC_KEY, r"sk-ant-api03-[A-Za-z0-9_-]+");
static_regex!(RE_SK_KEY, r"sk-[A-Za-z0-9_-]{20,}");
static_regex!(RE_BEARER, r"Bearer [A-Za-z0-9._-]+");
static_regex!(RE_JWT, r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+");
static_regex!(
    RE_SECRETS,
    // WHY (#6003): redact double-quoted and single-quoted values even when
    // they contain spaces, as well as unquoted non-whitespace values.
    r#"(?i)(password|secret|api_key|apikey)\s*[:=]\s*("[^"]*"|'[^']*'|\S+)"#
);

/// Redact sensitive values (API keys, JWTs, bearer tokens, passwords) from a string.
#[must_use]
pub fn redact_sensitive(value: &str) -> String {
    let mut result = replace_sensitive(&RE_BEARER, value, "Bearer ***");
    // JWT segments are base64url and can contain key-like substrings such as
    // `sk-...`; redact the full JWT before narrower API-key patterns split it.
    result = replace_sensitive(&RE_JWT, &result, "[JWT REDACTED]");
    result = replace_sensitive(&RE_ANTHROPIC_KEY, &result, "sk-ant-***");
    result = replace_sensitive(&RE_SK_KEY, &result, "sk-***");
    result = replace_sensitive(&RE_SECRETS, &result, "$1=***");
    result
}

fn replace_sensitive(regex: &LazyLock<Regex>, value: &str, replacement: &str) -> String {
    regex.replace_all(value, replacement).into_owned()
}

/// Canonical credential-bearing CLI flag names, checked after stripping
/// leading dashes and internal `-`/`_` separators (so `--api-key`,
/// `--api_key`, and `--apikey` all normalize to `apikey`).
///
/// WHY(#7020): union of the two argv-redaction policies this replaces
/// (`eval::provenance::is_secret_flag`, `agora::command::is_sensitive_arg_name`)
/// — `passphrase` was Agora-only and `ak-`/`pk-`-prefixed bare values were
/// Eval-only, so identical commands redacted differently depending on which
/// recorder observed them.
const SECRET_FLAG_NAMES: &[&str] = &[
    "apikey",
    "bearer",
    "passphrase",
    "password",
    "secret",
    "token",
    "key",
];

/// Subset of [`SECRET_FLAG_NAMES`] matched by suffix as well as by exact
/// name, so `--judge-api-key` and `--sig-token` are covered without a
/// generic `key`/`bearer`/`passphrase` suffix match turning `--monkey` or
/// `--turkey` into a false positive.
const SECRET_FLAG_SUFFIXES: &[&str] = &["apikey", "password", "secret", "token"];

/// Bare-value prefixes (case-insensitive) that mark an argv token as a
/// credential regardless of the preceding flag name.
///
/// WHY(#7020): union of `sk-`/`ak-`/`pk-`/`bearer ` (Eval) and `sk-`/`xox`/
/// `ghp_` (Agora).
const SECRET_VALUE_PREFIXES: &[&str] = &["sk-", "ak-", "pk-", "bearer ", "xox", "ghp_"];

/// Minimum length for an unprefixed token to be treated as an opaque secret
/// (long alphanumeric/`_`/`-`/`.` string, e.g. a bare API key or JWT-shaped
/// value with no recognizable flag or prefix).
const OPAQUE_SECRET_MIN_LEN: usize = 32;

/// True when `name` (a flag with leading dashes already stripped) names a
/// credential-bearing CLI option under the canonical policy.
#[must_use]
fn is_secret_flag_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['-', '_'], "");
    SECRET_FLAG_NAMES.contains(&normalized.as_str())
        || SECRET_FLAG_SUFFIXES
            .iter()
            .any(|suffix| normalized.ends_with(suffix))
}

/// True when `value` looks like a bare credential (no flag context): a
/// recognized key-prefix shape, or a long opaque alphanumeric/`_`/`-`/`.`
/// token.
#[must_use]
fn looks_like_secret_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if SECRET_VALUE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }
    value.len() >= OPAQUE_SECRET_MIN_LEN
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Redact secret-bearing CLI argument tokens under the canonical
/// flag/value-shape policy.
///
/// A token matching [`is_secret_flag_name`] (after stripping leading dashes,
/// and either as `--flag=value` or `--flag value`) has its value replaced
/// with `[REDACTED]`; the flag name itself is preserved. A bare token
/// matching [`looks_like_secret_value`] is replaced outright.
///
/// WHY(#7020): the single argv-redaction algorithm shared by Eval
/// (`EvalProvenance::redacted_args`) and Agora (`Command::redact_args`),
/// which previously reimplemented this with divergent policy tables —
/// identical commands persisted with different credential coverage
/// depending on which crate recorded them.
#[must_use]
pub fn redact_argv<'a>(tokens: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut redact_next = false;

    for token in tokens {
        if redact_next {
            out.push("[REDACTED]".to_owned());
            redact_next = false;
            continue;
        }

        if let Some((flag, _value)) = token.split_once('=')
            && is_secret_flag_name(flag.trim_start_matches('-'))
        {
            out.push(format!("{flag}=[REDACTED]"));
            continue;
        }

        if is_secret_flag_name(token.trim_start_matches('-')) {
            out.push(token.to_owned());
            redact_next = true;
            continue;
        }

        if looks_like_secret_value(token) {
            out.push("[REDACTED]".to_owned());
            continue;
        }

        out.push(token.to_owned());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_anthropic_api_key() {
        let input = "using key sk-ant-api03-abcdef123456_789XYZ for requests"; // kanon:ignore SECURITY/hardcoded-openai-api-key + gitleaks:allow + trufflehog:ignore -- synthetic key shape used by redaction self-test; not a real credential
        let output = redact_sensitive(input);
        assert_eq!(output, "using key sk-ant-*** for requests");
    }

    #[test]
    fn redacts_generic_sk_key() {
        let input = "key: sk-proj-abcdefghij1234567890abcdef"; // kanon:ignore SECURITY/hardcoded-openai-api-key + gitleaks:allow + trufflehog:ignore -- synthetic key shape used by redaction self-test; not a real credential
        let output = redact_sensitive(input);
        assert_eq!(output, "key: sk-***");
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Authorization: Bearer abc123def456.ghi789";
        let output = redact_sensitive(input);
        assert_eq!(output, "Authorization: Bearer ***");
    }

    #[test]
    fn redacts_jwt() {
        let input = "token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let output = redact_sensitive(input);
        assert!(output.contains("[JWT REDACTED]"));
        assert!(!output.contains("dozjgNryP4J3jVmNHl0w5N"));
    }

    #[test]
    fn redacts_jwt_with_key_like_segment() {
        let input = "token=eyJsk-AAAAAAAAAAAAAAAAAAAA.A.A";
        let output = redact_sensitive(input);
        assert_eq!(output, "token=[JWT REDACTED]");
    }

    #[test]
    fn redacts_password_patterns() {
        assert!(redact_sensitive("password=hunter2").contains("***"));
        assert!(redact_sensitive("secret: my-secret-value").contains("***"));
        assert!(redact_sensitive("api_key=sk123abc").contains("***"));
        assert!(redact_sensitive("APIKEY: tok_live_abc").contains("***"));
    }

    #[test]
    fn leaves_safe_strings_unchanged() {
        let safe = "normal log message with session_id=abc123";
        assert_eq!(redact_sensitive(safe), safe);
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(redact_sensitive(""), "");
    }

    #[test]
    #[should_panic(expected = "BUG: static regex must compile")]
    #[expect(
        clippy::invalid_regex,
        reason = "intentionally malformed regex to verify fail-closed behavior"
    )]
    fn invalid_regex_pattern_panics_fail_closed() {
        // WHY (#5603): a malformed static regex must never fall back to
        // returning the original string, which would leak credentials. This
        // test would *pass* (incorrectly) under the old fail-open code.
        static_regex!(RE_INVALID, r"(?<unclosed");
        let _ = replace_sensitive(&RE_INVALID, "secret", "***");
    }

    #[test]
    fn handles_multiple_sensitive_values() {
        let input = "key=sk-ant-api03-abc123 and password=secret123"; // kanon:ignore SECURITY/hardcoded-openai-api-key -- synthetic key shape used by redaction self-test; not a real credential
        let output = redact_sensitive(input);
        assert!(output.contains("sk-ant-***"));
        assert!(!output.contains("abc123"));
        assert!(!output.contains("secret123"));
    }

    #[test]
    fn redact_argv_token_next_value() {
        let args = ["--url", "http://localhost", "--token", "super-secret"];
        let redacted = redact_argv(args);
        assert!(redacted.contains(&"[REDACTED]".to_owned()));
        assert!(!redacted.contains(&"super-secret".to_owned()));
    }

    #[test]
    fn redact_argv_token_equals() {
        let redacted = redact_argv(["--token=super-secret"]);
        assert_eq!(redacted, vec!["--token=[REDACTED]"]);
    }

    #[test]
    fn redact_argv_judge_api_key_suffix() {
        let redacted = redact_argv(["--judge-api-key", "sk-abc123"]);
        assert_eq!(redacted, vec!["--judge-api-key", "[REDACTED]"]);
    }

    #[test]
    fn redact_argv_passphrase() {
        let redacted = redact_argv(["--passphrase", "hunter2"]);
        assert_eq!(redacted, vec!["--passphrase", "[REDACTED]"]);
    }

    #[test]
    fn redact_argv_keeps_safe_values() {
        let args = ["--url", "http://localhost", "--timeout", "30"];
        let expected: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(redact_argv(args), expected);
    }

    #[test]
    fn redact_argv_detects_ak_and_pk_prefixed_bare_values() {
        assert_eq!(redact_argv(["ak-abc123def456"]), vec!["[REDACTED]"]);
        assert_eq!(redact_argv(["pk-abc123def456"]), vec!["[REDACTED]"]);
    }

    #[test]
    fn redact_argv_detects_xox_and_ghp_prefixed_bare_values() {
        assert_eq!(redact_argv(["xoxb-abc123def456"]), vec!["[REDACTED]"]);
        assert_eq!(redact_argv(["ghp_abc123def456"]), vec!["[REDACTED]"]);
    }

    #[test]
    fn redact_argv_does_not_false_positive_on_key_suffixed_flag() {
        // WHY(#7020): "key" is exact-matched, not suffix-matched, so an
        // unrelated flag ending in "key" is left alone.
        let redacted = redact_argv(["--monkey", "banana"]);
        assert_eq!(redacted, vec!["--monkey", "banana"]);
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "proptest assertions")]
mod proptests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;

    const ALPHANUM_HYPHEN_UNDERSCORE: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    const BASE64URL_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    const BEARER_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-";
    const SECRET_VALUE_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-.~+/=";
    const QUOTED_VALUE_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 _-.~+/=";

    const WRAPPERS: &[&str] = &["", " ", "\"", "'", "\n"];
    const SECRET_KEYS: &[&str] = &["password", "secret", "api_key", "apikey"];
    const SEPARATORS: &[&str] = &["=", ":", " = ", " : "];

    fn wrapper() -> impl Strategy<Value = &'static str> {
        (0..WRAPPERS.len()).prop_map(|i| *WRAPPERS.get(i).unwrap())
    }

    fn secret_key() -> impl Strategy<Value = &'static str> {
        (0..SECRET_KEYS.len()).prop_map(|i| *SECRET_KEYS.get(i).unwrap())
    }

    fn separator() -> impl Strategy<Value = &'static str> {
        (0..SEPARATORS.len()).prop_map(|i| *SEPARATORS.get(i).unwrap())
    }

    fn secret_body(
        range: std::ops::Range<usize>,
        allowed: &'static str,
    ) -> impl Strategy<Value = String> {
        let char_count = allowed.chars().count();
        vec(
            (0..char_count).prop_map(move |i| {
                allowed
                    .chars()
                    .nth(i)
                    // WHY: `i` is drawn from `0..allowed.chars().count()`.
                    .unwrap()
            }),
            range,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    proptest! {
        #[test]
        fn prop_redacts_anthropic_key(
            prefix in wrapper(),
            suffix in wrapper(),
            body in secret_body(1..128usize, ALPHANUM_HYPHEN_UNDERSCORE),
        ) {
            let secret = format!("sk-ant-api03-{body}");
            let input = format!("{prefix}{secret}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("sk-ant-***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full key format is absent, not just body chars — single
            // chars from ALPHANUM_HYPHEN_UNDERSCORE can appear in "sk-ant-***" itself.
            prop_assert!(!output.contains(secret.as_str()), "secret leaked: {}", output);
        }

        #[test]
        fn prop_redacts_generic_sk_key(
            prefix in wrapper(),
            suffix in wrapper(),
            body in secret_body(20..128usize, ALPHANUM_HYPHEN_UNDERSCORE),
        ) {
            let secret = format!("sk-{body}");
            let input = format!("{prefix}{secret}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("sk-***"), "placeholder missing: {}", output);
            prop_assert!(!output.contains(&body), "secret body leaked: {}", output);
        }

        #[test]
        fn prop_leaves_short_sk_key_unredacted(
            prefix in wrapper(),
            suffix in wrapper(),
            body in secret_body(0..19usize, ALPHANUM_HYPHEN_UNDERSCORE),
        ) {
            let input = format!("{prefix}sk-{body}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert_eq!(output, input);
        }

        #[test]
        fn prop_redacts_bearer_token(
            prefix in wrapper(),
            suffix in wrapper(),
            token in secret_body(1..128usize, BEARER_CHARS),
        ) {
            let input = format!("{prefix}Bearer {token}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("Bearer ***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full bearer string is absent — single BEARER_CHARS
            // like "B","e","a","r" appear in the "Bearer ***" placeholder itself.
            prop_assert!(!output.contains(&format!("Bearer {token}")), "token leaked: {}", output);
        }

        #[test]
        fn prop_redacts_jwt(
            prefix in wrapper(),
            suffix in wrapper(),
            header in secret_body(1..64usize, BASE64URL_CHARS),
            payload in secret_body(1..64usize, BASE64URL_CHARS),
            signature in secret_body(1..64usize, BASE64URL_CHARS),
        ) {
            let token = format!("eyJ{header}.{payload}.{signature}");
            let input = format!("{prefix}{token}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(
                output.contains("[JWT REDACTED]"),
                "placeholder missing: {}",
                output
            );
            // WHY (#6003): check the full token is absent rather than individual parts —
            // "[JWT REDACTED]" contains chars (J,W,T,R,E,D,A,C) that are valid BASE64URL
            // chars, so a single-char header/payload/signature would cause a false failure.
            prop_assert!(!output.contains(&token), "JWT token leaked: {}", output);
        }

        #[test]
        fn prop_redacts_key_value_secret(
            prefix in wrapper(),
            suffix in wrapper(),
            key in secret_key(),
            sep in separator(),
            value in secret_body(1..64usize, SECRET_VALUE_CHARS),
        ) {
            let input = format!("{prefix}{key}{sep}{value}{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full key+sep+value context is absent — value chars like
            // "p" appear in the retained key name ("password=***") causing false failures.
            let full_secret = format!("{key}{sep}{value}");
            prop_assert!(!output.contains(&full_secret), "value leaked in context: {}", output);
        }

        #[test]
        fn prop_redacts_quoted_key_value_secret_with_spaces(
            prefix in wrapper(),
            suffix in wrapper(),
            key in secret_key(),
            sep in separator(),
            value in secret_body(1..64usize, QUOTED_VALUE_CHARS),
        ) {
            let input = format!("{prefix}{key}{sep}\"{value}\"{suffix}");
            let output = redact_sensitive(&input);
            prop_assert!(output.contains("***"), "placeholder missing: {}", output);
            // WHY (#6003): check the full key+sep+"value" context is absent — same
            // single-char leak as prop_redacts_key_value_secret.
            let full_secret = format!("{key}{sep}\"{value}\"");
            prop_assert!(!output.contains(&full_secret), "quoted value leaked: {}", output);
        }
    }
}
