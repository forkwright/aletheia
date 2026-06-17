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
    r"(?i)(password|secret|api_key|apikey)\s*[:=]\s*\S+"
);

/// Redact sensitive values (API keys, JWTs, bearer tokens, passwords) from a string.
#[must_use]
pub fn redact_sensitive(value: &str) -> String {
    let mut result = replace_sensitive(&RE_ANTHROPIC_KEY, value, "sk-ant-***");
    result = replace_sensitive(&RE_SK_KEY, &result, "sk-***");
    result = replace_sensitive(&RE_BEARER, &result, "Bearer ***");
    result = replace_sensitive(&RE_JWT, &result, "[JWT REDACTED]");
    result = replace_sensitive(&RE_SECRETS, &result, "$1=***");
    result
}

fn replace_sensitive(regex: &LazyLock<Regex>, value: &str, replacement: &str) -> String {
    regex.replace_all(value, replacement).into_owned()
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
}
