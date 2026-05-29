use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::resolved::ResolvedTheme;

/// Identifier for the `THEME/raw-color-literal` rule. The QA gate ([B-008])
/// will register this rule against the basanos engine using this string.
pub const RAW_COLOR_LITERAL_RULE_ID: &str = "THEME/raw-color-literal";

/// Identifier for the `THEME/raw-font-literal` rule.
pub const RAW_FONT_LITERAL_RULE_ID: &str = "THEME/raw-font-literal";

/// Identifier for the `THEME/unknown-token` rule.
pub const UNKNOWN_TOKEN_RULE_ID: &str = "THEME/unknown-token";

/// A single rule violation. The shape is QA-gate-friendly: `rule_id` is the
/// stable identifier, `pointer` is an RFC-6901 JSON Pointer into the spec
/// payload, `value` is the offending substring, and `message` is the
/// human-actionable diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    /// The stable rule id (e.g. `"THEME/raw-color-literal"`).
    pub rule_id: String,
    /// JSON Pointer into the spec document.
    pub pointer: String,
    /// The offending substring as it appears in the spec.
    pub value: String,
    /// Human-readable diagnostic.
    pub message: String,
}

/// `THEME/raw-color-literal` — reject `#rrggbb`, `#rgb`, `rgb(...)`, `rgba(...)`,
/// and a small set of CSS named colors. The lint is intended for spec fields
/// that should reference theme tokens rather than carry literal values; the
/// agent's only color decision is *which named theme*.
///
/// The rule is stateless; construct once and reuse across scans.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawColorLiteralRule;

impl RawColorLiteralRule {
    /// Scan a single spec field. `pointer` is the RFC-6901 path into the
    /// surrounding spec document — `value` carries no document context, so
    /// the caller supplies it.
    ///
    /// Time: O(n · k) where n is `value.len()` and k is the number of
    /// known dangerous named colors (small, finite). Space: O(violations).
    #[must_use]
    pub fn scan(self, pointer: &str, value: &str) -> Vec<Violation> {
        let mut out = Vec::new();
        for mat in hex_color_regex().find_iter(value) {
            out.push(Violation {
                rule_id: RAW_COLOR_LITERAL_RULE_ID.to_owned(),
                pointer: pointer.to_owned(),
                value: mat.as_str().to_owned(),
                message: format!(
                    "raw color literal {:?}: use a theme tone/role, not a hex value",
                    mat.as_str()
                ),
            });
        }
        for mat in rgb_func_regex().find_iter(value) {
            out.push(Violation {
                rule_id: RAW_COLOR_LITERAL_RULE_ID.to_owned(),
                pointer: pointer.to_owned(),
                value: mat.as_str().to_owned(),
                message: format!(
                    "raw color literal {:?}: use a theme tone/role, not rgb()/rgba()",
                    mat.as_str()
                ),
            });
        }
        let lowered = value.to_ascii_lowercase();
        for named in DANGEROUS_NAMED_COLORS {
            // word-boundary match
            for (idx, _) in lowered.match_indices(named) {
                let before = idx == 0
                    || !value
                        .as_bytes()
                        .get(idx - 1)
                        .copied()
                        .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
                let after_idx = idx + named.len();
                let after = after_idx >= value.len()
                    || !value
                        .as_bytes()
                        .get(after_idx)
                        .copied()
                        .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
                if before && after {
                    out.push(Violation {
                        rule_id: RAW_COLOR_LITERAL_RULE_ID.to_owned(),
                        pointer: pointer.to_owned(),
                        value: named.to_string(),
                        message: format!(
                            "raw color literal {named:?}: use a theme tone/role, not a CSS named color"
                        ),
                    });
                }
            }
        }
        out
    }
}

/// `THEME/raw-font-literal` — reject inline `font-family:` declarations and a
/// small set of well-known typeface names. As with the color rule, the goal
/// is mechanical: fonts come from the theme.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawFontLiteralRule;

