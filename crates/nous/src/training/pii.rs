//! PII and secret redaction for training data capture.
//!
//! Applied to `user_message` and `assistant_response` before a
//! `TrainingRecord` is written to disk. Redaction is token-preserving:
//! matches are replaced with `[REDACTED:<kind>]` so the structure of
//! the original text remains intact for downstream tokenization and
//! sequence-length statistics.
//!
//! # Threat model
//!
//! The PII filter is a *training-time* safeguard, not a commit-time
//! scanner. Its goal is to prevent the following from being written
//! into a JSONL corpus that may later be shared or uploaded to a
//! third-party fine-tuning service:
//!
//! - Email addresses
//! - Phone numbers (E.164 and common US formats)
//! - SSNs (`XXX-XX-XXXX`)
//! - Credit card numbers (13–19 digits, with or without separators)
//! - Anthropic API keys (`sk-ant-*`)
//! - AWS access key IDs and secret keys
//! - `OpenAI` / generic `sk-` bearer tokens
//! - Google API keys (`AIza*`)
//! - GitHub personal access tokens (`ghp_*`, `gho_*`, `ghu_*`, `ghs_*`, `ghr_*`)
//! - JWTs (`eyJ*.*.*`)
//! - Lines that assign to a secret-shaped env var name
//!   (e.g. `API_KEY=...`, `SECRET=...`, `TOKEN=...`, `PASSWORD=...`)
//!
//! # Ordering
//!
//! Patterns are applied in order of specificity (most-specific first)
//! so that a Google API key is redacted as `google_api_key` rather
//! than falling through to the generic `api_key` pattern. See
//! [`PATTERNS`] for the authoritative order.

use std::sync::LazyLock;

use regex::Regex;

/// Redaction marker template. Callers produce the final marker via
/// [`marker`].
const MARKER_PREFIX: &str = "[REDACTED:";
const MARKER_SUFFIX: &str = "]";

/// Build the redaction marker for a given kind.
#[must_use]
pub fn marker(kind: &str) -> String {
    format!("{MARKER_PREFIX}{kind}{MARKER_SUFFIX}")
}

/// A single PII/secret pattern with its redaction kind label.
struct Pattern {
    /// Stable short label used in the redaction marker
    /// (e.g. `"email"`, `"api_key"`).
    kind: &'static str,
    /// Compiled regex matching the PII to redact.
    re: Regex,
}

/// Compile a regex literal known at compile time to be valid.
fn compile(re: &str) -> Regex {
    #[expect(
        clippy::expect_used,
        reason = "compile-time-constant regex literals cannot fail"
    )]
    {
        Regex::new(re).expect("compile-time-constant regex literals cannot fail")
    }
}

