//! Environment variable interpolation for TOML configuration strings.
//!
//! Supports two substitution forms:
//!
//! - `${VAR:-default}` -- substitutes `default` when `VAR` is unset
//! - `${VAR:?error message}` -- aborts startup with `error message` when `VAR` is unset
//!
//! Plain `${VAR}` without an operator substitutes the variable value or an empty
//! string if unset.
//!
//! SECURITY(#5249): substitution is TOML-syntax-aware, not a blind pre-parse
//! string replace. The scanner tracks whether it is inside a bare (unquoted)
//! value, a quoted basic string (`"..."`/`"""..."""`), a literal string
//! (`'...'`/`'''...'''`), or a `#` comment, and treats each differently:
//!
//! - **Comments**: `${...}` is left untouched -- interpolation never alters
//!   what the TOML parser treats as inert text.
//! - **Literal strings**: `${...}` is left untouched. TOML literal strings
//!   are verbatim by design (no escape mechanism at all), so there is no safe
//!   way to substitute an arbitrary value into one without risking an
//!   unescapable embedded `'`; leaving them alone matches the syntax's own
//!   contract.
//! - **Basic strings** (single- or multi-line): the substituted value is
//!   escaped for TOML basic-string content (`\`, `"`, and control characters)
//!   before insertion, so it can never prematurely close the string or
//!   inject sibling keys/tables.
//! - **Bare positions** (unquoted values -- the numeric/boolean substitution
//!   pattern this module has always supported, e.g. `port = ${PORT:-8080}`):
//!   substituted raw, as before, but rejected if the result contains a
//!   newline. An unquoted TOML value can never legitimately span a line, so
//!   an embedded newline is the only way a bare substitution could inject
//!   new document structure (a fabricated `[section]` on the following
//!   "line"); every other character just yields a TOML parse error, which is
//!   a safe (fail-closed) outcome, not a silent one.
//!
//! KNOWN LIMITATION: multi-line string close detection triggers on the first
//! run of 3 consecutive quote characters, which does not implement the full
//! TOML corner case where a multi-line string legitimately contains 1-2
//! literal trailing quotes immediately before its real closing delimiter. A
//! config exercising that corner (already rare without interpolation) fails
//! to parse rather than being misinterpreted -- the safe direction.

use std::env;
use std::fmt::Write as _;

use crate::error::{
    EnvVarRequiredSnafu, EnvVarUnsafeSubstitutionSnafu, EnvVarUnterminatedSnafu, Result,
};

/// Which TOML lexical context the scanner is currently inside.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScanContext {
    /// Outside any string or comment: table headers, keys, unquoted values,
    /// punctuation.
    Bare,
    /// Inside a `"..."` basic string.
    BasicString,
    /// Inside a `"""..."""` multi-line basic string.
    MultilineBasicString,
    /// Inside a `'...'` literal string.
    LiteralString,
    /// Inside a `'''...'''` multi-line literal string.
    MultilineLiteralString,
    /// Inside a `#` comment, through end of line.
    Comment,
}

/// Interpolate `${VAR:-default}` and `${VAR:?error}` expressions in a TOML
/// document, honoring string/comment syntax so a substituted value can never
/// alter document structure. See the module docs for the full contract.
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
/// Returns [`crate::error::Error::EnvVarUnsafeSubstitution`] when a bare
/// (unquoted) substitution's value contains a newline (#5249).
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let out = taxis::interpolate::interpolate_env_vars(
///     "[gateway]\nport = ${_TAXIS_UNSET_EXAMPLE:-18789}"
/// )?;
/// assert_eq!(out, "[gateway]\nport = 18789");
/// # Ok(())
/// # }
/// ```
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn interpolate_env_vars(content: &str) -> Result<String> {
    let chars: Vec<char> = content.chars().collect();
    Scanner::new(&chars).run()
}

/// Drives the context-sensitive scan. Split from [`interpolate_env_vars`]
/// (and further into one `step_*` method per [`ScanContext`]) purely to stay
/// under the workspace's function-length lint; the state machine is a single
/// unit of logic.
struct Scanner<'a> {
    chars: &'a [char],
    out: String,
    i: usize,
    ctx: ScanContext,
}