impl RawFontLiteralRule {
    /// Scan a single spec field for raw font references.
    ///
    /// Time: O(n · k) where n is `value.len()` and k is the number of
    /// known dangerous typefaces (small, finite). Space: O(violations).
    #[must_use]
    pub fn scan(self, pointer: &str, value: &str) -> Vec<Violation> {
        let mut out = Vec::new();
        if let Some(mat) = font_family_regex().find(value) {
            out.push(Violation {
                rule_id: RAW_FONT_LITERAL_RULE_ID.to_owned(),
                pointer: pointer.to_owned(),
                value: mat.as_str().to_owned(),
                message: format!(
                    "raw font-family declaration {:?}: fonts come from the theme",
                    mat.as_str()
                ),
            });
        }
        let lowered = value.to_ascii_lowercase();
        for typeface in DANGEROUS_TYPEFACES {
            let needle = typeface.to_ascii_lowercase();
            if lowered.contains(&needle) {
                out.push(Violation {
                    rule_id: RAW_FONT_LITERAL_RULE_ID.to_owned(),
                    pointer: pointer.to_owned(),
                    value: (*typeface).to_owned(),
                    message: format!(
                        "raw typeface literal {typeface:?}: reference a theme [type.family] entry"
                    ),
                });
            }
        }
        out
    }
}

/// `THEME/unknown-token` — given a token reference (e.g. `color.tone.positive`
/// or `type.role.title`), reject it if the resolved theme does not define
/// that path.
#[derive(Debug, Clone, Copy)]
pub struct UnknownTokenRule<'theme> {
    /// The theme to check the reference against.
    pub theme: &'theme ResolvedTheme,
}

impl<'theme> UnknownTokenRule<'theme> {
    /// Construct the rule bound to a specific resolved theme.
    #[must_use]
    pub fn new(theme: &'theme ResolvedTheme) -> Self {
        Self { theme }
    }

    /// Check a single token reference. The reference takes the form
    /// `namespace.subspace.name` — e.g. `color.tone.positive`,
    /// `type.role.title`, `type.scale.hero`, `type.family.sans`,
    /// `space.md`, `chart.series.1`. Returns a violation when the reference
    /// names a path the theme does not define.
    #[must_use]
    pub fn check(&self, pointer: &str, token_ref: &str) -> Option<Violation> {
        let parts: Vec<&str> = token_ref.split('.').collect();
        let resolved = match parts.as_slice() {
            ["color", "role", name] => self.theme.role.contains_key(*name),
            ["color", "tone", name] => self.theme.tone.contains_key(*name),
            ["color", "surface", name] => self.theme.surface.contains_key(*name),
            ["type", "family", name] => self.theme.r#type.family.contains_key(*name),
            ["type", "scale", name] => self.theme.r#type.scale.contains_key(*name),
            ["type", "role", name] => self.theme.r#type.role.contains_key(*name),
            ["space", name] => self.theme.space.slots.contains_key(*name),
            ["chart", "series", idx] => idx
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .is_some_and(|i| i < self.theme.chart.series.len()),
            _ => false,
        };
        if resolved {
            None
        } else {
            Some(Violation {
                rule_id: UNKNOWN_TOKEN_RULE_ID.to_owned(),
                pointer: pointer.to_owned(),
                value: token_ref.to_owned(),
                message: format!("theme {} does not define token {token_ref}", self.theme.id),
            })
        }
    }
}

#[expect(
    clippy::expect_used,
    reason = "static regex literal — failure is a programmer bug caught on first call"
)]
fn hex_color_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b")
            .expect("static hex-color regex must compile")
    })
}

#[expect(
    clippy::expect_used,
    reason = "static regex literal — failure is a programmer bug caught on first call"
)]
fn rgb_func_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\brgba?\s*\([^)]*\)").expect("static rgb()/rgba() regex must compile")
    })
}

#[expect(
    clippy::expect_used,
    reason = "static regex literal — failure is a programmer bug caught on first call"
)]
fn font_family_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)font-family\s*:\s*[^;]+").expect("static font-family regex must compile")
    })
}

const DANGEROUS_NAMED_COLORS: &[&str] = &[
    "red", "green", "blue", "yellow", "black", "white", "cyan", "magenta", "orange", "purple",
    "pink", "brown", "gray", "grey",
];

