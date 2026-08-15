use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::error::{InvalidTokenKeySnafu, ThemeError};
use crate::id::ThemeId;

/// A theme as authored: the TOML on disk parses into this shape. Token
/// references in `[color.tone]` still point at *role names* — resolution into
/// concrete hex values is the job of [`ResolvedTheme`](crate::ResolvedTheme).
///
/// Map ordering uses [`IndexMap`] so the surface order in the TOML is
/// preserved through CSS / OOXML / doc-vars emission. The sinks rely on this
/// for byte-stable output.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Theme {
    /// Theme-level metadata: id, optional human title and description.
    pub meta: Meta,
    /// `[color.*]` token namespace.
    pub color: ColorTokens,
    /// `[type.*]` token namespace.
    pub r#type: TypeTokens,
    /// `[space]` table.
    #[serde(default)]
    pub space: SpaceTokens,
    /// `[grid]` table.
    #[serde(default)]
    pub grid: GridTokens,
    /// `[table]` table.
    #[serde(default)]
    pub table: TableTokens,
    /// `[chart]` table.
    #[serde(default)]
    pub chart: ChartTokens,
}

impl Theme {
    /// Validate every token-namespace key against the identifier-safe
    /// charset every sink (`css`, `typst`, `latex`) relies on when it
    /// interpolates a key into generated markup: lowercase ASCII letters,
    /// digits, `_`, `-`; must start with a letter. Mirrors the
    /// `validate_fs_safe` pattern in `poiesis-core/src/ids.rs` locally so
    /// this crate does not need a `poiesis-core` dependency for one
    /// predicate.
    ///
    /// Covers every namespace whose keys a sink emits as an identifier
    /// fragment: `[color.role]`, `[color.tone]`, `[color.surface]`,
    /// `[type.family]`, `[type.scale]`, `[type.role]`, `[space]`. Values
    /// (hex literals, tone/surface role references, family stacks) are
    /// validated separately — [`HexColor::parse`] for the former,
    /// resolution failure ([`crate::resolved::ResolvedTheme::from_theme`])
    /// for the latter.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeError::InvalidTokenKey`] naming the first offending
    /// namespace, key, and reason.
    pub fn validate_token_keys(&self) -> Result<(), ThemeError> {
        let id = self.meta.id.as_str();
        for key in self.color.role.keys() {
            validate_token_key(id, "color.role", key)?;
        }
        for key in self.color.tone.keys() {
            validate_token_key(id, "color.tone", key)?;
        }
        for key in self.color.surface.keys() {
            validate_token_key(id, "color.surface", key)?;
        }
        for key in self.r#type.family.keys() {
            validate_token_key(id, "type.family", key)?;
        }
        for key in self.r#type.scale.keys() {
            validate_token_key(id, "type.scale", key)?;
        }
        for key in self.r#type.role.keys() {
            validate_token_key(id, "type.role", key)?;
        }
        for key in self.space.slots.keys() {
            validate_token_key(id, "space", key)?;
        }
        Ok(())
    }
}

/// Validate a single token key against the identifier-safe charset:
/// lowercase ASCII `a-z`, digits `0-9`, `_`, `-`; first character must be a
/// lowercase letter.
fn validate_token_key(
    theme_id: &str,
    namespace: &'static str,
    key: &str,
) -> Result<(), ThemeError> {
    let mut chars = key.chars();
    match chars.next() {
        None => {
            return InvalidTokenKeySnafu {
                theme_id: theme_id.to_owned(),
                namespace,
                key: key.to_owned(),
                reason: "key is empty".to_owned(),
            }
            .fail();
        }
        Some(first) if !first.is_ascii_lowercase() => {
            return InvalidTokenKeySnafu {
                theme_id: theme_id.to_owned(),
                namespace,
                key: key.to_owned(),
                reason: format!("first character {first:?} must be a lowercase ASCII letter"),
            }
            .fail();
        }
        _ => {}
    }
    for ch in chars {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
            return InvalidTokenKeySnafu {
                theme_id: theme_id.to_owned(),
                namespace,
                key: key.to_owned(),
                reason: format!("character {ch:?} is not in [a-z0-9_-]"),
            }
            .fail();
        }
    }
    Ok(())
}

/// Theme-level metadata. The `id` here must match the on-disk filename and
/// pass [`ThemeId`] parsing.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Meta {
    /// Stable identifier (filesystem-safe, registry key).
    pub id: ThemeId,
    /// Human-readable label.
    #[serde(default)]
    pub title: Option<String>,
    /// One-line description of the brand or design intent.
    #[serde(default)]
    pub description: Option<String>,
}

/// `[color.*]` — the color token namespace. Three layers so components bind to
/// meaning, not literals:
///
/// - `role` — named brand colors (the *only* place raw hex lives),
/// - `tone` — semantic names (positive, before, neutral, accent) that point at
///   roles, so brands remap freely,
/// - `surface` — page/ink/muted/rule slots used by chrome and structural elements.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ColorTokens {
    /// Named brand colors. Values are concrete [`HexColor`]s.
    #[serde(default)]
    pub role: IndexMap<String, HexColor>,
    /// Semantic tones. Each entry is a role *name*; resolution turns it into a hex.
    #[serde(default)]
    pub tone: IndexMap<String, String>,
    /// Surface slots. Each entry is a role *name*; resolution turns it into a hex.
    #[serde(default)]
    pub surface: IndexMap<String, String>,
}