impl<'a> Scanner<'a> {
    fn new(chars: &'a [char]) -> Self {
        Self {
            chars,
            out: String::with_capacity(chars.len()),
            i: 0,
            ctx: ScanContext::Bare,
        }
    }

    fn run(mut self) -> Result<String> {
        while let Some(&c) = self.chars.get(self.i) {
            match self.ctx {
                ScanContext::Comment => self.step_comment(c),
                ScanContext::LiteralString => self.step_literal_string(c),
                ScanContext::MultilineLiteralString => self.step_multiline_literal_string(c),
                ScanContext::BasicString | ScanContext::MultilineBasicString => {
                    self.step_basic_string(c)?;
                }
                ScanContext::Bare => self.step_bare(c)?,
            }
        }
        Ok(self.out)
    }

    fn step_comment(&mut self, c: char) {
        self.out.push(c);
        self.i += 1;
        if c == '\n' {
            self.ctx = ScanContext::Bare;
        }
    }

    fn step_literal_string(&mut self, c: char) {
        // WHY(#5249): TOML literal strings are verbatim by design (no
        // escapes) -- copy through untouched, including any ${...} spans,
        // rather than risk a substitution with no safe way to escape a
        // literal `'` in the result.
        self.out.push(c);
        self.i += 1;
        if c == '\'' {
            self.ctx = ScanContext::Bare;
        }
    }

    fn step_multiline_literal_string(&mut self, c: char) {
        if c == '\'' && matches_ahead(self.chars, self.i, "''") {
            self.out.push_str("'''");
            self.i += 3;
            self.ctx = ScanContext::Bare;
        } else {
            self.out.push(c);
            self.i += 1;
        }
    }

    fn step_basic_string(&mut self, c: char) -> Result<()> {
        let multiline = self.ctx == ScanContext::MultilineBasicString;
        if c == '\\' {
            // WHY: copy the backslash and the char it escapes together and
            // verbatim, so an escaped quote (`\"`) never closes the string
            // early.
            self.out.push(c);
            self.i += 1;
            if let Some(&next) = self.chars.get(self.i) {
                self.out.push(next);
                self.i += 1;
            }
        } else if c == '"' {
            if multiline && matches_ahead(self.chars, self.i, "\"\"") {
                self.out.push_str("\"\"\"");
                self.i += 3;
                self.ctx = ScanContext::Bare;
            } else if multiline {
                self.out.push(c);
                self.i += 1;
            } else {
                self.out.push(c);
                self.i += 1;
                self.ctx = ScanContext::Bare;
            }
        } else if c == '$' && self.chars.get(self.i + 1) == Some(&'{') {
            let (expr, consumed) = extract_expr(self.chars, self.i)?;
            let substituted = resolve_expr(&expr)?;
            self.out.push_str(&escape_for_basic_string(&substituted));
            self.i += consumed;
        } else {
            self.out.push(c);
            self.i += 1;
        }
        Ok(())
    }

    fn step_bare(&mut self, c: char) -> Result<()> {
        if c == '#' {
            self.out.push(c);
            self.i += 1;
            self.ctx = ScanContext::Comment;
        } else if c == '\'' {
            if matches_ahead(self.chars, self.i, "''") {
                self.out.push_str("'''");
                self.i += 3;
                self.ctx = ScanContext::MultilineLiteralString;
            } else {
                self.out.push(c);
                self.i += 1;
                self.ctx = ScanContext::LiteralString;
            }
        } else if c == '"' {
            if matches_ahead(self.chars, self.i, "\"\"") {
                self.out.push_str("\"\"\"");
                self.i += 3;
                self.ctx = ScanContext::MultilineBasicString;
            } else {
                self.out.push(c);
                self.i += 1;
                self.ctx = ScanContext::BasicString;
            }
        } else if c == '$' && self.chars.get(self.i + 1) == Some(&'{') {
            let (expr, consumed) = extract_expr(self.chars, self.i)?;
            let substituted = resolve_expr(&expr)?;
            // SECURITY(#5249): a bare/unquoted TOML value can never
            // legitimately span a line. Without this check, an env value
            // containing a newline could complete the current line as a
            // valid value and then open new keys/tables on the "next line"
            // -- turning one substitution into multiple injected statements.
            if substituted.contains('\n') || substituted.contains('\r') {
                return EnvVarUnsafeSubstitutionSnafu { expr }.fail();
            }
            self.out.push_str(&substituted);
            self.i += consumed;
        } else {
            self.out.push(c);
            self.i += 1;
        }
        Ok(())
    }
}

