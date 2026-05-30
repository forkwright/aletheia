use std::fmt::Write;

use snafu::ResultExt;

use crate::error::{SinkSnafu, ThemeError};
use crate::resolved::ResolvedTheme;

/// Emit a LaTeX preamble fragment with `\definecolor` and `\newcommand`
/// declarations for every brand token in the [`ResolvedTheme`].
///
/// Downstream `.tex` files `\input` or `\include` this file to access brand
/// colors, typography metrics, spacing, and grid constants.
///
/// The output is deterministic: every map is emitted in declaration order
/// (preserved by [`indexmap::IndexMap`]). The same [`ResolvedTheme`] always
/// produces byte-identical output.
///
/// # Errors
///
/// Returns [`ThemeError::Sink`] only if the underlying [`std::fmt::Write`]
/// implementation fails. For `String` this is structurally unreachable; the
/// variant exists for composition with non-allocating sinks.
pub fn emit_latex_template(theme: &ResolvedTheme) -> Result<String, ThemeError> {
    let mut out = String::new();
    write_latex(&mut out, theme).context(SinkSnafu {
        sink: "latex".to_owned(),
    })?;
    Ok(out)
}

fn write_latex(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    writeln!(out, "%% poiesis-theme: {}", theme.id)?;
    writeln!(out, "%% Generated — do not edit.")?;
    writeln!(out)?;
    writeln!(
        out,
        "%% Requires \\usepackage[dvipsnames,svgnames,x11names]{{xcolor}}"
    )?;

    let mut needs_blank = false;

    // Colors — roles
    if !theme.role.is_empty() {
        if needs_blank {
            writeln!(out)?;
        }
        needs_blank = true;
        writeln!(out, "%% Colors — roles")?;
        for (name, color) in &theme.role {
            writeln!(
                out,
                "\\definecolor{{color-{}}}{{HTML}}{{{}}}",
                name,
                color.body()
            )?;
        }
    }

    // Colors — tones
    if !theme.tone.is_empty() {
        if needs_blank {
            writeln!(out)?;
        }
        needs_blank = true;
        writeln!(out, "%% Colors — tones")?;
        for (name, color) in &theme.tone {
            writeln!(
                out,
                "\\definecolor{{tone-{}}}{{HTML}}{{{}}}",
                name,
                color.body()
            )?;
        }
    }

    // Colors — surfaces
    if !theme.surface.is_empty() {
        if needs_blank {
            writeln!(out)?;
        }
        needs_blank = true;
        writeln!(out, "%% Colors — surfaces")?;
        for (name, color) in &theme.surface {
            writeln!(
                out,
                "\\definecolor{{surface-{}}}{{HTML}}{{{}}}",
                name,
                color.body()
            )?;
        }
    }

    // Typography — families
    if !theme.r#type.family.is_empty() {
        if needs_blank {
            writeln!(out)?;
        }
        needs_blank = true;
        writeln!(out, "%% Typography — families")?;
        for (name, stack) in &theme.r#type.family {
            if let Some(first) = stack.first() {
                let cmd = format!("typeFamily{}", capitalize(name));
                writeln!(out, "\\newcommand{{\\{cmd}}}{{{first}}}")?;
            }
        }
    }

    // Typography — scale
    if !theme.r#type.scale.is_empty() {
        if needs_blank {
            writeln!(out)?;
        }
        needs_blank = true;
        writeln!(out, "%% Typography — scale (pt, 1pt ≈ 1.333px)")?;
        for (name, px) in &theme.r#type.scale {
            let pt = px_to_pt(*px);
            let cmd = format!("typeScale{}", capitalize(name));
            writeln!(out, "\\newcommand{{\\{cmd}}}{{{pt}}}")?;
        }
    }

    // Space
    if !theme.space.slots.is_empty() {
        if needs_blank {
            writeln!(out)?;
        }
        needs_blank = true;
        writeln!(out, "%% Space (pt)")?;
        for (name, px) in &theme.space.slots {
            let pt = px_to_pt(*px);
            let cmd = format!("space{}", capitalize(name));
            writeln!(out, "\\newcommand{{\\{cmd}}}{{{pt}}}")?;
        }
    }

    // Grid
    let has_grid = theme.grid.columns.is_some()
        || theme.grid.gutter.is_some()
        || theme.grid.margin.is_some()
        || theme.grid.base_canvas.is_some();
    if has_grid {
        if needs_blank {
            writeln!(out)?;
        }
        needs_blank = true;
        writeln!(out, "%% Grid")?;
        if let Some(columns) = theme.grid.columns {
            writeln!(out, "\\newcommand{{\\gridColumns}}{{{columns}}}")?;
        }
        if let Some(gutter) = theme.grid.gutter {
            let pt = px_to_pt(gutter);
            writeln!(out, "\\newcommand{{\\gridGutter}}{{{pt}}}")?;
        }
        if let Some(margin) = theme.grid.margin {
            let pt = px_to_pt(margin);
            writeln!(out, "\\newcommand{{\\gridMargin}}{{{pt}}}")?;
        }
        if let Some([w, h]) = theme.grid.base_canvas {
            let w_pt = px_to_pt(w);
            let h_pt = px_to_pt(h);
            writeln!(out, "\\newcommand{{\\gridCanvasW}}{{{w_pt}}}")?;
            writeln!(out, "\\newcommand{{\\gridCanvasH}}{{{h_pt}}}")?;
        }
    }

    // Chart series
    let mut chart_emitted = false;
    for (i, series) in theme.chart.series.iter().enumerate() {
        if let Some(color) = theme.lookup_color(series) {
            if !chart_emitted {
                if needs_blank {
                    writeln!(out)?;
                }
                writeln!(out, "%% Chart series")?;
                chart_emitted = true;
            }
            let index = i + 1;
            writeln!(
                out,
                "\\definecolor{{chart-series-{index}}}{{HTML}}{{{}}}",
                color.body()
            )?;
        }
    }

    Ok(())
}

