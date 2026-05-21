//! Sensitive value redaction for log output.
//!
//! Strips API keys (Anthropic `sk-ant-*`, generic `sk-*`), bearer tokens,
//! JWTs, and password-like key=value pairs from strings before they reach logs.

use std::sync::LazyLock;

use regex::Regex;

/// Compile a static regex from a literal pattern.
macro_rules! static_regex {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new($pattern).ok());
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
pub(crate) fn redact_sensitive(value: &str) -> String {
    let mut result = replace_sensitive(&RE_ANTHROPIC_KEY, value, "sk-ant-***");
    result = replace_sensitive(&RE_SK_KEY, &result, "sk-***");
    result = replace_sensitive(&RE_BEARER, &result, "Bearer ***");
    result = replace_sensitive(&RE_JWT, &result, "[JWT REDACTED]");
    result = replace_sensitive(&RE_SECRETS, &result, "$1=***");
    result
}

fn replace_sensitive(regex: &LazyLock<Option<Regex>>, value: &str, replacement: &str) -> String {
    match regex.as_ref() {
        Some(regex) => regex.replace_all(value, replacement).into_owned(),
        None => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_anthropic_api_key() {
        let input = "using key sk-ant-api03-abcdef123456_789XYZ for requests"; // pii-allow: synthetic Anthropic key shape, redactor self-test
        let output = redact_sensitive(input);
        assert_eq!(output, "using key sk-ant-*** for requests");
    }

    #[test]
    fn redacts_generic_sk_key() {
        let input = "key: sk-proj-abcdefghij1234567890abcdef"; // pii-allow: synthetic OpenAI proj-token shape, redactor self-test
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
    fn handles_multiple_sensitive_values() {
        let input = "key=sk-ant-api03-abc123 and password=secret123";
        let output = redact_sensitive(input);
        assert!(output.contains("sk-ant-***"));
        assert!(!output.contains("abc123"));
        assert!(!output.contains("secret123"));
    }
}
