use std::fmt::Write;

use snafu::ResultExt;

use crate::error::{SinkSnafu, ThemeError};
use crate::resolved::ResolvedTheme;

/// Emit the theme as Typst `#let` variable declarations.
///
/// Downstream Typst templates `#import` or `#include` this file to access
/// brand colors, typography, spacing, grid, and chart palette.
///
/// Variable naming:
///
/// ```text
/// color-<role>    rgb("#RRGGBB")          (one per [color.role])
/// tone-<name>     rgb("#RRGGBB")          (one per [color.tone])
/// surface-<name>  rgb("#RRGGBB")          (one per [color.surface])
/// type-family-<name>  ("Geist", ...)      (one per [type.family])
/// type-scale-<name>   <px>                (one per [type.scale])
/// space-<name>    <px>                    (one per [space])
/// grid-<slot>     <value>                 (one per present [grid] field)
/// chart-series    (rgb("#..."), ...)      (resolved palette tuple)
/// ```
///
/// The output is deterministic: every map is emitted in declaration order
/// (preserved by [`indexmap::IndexMap`]); colors use the canonical uppercase
/// `#RRGGBB` form; integer values carry no fractional digits. The same
/// [`ResolvedTheme`] always produces byte-identical output.
///
/// # Errors
///
/// Returns [`ThemeError::Sink`] only if the underlying [`std::fmt::Write`]
/// implementation fails. For `String` this is structurally unreachable; the
/// variant exists for composition with non-allocating sinks.
pub fn emit_typst_template(theme: &ResolvedTheme) -> Result<String, ThemeError> {
    let mut out = String::new();
    write_typst(&mut out, theme).context(SinkSnafu {
        sink: "typst".to_owned(),
    })?;
    Ok(out)
}

fn write_typst(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    writeln!(out, "// poiesis-theme: {}", theme.id)?;
    writeln!(out, "// Generated — do not edit.")?;

    // Colors — roles
    if !theme.role.is_empty() {
        writeln!(out)?;
        writeln!(out, "// Colors — roles")?;
        for (name, color) in &theme.role {
            writeln!(out, "#let color-{name} = rgb(\"{}\")", color.as_str())?;
        }
    }

    // Colors — tones
    if !theme.tone.is_empty() {
        writeln!(out)?;
        writeln!(out, "// Colors — tones")?;
        for (name, color) in &theme.tone {
            writeln!(out, "#let tone-{name} = rgb(\"{}\")", color.as_str())?;
        }
    }

    // Colors — surfaces
    if !theme.surface.is_empty() {
        writeln!(out)?;
        writeln!(out, "// Colors — surfaces")?;
        for (name, color) in &theme.surface {
            writeln!(out, "#let surface-{name} = rgb(\"{}\")", color.as_str())?;
        }
    }

    // Typography — families
    if !theme.r#type.family.is_empty() {
        writeln!(out)?;
        writeln!(out, "// Typography — families")?;
        for (name, stack) in &theme.r#type.family {
            write!(out, "#let type-family-{name} = (")?;
            for (i, s) in stack.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ")?;
                }
                write!(out, "\"{s}\"")?;
            }
            writeln!(out, ")")?;
        }
    }

    // Typography — scale (px)
    if !theme.r#type.scale.is_empty() {
        writeln!(out)?;
        writeln!(out, "// Typography — scale (px)")?;
        for (name, px) in &theme.r#type.scale {
            writeln!(out, "#let type-scale-{name} = {px}")?;
        }
    }

    // Space (px)
    if !theme.space.slots.is_empty() {
        writeln!(out)?;
        writeln!(out, "// Space (px)")?;
        for (name, px) in &theme.space.slots {
            writeln!(out, "#let space-{name} = {px}")?;
        }
    }

    // Grid
    let has_grid = theme.grid.base_canvas.is_some()
        || theme.grid.columns.is_some()
        || theme.grid.gutter.is_some()
        || theme.grid.margin.is_some();
    if has_grid {
        writeln!(out)?;
        writeln!(out, "// Grid")?;
        if let Some([w, h]) = theme.grid.base_canvas {
            writeln!(out, "#let grid-canvas-w = {w}")?;
            writeln!(out, "#let grid-canvas-h = {h}")?;
        }
        if let Some(columns) = theme.grid.columns {
            writeln!(out, "#let grid-columns = {columns}")?;
        }
        if let Some(gutter) = theme.grid.gutter {
            writeln!(out, "#let grid-gutter = {gutter}")?;
        }
        if let Some(margin) = theme.grid.margin {
            writeln!(out, "#let grid-margin = {margin}")?;
        }
    }

    // Chart series
    if !theme.chart.series.is_empty() {
        writeln!(out)?;
        writeln!(out, "// Chart series")?;
        write!(out, "#let chart-series = (")?;
        let mut first = true;
        for series_ref in &theme.chart.series {
            if let Some(color) = theme.lookup_color(series_ref) {
                if !first {
                    write!(out, ", ")?;
                }
                write!(out, "rgb(\"{}\")", color.as_str())?;
                first = false;
            }
        }
        writeln!(out, ")")?;
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn summus() -> ResolvedTheme {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes");
        let registry = crate::registry::Registry::load_dir(&dir).expect("load");
        registry
            .resolve(&crate::registry::parse_theme_id("summus").expect("id"))
            .expect("resolve")
    }

    #[test]
    fn typst_starts_with_header_comment() {
        let typst = emit_typst_template(&summus()).expect("emit summus typst");
        assert!(
            typst.starts_with("// poiesis-theme: summus"),
            "output must start with theme header comment; got:\n{typst}"
        );
    }

    #[test]
    fn typst_emits_navy_role() {
        let typst = emit_typst_template(&summus()).expect("emit summus typst");
        assert!(
            typst.contains("#let color-navy = rgb(\"#232E54\")"),
            "navy role must appear verbatim; got:\n{typst}"
        );
    }

    #[test]
    fn typst_emits_positive_tone() {
        let typst = emit_typst_template(&summus()).expect("emit summus typst");
        assert!(
            typst.contains("#let tone-positive = rgb(\"#318891\")"),
            "positive tone must appear with resolved teal hex; got:\n{typst}"
        );
    }

    #[test]
    fn typst_family_is_array_literal() {
        let typst = emit_typst_template(&summus()).expect("emit summus typst");
        assert!(
            typst.contains("(\"Geist\""),
            "sans family must open as a Typst array literal; got:\n{typst}"
        );
    }

    #[test]
    fn typst_byte_stable_across_runs() {
        let a = emit_typst_template(&summus()).expect("first emit");
        let b = emit_typst_template(&summus()).expect("second emit");
        assert_eq!(a, b, "two emissions must match byte-for-byte");
    }
}