/// Whether the `pat.len()` characters immediately after position `i` (i.e.
/// `chars[i+1..=i+pat.len()]`) match `pat`, without slicing `chars`.
fn matches_ahead(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(i + 1 + offset) == Some(&expected))
}

/// Extract the raw expression body of a `${...}` reference starting at `i`
/// (which must point at `$`), returning `(expr, consumed)` where `consumed`
/// is the number of source characters spanned, including both delimiters.
///
/// # Errors
///
/// Returns [`crate::error::Error::EnvVarUnterminated`] if no closing `}` is
/// found before the end of input.
fn extract_expr(chars: &[char], i: usize) -> Result<(String, usize)> {
    let start = i + 2; // skip '${'
    let mut j = start;
    loop {
        match chars.get(j) {
            Some('}') => break,
            Some(_) => j += 1,
            None => {
                let excerpt: String = chars
                    .get(start..)
                    .unwrap_or_default()
                    .iter()
                    .take(30)
                    .collect();
                return EnvVarUnterminatedSnafu { excerpt }.fail();
            }
        }
    }
    let expr: String = chars.get(start..j).unwrap_or_default().iter().collect();
    Ok((expr, j + 1 - i))
}

/// Escape `value` so it can be spliced into an already-open TOML basic
/// string (single- or multi-line) without prematurely closing it or
/// otherwise corrupting the string's content (#5249).
fn escape_for_basic_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                // kanon:ignore RUST/no-result-unwrap-or-default — write! to a String never fails
                let _ = write!(escaped, "\\u{:04x}", u32::from(c));
            }
            c => escaped.push(c),
        }
    }
    escaped
}

/// Resolve the expression body between `${` and `}`.
fn resolve_expr(expr: &str) -> Result<String> {
    if let Some(sep) = expr.find(":-") {
        #[expect(
            clippy::string_slice,
            reason = "sep is a valid UTF-8 boundary returned by str::find on ASCII ':-'"
        )]
        // kanon:ignore RUST/indexing-slicing — sep from str::find on expr, always a valid UTF-8 boundary
        let var = &expr[..sep];
        #[expect(
            clippy::string_slice,
            reason = "sep + 2 skips ASCII ':-', valid UTF-8 boundary"
        )]
        // kanon:ignore RUST/indexing-slicing — sep + 2 skips ASCII ':-', always a valid UTF-8 boundary
        let default = &expr[sep + 2..];
        Ok(env::var(var).unwrap_or_else(|_| default.to_owned()))
    } else if let Some(sep) = expr.find(":?") {
        #[expect(
            clippy::string_slice,
            reason = "sep is a valid UTF-8 boundary returned by str::find on ASCII ':?'"
        )]
        // kanon:ignore RUST/indexing-slicing — sep from str::find on expr, always a valid UTF-8 boundary
        let var = &expr[..sep];
        #[expect(
            clippy::string_slice,
            reason = "sep + 2 skips ASCII ':?', valid UTF-8 boundary"
        )]
        // kanon:ignore RUST/indexing-slicing — sep + 2 skips ASCII ':?', always a valid UTF-8 boundary
        let message = &expr[sep + 2..];
        env::var(var).map_err(|_env_err| {
            EnvVarRequiredSnafu {
                var: var.to_owned(),
                message: message.to_owned(),
            }
            .build()
        })
    } else {
        // kanon:ignore RUST/no-result-unwrap-or-default — plain ${VAR} spec defines empty string as the fallback when the variable is unset
        Ok(env::var(expr).unwrap_or_default())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: toml::Value string-key indexing panics only if key is absent"
)]
mod tests {
    use super::*;
    use crate::test_support::EnvJail;

