use indexmap::IndexMap;

use crate::error::{ThemeError, UnknownRoleSnafu};
use crate::id::ThemeId;
use crate::tokens::{
    ChartTokens, GridTokens, HexColor, SpaceTokens, TableTokens, Theme, TypeRole, TypeTokens,
};

/// A theme with every tone and surface reference resolved to a concrete
/// [`HexColor`]. This is the value the renderers consume.
///
/// Resolution rules:
///
/// - `color.role` is copied verbatim — roles already carry concrete hex values.
/// - `color.tone[name] = role_name` becomes `tone[name] = role_value`.
/// - `color.surface[name] = role_name` becomes `surface[name] = role_value`.
/// - Type roles, space slots, grid, table, and chart tables are copied unchanged;
///   their color references continue to live as *strings* and the sinks resolve
///   them at emission time against the same role/tone/surface maps. This
///   matches the architectural rule that the sink owns the emission format.
///
/// All maps preserve their original order (the order the TOML listed them in)
/// so emitted output is deterministic for the same input.
#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    /// Stable identifier of the theme this resolution belongs to.
    pub id: ThemeId,
    /// Optional human label, carried through from `[meta].title`.
    pub title: Option<String>,
    /// Optional description, carried through from `[meta].description`.
    pub description: Option<String>,
    /// Brand colors, ordered. The map is the source of truth for tone / surface
    /// lookups elsewhere in the resolution.
    pub role: IndexMap<String, HexColor>,
    /// Tone → resolved hex value.
    pub tone: IndexMap<String, HexColor>,
    /// Surface → resolved hex value.
    pub surface: IndexMap<String, HexColor>,
    /// Typography (families, scale, roles) — carried through unchanged.
    pub r#type: TypeTokens,
    /// Spacing — carried through unchanged.
    pub space: SpaceTokens,
    /// Grid — carried through unchanged.
    pub grid: GridTokens,
    /// Table chrome — carried through unchanged.
    pub table: TableTokens,
    /// Chart palette — carried through unchanged.
    pub chart: ChartTokens,
}

impl ResolvedTheme {
    /// Resolve a parsed [`Theme`] into its renderer-facing form.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeError::UnknownRole`] if any `[color.tone]` or
    /// `[color.surface]` entry names a role that does not exist in
    /// `[color.role]`. This is one of the structural reasons B-002 names a
    /// theme registry rather than a free-form palette: tone references are
    /// resolved here, not at every sink.
    pub fn from_theme(theme: Theme) -> Result<Self, ThemeError> {
        let id = theme.meta.id;
        let role = theme.color.role;
        let tone = resolve_color_map(&id, &role, theme.color.tone, "tone")?;
        let surface = resolve_color_map(&id, &role, theme.color.surface, "surface")?;
        Ok(Self {
            id,
            title: theme.meta.title,
            description: theme.meta.description,
            role,
            tone,
            surface,
            r#type: theme.r#type,
            space: theme.space,
            grid: theme.grid,
            table: theme.table,
            chart: theme.chart,
        })
    }

    /// Look up a color by reference, trying `role`, then `tone`, then `surface`.
    /// Returns `None` if no namespace contains the name.
    ///
    /// This is the runtime sibling of the `THEME/unknown-token` lint rule: the
    /// lint rejects unknown references at the spec boundary; this method
    /// surfaces the same condition during sink emission.
    #[must_use]
    pub fn lookup_color(&self, reference: &str) -> Option<&HexColor> {
        self.role
            .get(reference)
            .or_else(|| self.tone.get(reference))
            .or_else(|| self.surface.get(reference))
    }

    /// Look up a typography role by name.
    #[must_use]
    pub fn lookup_type_role(&self, name: &str) -> Option<&TypeRole> {
        self.r#type.role.get(name)
    }

    /// Look up a scale entry by name (e.g. `"title"` → `64`).
    #[must_use]
    pub fn lookup_scale(&self, name: &str) -> Option<u32> {
        self.r#type.scale.get(name).copied()
    }

    /// Look up a family stack by name (e.g. `"sans"` → `["Geist", …]`).
    /// Time: O(1) (`IndexMap` lookup). Space: O(1).
    #[must_use]
    pub fn lookup_family(&self, name: &str) -> Option<&[String]> {
        self.r#type.family.get(name).map(Vec::as_slice)
    }
}