/// `[type.*]` — typography tokens.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TypeTokens {
    /// `[type.family]` — typeface family stacks. Each entry is a fallback list.
    #[serde(default)]
    pub family: IndexMap<String, Vec<String>>,
    /// `[type.scale]` — pixel sizes at `[grid].base_canvas`.
    #[serde(default)]
    pub scale: IndexMap<String, u32>,
    /// `[type.role]` — composite text roles (title, `hero_number`, eyebrow, …).
    #[serde(default)]
    pub role: IndexMap<String, TypeRole>,
}

/// A composite text role. Every field is optional so themes may declare only
/// the slots that deviate from the family/scale defaults.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TypeRole {
    /// Reference into [`TypeTokens::family`] (e.g. `"sans"`).
    #[serde(default)]
    pub family: Option<String>,
    /// Numeric weight (100..=900).
    #[serde(default)]
    pub weight: Option<u16>,
    /// Reference into [`TypeTokens::scale`] (e.g. `"title"`).
    #[serde(default)]
    pub size: Option<String>,
    /// Tracking in `em`.
    #[serde(default)]
    pub tracking: Option<f32>,
    /// Leading (line-height) as a unitless multiplier.
    #[serde(default)]
    pub leading: Option<f32>,
    /// Reference into [`ColorTokens::role`] / [`ColorTokens::tone`] /
    /// [`ColorTokens::surface`]; the resolver disambiguates.
    #[serde(default)]
    pub color: Option<String>,
}

/// `[space]` — spacing scale (padding, gaps, margins) in canvas pixels.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SpaceTokens {
    /// Named spacing slots.
    #[serde(flatten, default)]
    pub slots: IndexMap<String, u32>,
}

/// `[grid]` — layout grid: base canvas + column system.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GridTokens {
    /// `base_canvas = [width, height]` in pixels. Drives `type.scale` units.
    #[serde(default)]
    pub base_canvas: Option<[u32; 2]>,
    /// Aspect ratio token (e.g. `"16:9"`).
    #[serde(default)]
    pub aspect: Option<String>,
    /// Number of columns in the grid.
    #[serde(default)]
    pub columns: Option<u32>,
    /// Gutter width in canvas pixels.
    #[serde(default)]
    pub gutter: Option<u32>,
    /// Outer margin in canvas pixels.
    #[serde(default)]
    pub margin: Option<u32>,
}

/// `[table]` — table chrome tokens. Values are color role/tone names.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TableTokens {
    /// Header fill (color reference).
    #[serde(default)]
    pub header_fill: Option<String>,
    /// Header ink (color reference).
    #[serde(default)]
    pub header_ink: Option<String>,
    /// Zebra-stripe fill (color reference).
    #[serde(default)]
    pub zebra: Option<String>,
    /// Border color (color reference).
    #[serde(default)]
    pub border: Option<String>,
    /// Whether to suppress vertical borders.
    #[serde(default)]
    pub no_vertical_borders: bool,
}

/// `[chart]` — chart palette tokens.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ChartTokens {
    /// Ordered series palette (color references).
    #[serde(default)]
    pub series: Vec<String>,
    /// Gridline color reference.
    #[serde(default)]
    pub gridline: Option<String>,
    /// Label color reference.
    #[serde(default)]
    pub label: Option<String>,
}

/// A `#rrggbb` color literal — the only place in the system where raw hex lives.
///
/// Construction is the parse boundary: `HexColor::parse("#1E293B")` accepts the
/// canonical seven-character form. The internal representation normalizes to
/// uppercase so two themes that differ only by case emit byte-identical CSS.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HexColor(String);

impl HexColor {
    /// Parse `#rrggbb` or `#rgb` shorthand into a canonical `#RRGGBB`.
    ///
    /// Time: O(n) in candidate length. Space: O(1) — the output is a
    /// fixed seven-byte string.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHexColor`] if the candidate does not match
    /// `#[0-9a-fA-F]{3}` or `#[0-9a-fA-F]{6}`.
    pub fn parse(candidate: &str) -> Result<Self, InvalidHexColor> {
        let stripped = candidate.strip_prefix('#').ok_or_else(|| InvalidHexColor {
            candidate: candidate.to_owned(),
        })?;
        let normalized = match stripped.len() {
            3 => {
                let mut buf = String::with_capacity(6);
                for c in stripped.chars() {
                    if !c.is_ascii_hexdigit() {
                        return Err(InvalidHexColor {
                            candidate: candidate.to_owned(),
                        });
                    }
                    let upper = c.to_ascii_uppercase();
                    buf.push(upper);
                    buf.push(upper);
                }
                buf
            }
            6 => {
                if !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(InvalidHexColor {
                        candidate: candidate.to_owned(),
                    });
                }
                stripped.to_ascii_uppercase()
            }
            _ => {
                return Err(InvalidHexColor {
                    candidate: candidate.to_owned(),
                });
            }
        };
        Ok(Self(format!("#{normalized}")))
    }

    /// Canonical `#RRGGBB` form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The six-hex-digit body, no leading `#`. OOXML embeds the digits in
    /// `<a:srgbClr val="..."/>` and rejects the `#` prefix.
    #[must_use]
    pub fn body(&self) -> &str {
        // INVARIANT: constructors guarantee a leading `#`; a missing `#` is a
        // programming bug that must surface loudly rather than emit invalid OOXML.
        self.0
            .strip_prefix('#')
            .unwrap_or_else(|| panic!("HexColor invariant: inner string must begin with '#'"))
    }
}

