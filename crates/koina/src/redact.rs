//! Sensitive value redaction for log output.

use std::sync::LazyLock;

use regex::Regex;

static RE_ANTHROPIC_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-ant-api03-[A-Za-z0-9_-]+").expect("regex"));

static RE_SK_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9_-]{20,}").expect("regex"));

static RE_BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Bearer [A-Za-z0-9._-]+").expect("regex"));

static RE_JWT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+").expect("regex")
});

static RE_SECRETS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(password|secret|api_key|apikey)\s*[:=]\s*\S+").expect("regex")
});

/// Redact sensitive values (API keys, JWTs, bearer tokens, passwords) from a string.
#[must_use]
pub fn redact_sensitive(value: &str) -> String {
    let mut result = RE_ANTHROPIC_KEY.replace_all(value, "sk-ant-***").into_owned();
    result = RE_SK_KEY.replace_all(&result, "sk-***").into_owned();
    result = RE_BEARER.replace_all(&result, "Bearer ***").into_owned();
    result = RE_JWT.replace_all(&result, "[JWT REDACTED]").into_owned();
    result = RE_SECRETS.replace_all(&result, "$1=***").into_owned();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_anthropic_api_key() {
        let input = "using key sk-ant-api03-abcdef123456_789XYZ for requests";
        let output = redact_sensitive(input);
        assert_eq!(output, "using key sk-ant-*** for requests");
    }

    #[test]
    fn redacts_generic_sk_key() {
        let input = "key: sk-proj-abcdefghij1234567890abcdef";
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