/// Ordered list of redaction patterns.
///
/// WHY order matters: more-specific prefixes (Anthropic, `OpenAI`,
/// Google, GitHub, AWS) MUST be evaluated before the generic
/// `api_key` fallback so they produce precise kind labels.
///
/// WHY `LazyLock<Vec<Pattern>>` instead of `&'static [Pattern]`:
/// `Regex` owns interior-mutable match state, so a plain static of
/// `Pattern` values is rejected by the compiler. `LazyLock` compiles
/// the patterns once on first use.
static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    vec![
        // ── Anthropic API keys ─────────────────────────────────────
        Pattern {
            kind: "anthropic_api_key",
            // sk-ant-<tokentype>-<base64url>
            re: compile(r"sk-ant-[A-Za-z0-9_\-]{16,}"),
        },
        // ── `OpenAI` / generic sk- bearer tokens ─────────────────────
        Pattern {
            kind: "openai_api_key",
            re: compile(r"sk-(?:proj-)?[A-Za-z0-9_\-]{20,}"),
        },
        // ── Google API keys ────────────────────────────────────────
        Pattern {
            kind: "google_api_key",
            re: compile(r"AIza[0-9A-Za-z_\-]{35}"),
        },
        // ── GitHub tokens ──────────────────────────────────────────
        Pattern {
            kind: "github_token",
            re: compile(r"gh[pousr]_[A-Za-z0-9]{20,}"),
        },
        // ── AWS access key IDs ─────────────────────────────────────
        Pattern {
            kind: "aws_access_key_id",
            re: compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
        },
        // ── AWS secret access key (heuristic: 40-char base64) ─────
        // WHY: secrets don't have a unique prefix. We match only
        // when preceded by an assignment-like token to avoid false
        // positives on random 40-char strings.
        Pattern {
            kind: "aws_secret_access_key",
            re: compile(
                r"(?i)aws_?secret(?:_access)?_?key[\s]*[:=][\s]*['\x22]?[A-Za-z0-9/+=]{40}['\x22]?",
            ),
        },
        // ── JWT (three base64url segments separated by `.`) ───────
        Pattern {
            kind: "jwt",
            re: compile(r"eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}"),
        },
        // ── Email addresses ────────────────────────────────────────
        // WHY this regex: intentionally permissive to cover real-
        // world addresses; the shape ensures `local@domain.tld` at
        // minimum.
        Pattern {
            kind: "email",
            re: compile(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}"),
        },
        // ── SSN (US social security number) ────────────────────────
        Pattern {
            kind: "ssn",
            re: compile(r"\b\d{3}-\d{2}-\d{4}\b"),
        },
        // ── Credit card numbers ───────────────────────────────────
        // WHY: Luhn is enforced at the redaction step in
        // `redact_credit_cards` to avoid false positives on long
        // numeric sequences.
        Pattern {
            kind: "credit_card",
            re: compile(r"\b(?:\d[ \-]?){12,18}\d\b"),
        },
        // ── Phone numbers (E.164 and common US formats) ───────────
        Pattern {
            kind: "phone",
            re: compile(
                r"(?x)
                (?:\+?\d{1,3}[ \-.])?          # country code
                (?:\(\d{3}\)|\d{3})[ \-.]      # area code
                \d{3}[ \-.]\d{4}               # subscriber
                \b
                |
                \+\d{10,15}\b                  # bare E.164
                ",
            ),
        },
        // ── Secret-shaped env-var assignments ─────────────────────
        // WHY: captures `API_KEY=...`, `SECRET=...`, `TOKEN=...`,
        // `PASSWORD=...`, `ANTHROPIC_API_KEY=...` etc. on a line.
        Pattern {
            kind: "secret_assignment",
            re: compile(
                r#"(?im)\b[A-Z][A-Z0-9_]*(?:API_?KEY|SECRET|TOKEN|PASSWORD|PASSWD|PRIVATE_?KEY|ACCESS_?KEY)[A-Z0-9_]*\s*[:=]\s*['"]?[^\s'"]+['"]?"#,
            ),
        },
        // ── Generic API-key-looking values (fallback) ─────────────
        // WHY: narrow — requires the literal token "api_key" or
        // "apikey" before an assignment. Keeps false positives low.
        Pattern {
            kind: "api_key",
            re: compile(r#"(?i)\bapi[_\-]?key\b\s*[:=]\s*['"]?[A-Za-z0-9_\-]{16,}['"]?"#),
        },
    ]
});

/// Luhn check for credit-card-shaped digit sequences.
fn luhn_valid(digits: &str) -> bool {
    let mut sum: u32 = 0;
    let mut alt = false;
    for ch in digits.chars().rev() {
        let Some(d) = ch.to_digit(10) else { continue };
        let v = if alt {
            let doubled = d * 2;
            if doubled > 9 { doubled - 9 } else { doubled }
        } else {
            d
        };
        sum += v;
        alt = !alt;
    }
    sum.is_multiple_of(10)
}

/// Redact credit card numbers, but only when Luhn-valid.
fn redact_credit_cards(input: &str) -> String {
    #[expect(
        clippy::expect_used,
        reason = "credit_card pattern is always present in PATTERNS (construction invariant)"
    )]
    let re = &PATTERNS
        .iter()
        .find(|p| p.kind == "credit_card")
        .expect("credit_card pattern is present in PATTERNS")
        .re;
    let m = marker("credit_card");
    re.replace_all(input, |caps: &regex::Captures<'_>| {
        let raw = &caps[0];
        let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
        // Credit cards are 13–19 digits.
        if (13..=19).contains(&digits.len()) && luhn_valid(&digits) {
            m.clone()
        } else {
            raw.to_owned()
        }
    })
    .into_owned()
}