fn resolve_color_map(
    theme_id: &ThemeId,
    role: &IndexMap<String, HexColor>,
    refs: IndexMap<String, String>,
    namespace: &'static str,
) -> Result<IndexMap<String, HexColor>, ThemeError> {
    let mut out = IndexMap::with_capacity(refs.len());
    for (name, target) in refs {
        let value = role.get(&target).cloned().ok_or_else(|| {
            UnknownRoleSnafu {
                theme_id: theme_id.as_str().to_owned(),
                tone_name: format!("{namespace}.{name}"),
                role: target.clone(),
            }
            .build()
        })?;
        out.insert(name, value);
    }
    Ok(out)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::tokens::{ColorTokens, Meta};

    fn parse_id(s: &str) -> ThemeId {
        ThemeId::parse(s).expect("test id must parse")
    }

    fn hex(s: &str) -> HexColor {
        HexColor::parse(s).expect("test hex must parse")
    }

    fn minimal_theme() -> Theme {
        let mut role = IndexMap::new();
        role.insert("navy".to_owned(), hex("#232E54"));
        role.insert("teal".to_owned(), hex("#318891"));
        let mut tone = IndexMap::new();
        tone.insert("positive".to_owned(), "teal".to_owned());
        let mut surface = IndexMap::new();
        surface.insert("page".to_owned(), "navy".to_owned());
        Theme {
            meta: Meta {
                id: parse_id("summus"),
                title: None,
                description: None,
            },
            color: ColorTokens {
                role,
                tone,
                surface,
            },
            r#type: TypeTokens::default(),
            space: SpaceTokens::default(),
            grid: GridTokens::default(),
            table: TableTokens::default(),
            chart: ChartTokens::default(),
        }
    }

    #[test]
    fn resolution_preserves_role_values() {
        let resolved = ResolvedTheme::from_theme(minimal_theme()).expect("must resolve");
        assert_eq!(
            resolved.role.get("navy").map(HexColor::as_str),
            Some("#232E54")
        );
    }

    #[test]
    fn resolution_dereferences_tone_to_role_value() {
        let resolved = ResolvedTheme::from_theme(minimal_theme()).expect("must resolve");
        assert_eq!(
            resolved.tone.get("positive").map(HexColor::as_str),
            Some("#318891"),
            "tone positive→teal must resolve to the teal hex"
        );
    }

    #[test]
    fn resolution_dereferences_surface() {
        let resolved = ResolvedTheme::from_theme(minimal_theme()).expect("must resolve");
        assert_eq!(
            resolved.surface.get("page").map(HexColor::as_str),
            Some("#232E54"),
        );
    }

    #[test]
    fn resolution_fails_on_unknown_role_in_tone() {
        let mut theme = minimal_theme();
        theme
            .color
            .tone
            .insert("ghost".to_owned(), "missing".to_owned());
        let err = ResolvedTheme::from_theme(theme).expect_err("missing role must reject");
        assert!(matches!(err, ThemeError::UnknownRole { .. }));
    }

    #[test]
    fn lookup_color_walks_role_then_tone_then_surface() {
        let resolved = ResolvedTheme::from_theme(minimal_theme()).expect("must resolve");
        assert_eq!(
            resolved.lookup_color("navy").map(HexColor::as_str),
            Some("#232E54"),
            "lookup must find role"
        );
        assert_eq!(
            resolved.lookup_color("positive").map(HexColor::as_str),
            Some("#318891"),
            "lookup must find tone"
        );
        assert_eq!(
            resolved.lookup_color("page").map(HexColor::as_str),
            Some("#232E54"),
            "lookup must find surface"
        );
        assert!(
            resolved.lookup_color("nope").is_none(),
            "unknown name must miss"
        );
    }

    #[test]
    fn resolution_preserves_role_order() {
        let resolved = ResolvedTheme::from_theme(minimal_theme()).expect("must resolve");
        let keys: Vec<&str> = resolved.role.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["navy", "teal"],
            "role iteration must match TOML order"
        );
    }
}
