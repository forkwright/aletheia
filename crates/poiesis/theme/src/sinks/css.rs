use std::fmt::Write;

use snafu::ResultExt;

use crate::error::{SinkSnafu, ThemeError};
use crate::resolved::ResolvedTheme;

/// Emit the theme as CSS custom properties on `:root`.
///
/// Variable names follow the convention surfaced in B-002:
///
/// ```text
/// --color-<role>   : #RRGGBB                       (one per [color.role])
/// --tone-<name>    : var(--color-<role>) or #hex   (one per [color.tone])
/// --surface-<name> : #RRGGBB                       (one per [color.surface])
/// --type-<role>-size, --type-<role>-weight, ...     (one per [type.role] slot)
/// --space-<name>   : <px>px                        (one per [space] slot)
/// ```
///
/// The output is deterministic: every map is emitted in declaration order
/// (preserved by [`indexmap::IndexMap`]); CSS values use the canonical
/// uppercase `#RRGGBB` form; integer values carry no fractional digits. The
/// same [`ResolvedTheme`] always produces byte-identical output so callers
/// can fingerprint or diff brand assets without normalization.
///
/// # Errors
///
/// Returns [`ThemeError::Sink`] only if the underlying [`std::fmt::Write`]
/// implementation fails. For `String` this is structurally unreachable; the
/// variant exists for composition with non-allocating sinks.
pub fn emit_css(theme: &ResolvedTheme) -> Result<String, ThemeError> {
    let mut out = String::new();
    write_css(&mut out, theme).context(SinkSnafu {
        sink: "css".to_owned(),
    })?;
    Ok(out)
}

fn write_css(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    writeln!(out, "/* poiesis-theme: {} */", theme.id)?;
    writeln!(out, ":root {{")?;
    write_colors(out, theme)?;
    write_typography(out, theme)?;
    write_layout(out, theme)?;
    write_chrome(out, theme)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn write_colors(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    for (name, color) in &theme.role {
        writeln!(out, "  --color-{}: {};", name, color.as_str())?;
    }
    for (name, value) in &theme.tone {
        // The TOML source pointed each tone at a role name; recover that role
        // name so the emission reads `--tone-positive: var(--color-teal);`
        // instead of duplicating the hex. If two roles share a hex (rare)
        // the first match wins; both `var(...)` references resolve to the
        // same color so the CSS remains correct.
        let role_name = theme
            .role
            .iter()
            .find(|(_, role_value)| *role_value == value)
            .map(|(role_name, _)| role_name.as_str());
        if let Some(role_name) = role_name {
            writeln!(out, "  --tone-{name}: var(--color-{role_name});")?;
        } else {
            writeln!(out, "  --tone-{name}: {};", value.as_str())?;
        }
    }
    for (name, color) in &theme.surface {
        writeln!(out, "  --surface-{}: {};", name, color.as_str())?;
    }
    Ok(())
}

fn write_typography(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    for (name, stack) in &theme.r#type.family {
        let joined = stack
            .iter()
            .map(|s| {
                if s.chars().any(char::is_whitespace) {
                    format!("\"{s}\"")
                } else {
                    s.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "  --family-{name}: {joined};")?;
    }
    for (name, px) in &theme.r#type.scale {
        writeln!(out, "  --scale-{name}: {px}px;")?;
    }
    for (name, role) in &theme.r#type.role {
        if let Some(family) = &role.family {
            writeln!(out, "  --type-{name}-family: var(--family-{family});")?;
        }
        if let Some(weight) = role.weight {
            writeln!(out, "  --type-{name}-weight: {weight};")?;
        }
        if let Some(size) = &role.size {
            writeln!(out, "  --type-{name}-size: var(--scale-{size});")?;
        }
        if let Some(tracking) = role.tracking {
            writeln!(out, "  --type-{name}-tracking: {tracking}em;")?;
        }
        if let Some(leading) = role.leading {
            writeln!(out, "  --type-{name}-leading: {leading};")?;
        }
        if let Some(color) = &role.color {
            // The reference may name a role, tone, or surface; emit a var()
            // of the corresponding prefix. SECURITY(#5633): an unresolved
            // reference is skipped rather than falling through to a guessed
            // prefix — `color` is a raw TOML-authored string that has not
            // itself passed key-charset validation, so interpolating it
            // unconditionally would reopen the CSS-injection class the
            // resolved-key validation closes.
            if let Some(prefix) = pick_color_prefix(theme, color) {
                writeln!(out, "  --type-{name}-color: var(--{prefix}-{color});")?;
            }
        }
    }
    Ok(())
}

fn write_layout(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    for (name, px) in &theme.space.slots {
        writeln!(out, "  --space-{name}: {px}px;")?;
    }
    if let Some([w, h]) = theme.grid.base_canvas {
        writeln!(out, "  --grid-canvas-w: {w}px;")?;
        writeln!(out, "  --grid-canvas-h: {h}px;")?;
    }
    if let Some(columns) = theme.grid.columns {
        writeln!(out, "  --grid-columns: {columns};")?;
    }
    if let Some(gutter) = theme.grid.gutter {
        writeln!(out, "  --grid-gutter: {gutter}px;")?;
    }
    if let Some(margin) = theme.grid.margin {
        writeln!(out, "  --grid-margin: {margin}px;")?;
    }
    Ok(())
}