/// Redact PII and secret patterns from `input`.
///
/// Returns a tuple of (`redacted_text`, `did_redact`). `did_redact` is
/// `true` iff any pattern matched and produced a replacement. The
/// boolean lets callers mark the record's `pii_redacted` flag even
/// when no pattern fires (policy-applied, no matches) by OR-ing with
/// the configured policy state.
///
/// WHY return `(String, bool)`: callers need both the scrubbed text
/// and whether anything was scrubbed. Returning only `String` would
/// force a separate `contains(MARKER_PREFIX)` pass.
#[must_use]
pub fn redact(input: &str) -> (String, bool) {
    let mut current = input.to_owned();
    let mut changed = false;

    for pat in PATTERNS.iter() {
        if pat.kind == "credit_card" {
            // WHY: credit cards go through the Luhn-gated redactor to
            // avoid clobbering long numeric strings that happen to be
            // 13+ digits (e.g. session ids, hashes).
            let next = redact_credit_cards(&current);
            if next != current {
                changed = true;
                current = next;
            }
            continue;
        }
        if pat.re.is_match(&current) {
            changed = true;
            current = pat.re.replace_all(&current, marker(pat.kind)).into_owned();
        }
    }

    (current, changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn redacts_email() {
        let (out, changed) = redact("contact me at alice@example.com please");
        assert!(changed);
        assert!(out.contains("[REDACTED:email]"));
        assert!(!out.contains("alice@example.com"));
    }

    #[test]
    fn redacts_phone_us_dashed() {
        let (out, changed) = redact("call 512-555-0199 anytime");
        assert!(changed);
        assert!(out.contains("[REDACTED:phone]"));
    }

    #[test]
    fn redacts_phone_e164() {
        let (out, changed) = redact("my number is +15125550199");
        assert!(changed);
        assert!(out.contains("[REDACTED:phone]"));
    }

    #[test]
    fn redacts_ssn() {
        let (out, changed) = redact("SSN: 123-45-6789");
        assert!(changed);
        assert!(out.contains("[REDACTED:ssn]"));
    }

    #[test]
    fn redacts_credit_card_luhn_valid() {
        // Visa test number from https://docs.stripe.com/testing
        let (out, changed) = redact("card: 4242 4242 4242 4242 exp 01/30");
        assert!(changed, "Luhn-valid CC should be redacted: {out}");
        assert!(out.contains("[REDACTED:credit_card]"));
    }

    #[test]
    fn preserves_non_card_digit_sequences() {
        // Long numeric id that fails Luhn — must not be redacted as CC.
        let (out, _) = redact("batch id 1111111111111111 processed");
        assert!(
            !out.contains("[REDACTED:credit_card]"),
            "non-Luhn digit sequence wrongly redacted: {out}"
        );
    }

    #[test]
    fn redacts_anthropic_api_key() {
        let (out, changed) =
            redact("use sk-ant-api03-abcdefghij-klmnopqrstuvwxyz0123456789 as your key");
        assert!(changed);
        assert!(out.contains("[REDACTED:anthropic_api_key]"));
        assert!(!out.contains("sk-ant-"));
    }

    #[test]
    fn redacts_openai_api_key() {
        let (out, changed) = redact("OPENAI=sk-proj-abcdefghij1234567890ABCDEF0123456789");
        assert!(changed);
        assert!(out.contains("[REDACTED:"));
        assert!(!out.contains("sk-proj-abcdefghij1234567890ABCDEF0123456789"));
    }

    #[test]
    fn redacts_google_api_key() {
        let (out, changed) = redact("key=AIzaSyAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        assert!(changed);
        assert!(out.contains("[REDACTED:google_api_key]"));
    }

    #[test]
    fn redacts_github_token() {
        let (out, changed) = redact("export TOKEN=ghp_1234567890abcdefghij1234567890ABCD");
        assert!(changed);
        assert!(out.contains("[REDACTED:"));
        assert!(!out.contains("ghp_1234567890abcdefghij1234567890ABCD"));
    }

    #[test]
    fn redacts_aws_access_key_id() {
        let (out, changed) = redact("AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE");
        assert!(changed);
        assert!(out.contains("[REDACTED:"));
    }

    #[test]
    fn redacts_aws_secret_access_key() {
        let (out, changed) =
            redact("aws_secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
        assert!(changed);
        assert!(out.contains("[REDACTED:"));
    }

    #[test]
    fn redacts_jwt() {
        let (out, changed) = redact(
            "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
        );
        assert!(changed);
        assert!(out.contains("[REDACTED:jwt]"));
    }

    #[test]
    fn redacts_env_assignment_password() {
        let (out, changed) = redact("DATABASE_PASSWORD=hunter2");
        assert!(changed);
        assert!(out.contains("[REDACTED:secret_assignment]"));
        assert!(!out.contains("hunter2"));
    }

    #[test]
    fn redacts_generic_api_key_assignment() {
        let (out, changed) = redact("api_key: \"abcdefghij1234567890\"");
        assert!(changed);
        assert!(out.contains("[REDACTED:api_key]"));
    }

    #[test]
    fn no_redaction_when_clean() {
        let (out, changed) = redact("The quick brown fox jumps over the lazy dog.");
        assert!(!changed);
        assert_eq!(out, "The quick brown fox jumps over the lazy dog.");
    }

    #[test]
    fn multiple_patterns_in_one_string() {
        let input =
            "alice@example.com called 512-555-0199 about sk-ant-api03-abcdefghijklmnopqrstuvwxyz";
        let (out, changed) = redact(input);
        assert!(changed);
        assert!(out.contains("[REDACTED:email]"));
        assert!(out.contains("[REDACTED:phone]"));
        assert!(out.contains("[REDACTED:anthropic_api_key]"));
        assert!(!out.contains("alice@example.com"));
    }

    #[test]
    fn redaction_is_idempotent() {
        // Applying twice produces the same output as once.
        let (first, _) = redact("alice@example.com is offline");
        let (second, changed) = redact(&first);
        assert!(!changed, "second pass should be a no-op");
        assert_eq!(first, second);
    }

    // ── Property tests ──────────────────────────────────────────────

    proptest! {
        #[test]
        fn email_bypass_none(
            local in "[a-zA-Z0-9._%+\\-]{1,20}",
            domain in "[a-zA-Z0-9.\\-]{1,20}",
            tld in "[a-zA-Z]{2,6}"
        ) {
            let email = format!("{local}@{domain}.{tld}");
            let text = format!("note: {email} here");
            let (out, changed) = redact(&text);
            prop_assert!(changed, "did not redact email: {email}");
            prop_assert!(out.contains("[REDACTED:email]"));
            prop_assert!(!out.contains(&email), "email leaked through: {out}");
        }

        #[test]
        fn ssn_bypass_none(
            a in 100u32..=999, b in 10u32..=99, c in 1000u32..=9999
        ) {
            let ssn = format!("{a:03}-{b:02}-{c:04}");
            let text = format!("Subject SSN: {ssn}.");
            let (out, changed) = redact(&text);
            prop_assert!(changed, "did not redact SSN: {ssn}");
            prop_assert!(out.contains("[REDACTED:ssn]"));
            prop_assert!(!out.contains(&ssn));
        }

        #[test]
        fn anthropic_key_bypass_none(tail in "[A-Za-z0-9_\\-]{30,60}") {
            let key = format!("sk-ant-{tail}");
            let text = format!("leak: {key} done");
            let (out, changed) = redact(&text);
            prop_assert!(changed, "did not redact anthropic key");
            prop_assert!(!out.contains(&key), "anthropic key leaked: {out}");
        }

        #[test]
        fn clean_text_untouched(
            words in proptest::collection::vec("[a-z]{3,8}", 1..20)
        ) {
            // Pure ASCII lowercase words with no `@`, digits or key
            // shapes — should pass through unchanged.
            let text = words.join(" ");
            let (out, changed) = redact(&text);
            prop_assert!(!changed, "clean text redacted: {text} -> {out}");
            prop_assert_eq!(out, text);
        }
    }
}