// WHY: we cannot flag every typeface in existence; the rule's purpose is to
// catch the high-frequency literal slips a human spec writer would commit.
const DANGEROUS_TYPEFACES: &[&str] = &[
    "Arial",
    "Helvetica",
    "Times New Roman",
    "Calibri",
    "Verdana",
    "Comic Sans",
    "Courier New",
];

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions; bounds asserted by surrounding test setup"
)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::registry::Registry;

    fn summus() -> ResolvedTheme {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes");
        let registry = Registry::load_dir(&dir).expect("load summus");
        let id = crate::registry::parse_theme_id("summus").expect("parse summus");
        registry.resolve(&id).expect("resolve summus")
    }

    // ── THEME/raw-color-literal ──────────────────────────────────────────────

    #[test]
    fn raw_color_rule_flags_long_hex() {
        let rule = RawColorLiteralRule;
        let v = rule.scan("/slides/0/fields/title_color", "#FF00AA");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule_id, RAW_COLOR_LITERAL_RULE_ID);
        assert_eq!(v[0].pointer, "/slides/0/fields/title_color");
        assert_eq!(v[0].value, "#FF00AA");
    }

    #[test]
    fn raw_color_rule_flags_short_hex() {
        let rule = RawColorLiteralRule;
        let v = rule.scan("/fields/c", "#abc");
        assert_eq!(v.len(), 1, "#abc must flag");
    }

    #[test]
    fn raw_color_rule_flags_rgb_function() {
        let rule = RawColorLiteralRule;
        let v = rule.scan("/fields/c", "rgb(255, 0, 0)");
        assert_eq!(v.len(), 1);
        assert!(v[0].value.starts_with("rgb"));
    }

    #[test]
    fn raw_color_rule_flags_rgba_function() {
        let rule = RawColorLiteralRule;
        let v = rule.scan("/fields/c", "rgba(0,0,0,0.5)");
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn raw_color_rule_flags_named_color() {
        let rule = RawColorLiteralRule;
        let v = rule.scan("/fields/c", "color: red");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].value, "red");
    }

    #[test]
    fn raw_color_rule_does_not_flag_token_reference() {
        let rule = RawColorLiteralRule;
        let v = rule.scan("/fields/c", "color.tone.positive");
        assert!(v.is_empty(), "tone references must not flag");
    }

    #[test]
    fn raw_color_rule_does_not_flag_color_role_substring() {
        let rule = RawColorLiteralRule;
        let v = rule.scan("/fields/c", "honored");
        assert!(
            v.is_empty(),
            "'honored' contains 'red' as substring but is not a named color"
        );
    }

    // ── THEME/raw-font-literal ───────────────────────────────────────────────

    #[test]
    fn raw_font_rule_flags_font_family_declaration() {
        let rule = RawFontLiteralRule;
        let v = rule.scan("/fields/style", "font-family: Helvetica");
        assert!(!v.is_empty());
        assert_eq!(v[0].rule_id, RAW_FONT_LITERAL_RULE_ID);
    }

    #[test]
    fn raw_font_rule_flags_well_known_typeface() {
        let rule = RawFontLiteralRule;
        let v = rule.scan("/fields/typeface", "Arial");
        assert!(!v.is_empty(), "Arial literal must flag");
    }

    #[test]
    fn raw_font_rule_does_not_flag_token_reference() {
        let rule = RawFontLiteralRule;
        let v = rule.scan("/fields/typeface", "type.family.sans");
        assert!(v.is_empty(), "type.family.sans must not flag");
    }

    // ── THEME/unknown-token ──────────────────────────────────────────────────

    #[test]
    fn unknown_token_rule_accepts_known_role() {
        let theme = summus();
        let rule = UnknownTokenRule::new(&theme);
        assert!(rule.check("/p", "color.role.navy").is_none());
    }

    #[test]
    fn unknown_token_rule_accepts_known_tone() {
        let theme = summus();
        let rule = UnknownTokenRule::new(&theme);
        assert!(rule.check("/p", "color.tone.positive").is_none());
    }

    #[test]
    fn unknown_token_rule_accepts_known_scale() {
        let theme = summus();
        let rule = UnknownTokenRule::new(&theme);
        assert!(rule.check("/p", "type.scale.title").is_none());
    }

    #[test]
    fn unknown_token_rule_rejects_unknown_role() {
        let theme = summus();
        let rule = UnknownTokenRule::new(&theme);
        let v = rule.check("/p", "color.role.fuchsia");
        assert!(v.is_some());
        let vio = v.expect("violation");
        assert_eq!(vio.rule_id, UNKNOWN_TOKEN_RULE_ID);
        assert!(vio.message.contains("summus"));
    }

    #[test]
    fn unknown_token_rule_rejects_unstructured_reference() {
        let theme = summus();
        let rule = UnknownTokenRule::new(&theme);
        assert!(rule.check("/p", "navy").is_some(), "bare names must reject");
    }

    #[test]
    fn unknown_token_rule_accepts_chart_series_index() {
        let theme = summus();
        let rule = UnknownTokenRule::new(&theme);
        assert!(rule.check("/p", "chart.series.1").is_none());
    }

    #[test]
    fn unknown_token_rule_rejects_oob_chart_series_index() {
        let theme = summus();
        let rule = UnknownTokenRule::new(&theme);
        let series_len = theme.chart.series.len();
        let oob = series_len + 5;
        assert!(rule.check("/p", &format!("chart.series.{oob}")).is_some());
    }
}