/// Convert pixels to points (1 px ≈ 0.75 pt) and round to the nearest 0.5 pt.
fn px_to_pt(px: u32) -> String {
    let raw = f64::from(px) * 0.75;
    let rounded = (raw * 2.0).round() / 2.0;
    // Emit without trailing `.0` when the value is a whole number.
    if (rounded * 2.0).round().rem_euclid(2.0) == 0.0 {
        format!("{:.0}pt", rounded)
    } else {
        format!("{:.1}pt", rounded)
    }
}

/// Capitalize the first character of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut result = first.to_uppercase().collect::<String>();
            result.push_str(chars.as_str());
            result
        }
    }
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
    fn latex_starts_with_header_comment() {
        let latex = emit_latex_template(&summus()).expect("emit summus latex");
        assert!(
            latex.starts_with("%% poiesis-theme: summus"),
            "output must start with theme header; got:\n{latex}"
        );
    }

    #[test]
    fn latex_emits_navy_role() {
        let latex = emit_latex_template(&summus()).expect("emit summus latex");
        assert!(
            latex.contains("\\definecolor{color-navy}{HTML}{232E54}"),
            "navy role must appear verbatim; got:\n{latex}"
        );
    }

    #[test]
    fn latex_emits_positive_tone() {
        let latex = emit_latex_template(&summus()).expect("emit summus latex");
        assert!(
            latex.contains("\\definecolor{tone-positive}{HTML}{318891}"),
            "positive tone must resolve to teal hex; got:\n{latex}"
        );
    }

    #[test]
    fn latex_family_command_uses_first_element() {
        let latex = emit_latex_template(&summus()).expect("emit summus latex");
        assert!(
            latex.contains("\\newcommand{\\typeFamilySans}{Geist}"),
            "sans family command must use first stack element; got:\n{latex}"
        );
    }

    #[test]
    fn latex_byte_stable_across_runs() {
        let a = emit_latex_template(&summus()).expect("first emit");
        let b = emit_latex_template(&summus()).expect("second emit");
        assert_eq!(a, b, "two emissions must match byte-for-byte");
    }
}