impl std::fmt::Display for HexColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for HexColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Returned when [`HexColor::parse`] rejects a candidate.
#[derive(Debug, snafu::Snafu, PartialEq, Eq)]
#[snafu(display("invalid hex color {candidate:?}; expected #rgb or #rrggbb"))]
pub struct InvalidHexColor {
    /// The verbatim input that was rejected.
    pub candidate: String,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_long_form() {
        let c = HexColor::parse("#1E293B").expect("#1E293B must parse");
        assert_eq!(c.as_str(), "#1E293B");
    }

    #[test]
    fn hex_parses_short_form_to_canonical() {
        let c = HexColor::parse("#fff").expect("#fff must parse");
        assert_eq!(c.as_str(), "#FFFFFF");
    }

    #[test]
    fn hex_uppercases_lowercase_long_form() {
        let c = HexColor::parse("#0f172a").expect("lowercase long form must parse");
        assert_eq!(c.as_str(), "#0F172A");
    }

    #[test]
    fn hex_rejects_missing_hash() {
        assert!(HexColor::parse("1E293B").is_err(), "missing # must reject");
    }

    #[test]
    fn hex_rejects_non_hex_digit() {
        assert!(HexColor::parse("#zz1234").is_err(), "non-hex must reject");
    }

    #[test]
    fn hex_rejects_wrong_length() {
        assert!(HexColor::parse("#12345").is_err(), "5 chars must reject");
        assert!(HexColor::parse("#1234567").is_err(), "7 chars must reject");
    }

    #[test]
    fn hex_body_strips_hash() {
        let c = HexColor::parse("#0D9488").expect("#0D9488 must parse");
        assert_eq!(c.body(), "0D9488");
    }

    fn theme_with_role_key(key: &str) -> Theme {
        let mut role = IndexMap::new();
        role.insert(
            key.to_owned(),
            HexColor::parse("#1E293B").expect("hex parses"),
        );
        Theme {
            meta: Meta {
                id: ThemeId::parse("protos").expect("id parses"),
                title: None,
                description: None,
            },
            color: ColorTokens {
                role,
                tone: IndexMap::new(),
                surface: IndexMap::new(),
            },
            r#type: TypeTokens::default(),
            space: SpaceTokens::default(),
            grid: GridTokens::default(),
            table: TableTokens::default(),
            chart: ChartTokens::default(),
        }
    }

    #[test]
    fn validate_token_keys_accepts_canonical_key() {
        theme_with_role_key("navy_2")
            .validate_token_keys()
            .expect("canonical key must pass");
    }

    #[test]
    fn validate_token_keys_rejects_css_injection_key() {
        // SECURITY(#5633): a crafted key attempting to break out of the
        // `--color-<key>: <hex>;` declaration and open a new ruleset.
        let err = theme_with_role_key("x}: body { color: red; --y")
            .validate_token_keys()
            .expect_err("CSS-breaking key must reject");
        assert!(matches!(
            err,
            ThemeError::InvalidTokenKey {
                namespace: "color.role",
                ..
            }
        ));
    }

    #[test]
    fn validate_token_keys_rejects_uppercase() {
        let err = theme_with_role_key("Navy")
            .validate_token_keys()
            .expect_err("uppercase must reject");
        assert!(matches!(err, ThemeError::InvalidTokenKey { .. }));
    }

    #[test]
    fn validate_token_keys_rejects_leading_digit() {
        let err = theme_with_role_key("2navy")
            .validate_token_keys()
            .expect_err("leading digit must reject");
        assert!(matches!(err, ThemeError::InvalidTokenKey { .. }));
    }

    #[test]
    fn validate_token_keys_rejects_empty_key() {
        let err = theme_with_role_key("")
            .validate_token_keys()
            .expect_err("empty key must reject");
        assert!(matches!(err, ThemeError::InvalidTokenKey { .. }));
    }

    #[test]
    fn validate_token_keys_rejects_newline_in_type_family_key() {
        // SECURITY(#5633): a key with an embedded newline could close a
        // Typst `//` comment or a LaTeX `%%` comment line early.
        let mut theme = theme_with_role_key("navy");
        theme
            .r#type
            .family
            .insert("sans\n#panic()".to_owned(), vec!["Geist".to_owned()]);
        let err = theme
            .validate_token_keys()
            .expect_err("newline in key must reject");
        assert!(matches!(
            err,
            ThemeError::InvalidTokenKey {
                namespace: "type.family",
                ..
            }
        ));
    }
}