    // NOTE: All tests that touch env vars run inside EnvJail to isolate them
    // from the process environment and each other (serialised via a global mutex).

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
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_TEST_PORT", "9999");
        let out = interpolate_env_vars("port = ${_TAX_INTERP_TEST_PORT}").unwrap();
        assert_eq!(out, "port = 9999", "set env var should be substituted");
    }

    #[test]
    fn plain_var_substitutes_empty_when_unset() {
        let mut jail = EnvJail::new();
        jail.remove_env("_TAX_INTERP_UNSET_XYZ");
        let out = interpolate_env_vars("val = ${_TAX_INTERP_UNSET_XYZ}").unwrap();
        assert_eq!(out, "val = ", "unset var should substitute empty string");
    }

    #[test]
    fn default_used_when_var_unset() {
        let mut jail = EnvJail::new();
        jail.remove_env("_TAX_INTERP_MISSING");
        let out = interpolate_env_vars("port = ${_TAX_INTERP_MISSING:-18789}").unwrap();
        assert_eq!(
            out, "port = 18789",
            "default value should be used when var is unset"
        );
    }

    #[test]
    fn default_not_used_when_var_set() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_PRESENT", "42");
        let out = interpolate_env_vars("port = ${_TAX_INTERP_PRESENT:-99}").unwrap();
        assert_eq!(out, "port = 42", "set var should override default value");
    }

    #[test]
    fn required_var_aborts_when_unset() {
        let mut jail = EnvJail::new();
        jail.remove_env("_TAX_INTERP_REQUIRED");
        let result = interpolate_env_vars("key = ${_TAX_INTERP_REQUIRED:?API key must be set}");
        assert!(result.is_err(), "expected an error for unset required var");
    }

    #[test]
    fn required_var_error_contains_var_name_and_message() {
        let mut jail = EnvJail::new();
        jail.remove_env("_TAX_INTERP_REQUIRED2");
        let result = interpolate_env_vars("key = ${_TAX_INTERP_REQUIRED2:?API key must be set}");
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
    }

    #[test]
    fn required_var_succeeds_when_set() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_PRESENT2", "secret");
        let out = interpolate_env_vars("key = ${_TAX_INTERP_PRESENT2:?must be set}").unwrap();
        assert_eq!(
            out, "key = secret",
            "required var should substitute its value when set"
        );
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
    fn unterminated_ref_inside_basic_string_returns_error() {
        let err = interpolate_env_vars("key = \"${UNCLOSED").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unterminated"),
            "error should say unterminated: {msg}"
        );
    }

    #[test]
    fn multiple_substitutions_in_one_string() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_HOST", "localhost");
        jail.set_env("_TAX_INTERP_PORT2", "8080");
        let out =
            interpolate_env_vars("bind = \"${_TAX_INTERP_HOST}:${_TAX_INTERP_PORT2}\"").unwrap();
        assert_eq!(
            out, "bind = \"localhost:8080\"",
            "multiple substitutions should all resolve"
        );
    }

    #[test]
    fn default_value_containing_colon_is_preserved() {
        let mut jail = EnvJail::new();
        jail.remove_env("_TAX_INTERP_URL");
        // NOTE: the first `:-` is the operator; the rest is the default value.
        let out = interpolate_env_vars("url = ${_TAX_INTERP_URL:-http://localhost:8080}").unwrap();
        assert_eq!(
            out, "url = http://localhost:8080",
            "colon in default value should be preserved"
        );
    }

    // ── #5249: TOML-aware interpolation ──────────────────────────────────

    #[test]
    fn basic_string_substitution_escapes_embedded_quote() {
        // SECURITY(#5249) regression: previously this was a raw pre-parse
        // replace, so an env value containing `"` closed the TOML string
        // early and everything after it became live TOML syntax -- e.g. a
        // forged `[gateway.auth]` table with `mode = "none"`. The escaped
        // value must stay literal string *content*.
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_INJECT", "x\"\n[gateway.auth]\nmode=\"none");
        let toml_str = "[gateway.auth]\nsigningKey = \"${_TAX_INTERP_INJECT}\"\n";
        let out = interpolate_env_vars(toml_str).unwrap();

        let value: toml::Value = toml::from_str(&out).expect("escaped output must still parse");
        assert_eq!(
            value["gateway"]["auth"]["signingKey"].as_str(),
            Some("x\"\n[gateway.auth]\nmode=\"none"),
            "the injected content must land as the literal string value, not as TOML structure"
        );
        assert_eq!(
            value["gateway"]["auth"].get("mode"),
            None,
            "no sibling `mode` key should have been injected"
        );
    }

    #[test]
    fn basic_string_substitution_escapes_backslash_and_control_chars() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_BACKSLASH", "C:\\path\twith\ttabs");
        let toml_str = "path = \"${_TAX_INTERP_BACKSLASH}\"\n";
        let out = interpolate_env_vars(toml_str).unwrap();

        let value: toml::Value = toml::from_str(&out).expect("escaped output must still parse");
        assert_eq!(
            value["path"].as_str(),
            Some("C:\\path\twith\ttabs"),
            "backslashes and tabs should round-trip through escaping"
        );
    }

    #[test]
    fn bare_context_rejects_newline_in_substituted_value() {
        // SECURITY(#5249) regression: the unquoted-value injection path --
        // `port = ${VAR:-18789}` where VAR's value contains a newline could
        // terminate the current line as a valid value and then open a new
        // `[section]` on the "next line".
        let mut jail = EnvJail::new();
        jail.set_env(
            "_TAX_INTERP_NEWLINE",
            "18789\n[gateway.auth]\nmode = \"none\"",
        );
        let result = interpolate_env_vars("port = ${_TAX_INTERP_NEWLINE}");
        let err = result.expect_err("newline in a bare substitution must be rejected");
        assert!(
            matches!(err, crate::error::Error::EnvVarUnsafeSubstitution { .. }),
            "expected EnvVarUnsafeSubstitution, got {err:?}"
        );
    }

    #[test]
    fn comment_context_does_not_interpolate() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_COMMENTED", "should-not-appear");
        let out = interpolate_env_vars("# see ${_TAX_INTERP_COMMENTED} for details\nport = 8080")
            .unwrap();
        assert!(
            out.contains("${_TAX_INTERP_COMMENTED}"),
            "a placeholder inside a comment must be left untouched: {out}"
        );
    }

    #[test]
    fn literal_string_does_not_interpolate() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_LITERAL", "should-not-substitute");
        let out = interpolate_env_vars("pattern = 'literal ${_TAX_INTERP_LITERAL} text'").unwrap();
        assert_eq!(
            out, "pattern = 'literal ${_TAX_INTERP_LITERAL} text'",
            "TOML literal strings must not be interpolated at all"
        );
    }

    #[test]
    fn multiline_literal_string_does_not_interpolate() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_MLLITERAL", "should-not-substitute");
        let out =
            interpolate_env_vars("pattern = '''line one\n${_TAX_INTERP_MLLITERAL}\nline two'''")
                .unwrap();
        assert!(
            out.contains("${_TAX_INTERP_MLLITERAL}"),
            "multi-line literal strings must not be interpolated: {out}"
        );
    }

    #[test]
    fn multiline_basic_string_interpolates_and_escapes() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_ML", "value with \"quotes\"");
        let toml_str = "note = \"\"\"line one\n${_TAX_INTERP_ML}\nline two\"\"\"\n";
        let out = interpolate_env_vars(toml_str).unwrap();

        let value: toml::Value = toml::from_str(&out).expect("escaped output must still parse");
        assert_eq!(
            value["note"].as_str(),
            Some("line one\nvalue with \"quotes\"\nline two"),
            "multi-line basic string substitution should escape embedded quotes"
        );
    }

    #[test]
    fn escaped_quote_inside_basic_string_does_not_close_it_early() {
        // A literal `\"` in the SOURCE TOML (not from substitution) must not
        // be mistaken for the closing delimiter.
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_AFTER_ESCAPE", "tail");
        let out =
            interpolate_env_vars("note = \"a \\\"quoted\\\" word ${_TAX_INTERP_AFTER_ESCAPE}\"")
                .unwrap();
        assert_eq!(out, "note = \"a \\\"quoted\\\" word tail\"");
    }

    #[test]
    fn hash_inside_basic_string_is_not_a_comment() {
        let mut jail = EnvJail::new();
        jail.set_env("_TAX_INTERP_AFTER_HASH", "value");
        let out =
            interpolate_env_vars("note = \"#not-a-comment ${_TAX_INTERP_AFTER_HASH}\"").unwrap();
        assert_eq!(out, "note = \"#not-a-comment value\"");
    }
}