fn write_chrome(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    for (slot, value) in [
        ("header-fill", &theme.table.header_fill),
        ("header-ink", &theme.table.header_ink),
        ("zebra", &theme.table.zebra),
        ("border", &theme.table.border),
    ] {
        if let Some(v) = value
            && let Some(prefix) = pick_color_prefix(theme, v)
        {
            writeln!(out, "  --table-{slot}: var(--{prefix}-{v});")?;
        }
    }
    for (i, series) in theme.chart.series.iter().enumerate() {
        if let Some(prefix) = pick_color_prefix(theme, series) {
            let index = i + 1;
            writeln!(out, "  --chart-series-{index}: var(--{prefix}-{series});")?;
        }
    }
    if let Some(gridline) = &theme.chart.gridline
        && let Some(prefix) = pick_color_prefix(theme, gridline)
    {
        writeln!(out, "  --chart-gridline: var(--{prefix}-{gridline});")?;
    }
    if let Some(label) = &theme.chart.label
        && let Some(prefix) = pick_color_prefix(theme, label)
    {
        writeln!(out, "  --chart-label: var(--{prefix}-{label});")?;
    }
    Ok(())
}

/// Resolve a raw theme-authored color reference (a `[table]`/`[chart]`/
/// `[type.role]` value naming a role/tone/surface key) to its CSS
/// custom-property namespace prefix.
///
/// SECURITY(#5633): returns `None` for an unresolved reference rather than
/// guessing `"color"`. Role/tone/surface *keys* are charset-validated at
/// [`crate::resolved::ResolvedTheme::from_theme`], but a reference *value*
/// like this one is an independent, unvalidated TOML-authored string; if it
/// doesn't match a known key, interpolating it unconditionally (the old
/// `"color"`-fallback behavior) would re-open the same CSS-injection class
/// the key-charset validation closes, just one hop further down the theme
/// TOML shape. Every call site skips emission on `None`, matching how the
/// typst/latex sinks already skip an unresolved chart-series reference via
/// `ResolvedTheme::lookup_color`.
fn pick_color_prefix(theme: &ResolvedTheme, name: &str) -> Option<&'static str> {
    if theme.role.contains_key(name) {
        Some("color")
    } else if theme.tone.contains_key(name) {
        Some("tone")
    } else if theme.surface.contains_key(name) {
        Some("surface")
    } else {
        None
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::registry::Registry;

    fn protos() -> ResolvedTheme {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes");
        let registry = Registry::load_dir(&dir).expect("load protos");
        let id = crate::registry::parse_theme_id("protos").expect("parse protos");
        registry.resolve(&id).expect("resolve protos")
    }

    #[test]
    fn css_contains_color_role_hex() {
        let css = emit_css(&protos()).expect("emit protos css");
        assert!(
            css.contains("--color-navy: #1E293B;"),
            "navy role hex must appear verbatim; got:\n{css}"
        );
        assert!(
            css.contains("--color-teal: #0D9488;"),
            "teal role hex must appear verbatim; got:\n{css}"
        );
    }

    #[test]
    fn css_tone_references_role_via_var() {
        let css = emit_css(&protos()).expect("emit protos css");
        assert!(
            css.contains("--tone-positive: var(--color-teal);"),
            "positive tone must point at color-teal via var(): {css}"
        );
        assert!(
            css.contains("--tone-before: var(--color-rose);"),
            "before tone must point at color-rose via var(): {css}"
        );
    }

    #[test]
    fn css_surface_uses_resolved_hex() {
        let css = emit_css(&protos()).expect("emit protos css");
        assert!(
            css.contains("--surface-page: #FFFFFF;"),
            "page surface must resolve to bg hex"
        );
    }

    #[test]
    fn css_emits_scale_in_pixels() {
        let css = emit_css(&protos()).expect("emit protos css");
        assert!(
            css.contains("--scale-title: 64px;"),
            "title scale must be 64px"
        );
        assert!(
            css.contains("--scale-hero: 128px;"),
            "hero scale must be 128px"
        );
    }

    #[test]
    fn css_family_quotes_multiword_names() {
        let css = emit_css(&protos()).expect("emit protos css");
        assert!(
            css.contains("--family-mono: \"Geist Mono\", ui-monospace, monospace;"),
            "multi-word family names must be quoted: {css}"
        );
    }

    #[test]
    fn css_byte_stable_across_runs() {
        let a = emit_css(&protos()).expect("first emit");
        let b = emit_css(&protos()).expect("second emit");
        assert_eq!(a, b, "two emissions must match byte-for-byte");
    }

    #[test]
    fn css_chart_series_resolves_through_tone() {
        let css = emit_css(&protos()).expect("emit protos css");
        assert!(
            css.contains("--chart-series-1: var(--tone-accent);"),
            "first series should resolve through tone prefix: {css}"
        );
    }

    #[test]
    fn css_skips_unresolved_chart_gridline_reference() {
        // SECURITY(#5633): a `[chart].gridline` value that names no known
        // role/tone/surface must not be interpolated raw into CSS — it must
        // simply not appear, exactly like an unresolved chart-series entry
        // already doesn't (see the typst/latex sinks' matching behavior).
        let mut theme = protos();
        theme.chart.gridline = Some("x}: body { color: red; --y".to_owned());
        let css = emit_css(&theme).expect("emit must still succeed");
        assert!(
            !css.contains("--chart-gridline"),
            "unresolved gridline reference must be skipped entirely, not emitted raw: {css}"
        );
        assert!(
            !css.contains("color: red"),
            "the crafted reference string must never reach the output: {css}"
        );
    }

    #[test]
    fn css_skips_unresolved_table_header_fill_reference() {
        let mut theme = protos();
        theme.table.header_fill = Some("}; * { display: none".to_owned());
        let css = emit_css(&theme).expect("emit must still succeed");
        assert!(
            !css.contains("--table-header-fill"),
            "unresolved table reference must be skipped entirely: {css}"
        );
        assert!(!css.contains("display: none"));
    }
}
