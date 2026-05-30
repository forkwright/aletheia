use std::fmt::Write;

use snafu::ResultExt;

use crate::error::{SinkSnafu, ThemeError};
use crate::resolved::ResolvedTheme;
use crate::tokens::HexColor;

// WHY: the `xmlns:a` value below is the ECMA-376 DrawingML namespace
// identifier — a fixed URI literal mandated by the OOXML spec. PowerPoint and
// LibreOffice match it as an opaque string; it is never fetched. Substituting
// `https://` breaks every Office consumer (the namespace string must match
// the spec verbatim). See ECMA-376 Part 1 §A.4.1.
const OOXML_DRAWINGML_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// Emit the OOXML `theme1.xml` body — the `<a:clrScheme>` + `<a:fontScheme>`
/// that `PowerPoint` and `LibreOffice` read at file open to populate accent
/// swatches and the theme font picker.
///
/// The schema slot mapping follows the convention every Office consumer
/// expects (dk1=text/dark, lt1=background/light, dk2/lt2=secondary,
/// accent1..6 = the brand palette in order):
///
/// | OOXML slot | Source token        | Why                              |
/// |------------|---------------------|----------------------------------|
/// | `dk1`      | `surface.ink`       | primary text color               |
/// | `lt1`      | `surface.page`      | primary background               |
/// | `dk2`      | `tone.neutral` role | brand-dark accent (B-002 names) |
/// | `lt2`      | `surface.page_alt`  | brand-light fallback             |
/// | `accent1`  | `tone.accent` role  | primary brand accent             |
/// | `accent2`  | `tone.before` role  | secondary brand accent           |
/// | `accent3`  | first `color.role`  | fallback to first role           |
/// | `accent4..6` | next roles in order | populate from `color.role` order |
///
/// Defaults are conservative: if a slot's source token is absent, the slot
/// emits the canonical Office default (black/white). Native `PowerPoint`
/// chart series bind to `accent1..3`, so recoloring the theme recolors
/// charts as B-002 names.
///
/// # Errors
///
/// Returns [`ThemeError::Sink`] only if the underlying [`std::fmt::Write`]
/// fails. For `String` this is structurally unreachable.
pub fn emit_theme_xml(theme: &ResolvedTheme) -> Result<String, ThemeError> {
    let mut out = String::new();
    write_theme_xml(&mut out, theme).context(SinkSnafu {
        sink: "ooxml".to_owned(),
    })?;
    Ok(out)
}

fn write_theme_xml(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    writeln!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
    )?;
    writeln!(
        out,
        r#"<a:theme xmlns:a="{ns}" name="{name}">"#,
        ns = OOXML_DRAWINGML_NS,
        name = escape_xml(theme.id.as_str())
    )?;
    writeln!(out, "  <a:themeElements>")?;

    // ── clrScheme ────────────────────────────────────────────────────────────
    writeln!(
        out,
        r#"    <a:clrScheme name="{}">"#,
        escape_xml(theme.id.as_str())
    )?;

    write_color_slot(out, "dk1", color_for(theme, &["surface:ink"]), "000000")?;
    write_color_slot(out, "lt1", color_for(theme, &["surface:page"]), "FFFFFF")?;
    write_color_slot(out, "dk2", color_for(theme, &["tone:neutral"]), "000000")?;
    write_color_slot(
        out,
        "lt2",
        color_for(theme, &["surface:page_alt", "surface:page"]),
        "FFFFFF",
    )?;
    write_color_slot(
        out,
        "accent1",
        color_for(theme, &["tone:accent", "tone:positive"]),
        "4472C4",
    )?;
    write_color_slot(out, "accent2", color_for(theme, &["tone:before"]), "ED7D31")?;
    write_accent_slots_3_to_6(out, theme)?;

    write_color_slot(out, "hlink", color_for(theme, &["tone:accent"]), "0563C1")?;
    write_color_slot(
        out,
        "folHlink",
        color_for(theme, &["tone:before"]),
        "954F72",
    )?;
    writeln!(out, "    </a:clrScheme>")?;

    // ── fontScheme ───────────────────────────────────────────────────────────
    let major = primary_typeface(theme.lookup_family("serif").or(theme.lookup_family("sans")));
    let minor = primary_typeface(theme.lookup_family("sans"));
    writeln!(
        out,
        r#"    <a:fontScheme name="{}">"#,
        escape_xml(theme.id.as_str())
    )?;
    writeln!(out, "      <a:majorFont>")?;
    writeln!(
        out,
        r#"        <a:latin typeface="{}"/>"#,
        escape_xml(&major)
    )?;
    writeln!(out, r#"        <a:ea typeface=""/>"#)?;
    writeln!(out, r#"        <a:cs typeface=""/>"#)?;
    writeln!(out, "      </a:majorFont>")?;
    writeln!(out, "      <a:minorFont>")?;
    writeln!(
        out,
        r#"        <a:latin typeface="{}"/>"#,
        escape_xml(&minor)
    )?;
    writeln!(out, r#"        <a:ea typeface=""/>"#)?;
    writeln!(out, r#"        <a:cs typeface=""/>"#)?;
    writeln!(out, "      </a:minorFont>")?;
    writeln!(out, "    </a:fontScheme>")?;

    writeln!(out, "    <a:fmtScheme name=\"\"/>")?;
    writeln!(out, "  </a:themeElements>")?;
    writeln!(out, "</a:theme>")?;
    Ok(())
}

// accent3..6: use chart.series[2..] first (the designer's palette order),
// then fall back to unused roles. Extracted to keep write_theme_xml under the
// 100-line clippy limit and to separate the accent-assignment logic.
//
// WHY series-first: alphabetical role ordering (from toml BTreeMap) puts
// background colors (bg, bg_soft) early, which would assign white (#FFF) to
// accent3 and make chart series invisible on white slides. chart.series
// captures the designer's intended chart palette order.
fn write_accent_slots_3_to_6(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    let mut used: Vec<HexColor> = Vec::new();
    if let Some(c) = color_for(theme, &["tone:accent", "tone:positive"]) {
        used.push(c.clone());
    }
    if let Some(c) = color_for(theme, &["tone:before"]) {
        used.push(c.clone());
    }
    let mut accent_idx: u8 = 3;
    for series_ref in theme.chart.series.iter().skip(2) {
        if accent_idx > 6 {
            break;
        }
        if let Some(color) = theme.lookup_color(series_ref)
            && !used.iter().any(|c| c == color)
        {
            write_color_slot(out, &format!("accent{accent_idx}"), Some(color), "000000")?;
            used.push(color.clone());
            accent_idx += 1;
        }
    }
    for color in theme.role.values() {
        if accent_idx > 6 {
            break;
        }
        if used.iter().any(|c| c == color) {
            continue;
        }
        write_color_slot(out, &format!("accent{accent_idx}"), Some(color), "000000")?;
        used.push(color.clone());
        accent_idx += 1;
    }
    while accent_idx <= 6 {
        write_color_slot(out, &format!("accent{accent_idx}"), None, "000000")?;
        accent_idx += 1;
    }
    Ok(())
}

fn write_color_slot(
    out: &mut String,
    slot: &str,
    color: Option<&HexColor>,
    default_body: &str,
) -> std::fmt::Result {
    // `dk1` and `lt1` use `<a:sysClr val=... lastClr=.../>` in canonical
    // theme1 files; consumer parsers accept `<a:srgbClr/>` for all slots and
    // the rendered result is identical. We use srgbClr for every slot so the
    // emission is uniform and diffable.
    let body = color.map_or(default_body.to_owned(), |c| c.body().to_owned());
    writeln!(out, "      <a:{slot}>")?;
    writeln!(out, r#"        <a:srgbClr val="{body}"/>"#)?;
    writeln!(out, "      </a:{slot}>")?;
    Ok(())
}

fn color_for<'a>(theme: &'a ResolvedTheme, refs: &[&str]) -> Option<&'a HexColor> {
    for r in refs {
        let (namespace, name) = r.split_once(':')?;
        let hit = match namespace {
            "role" => theme.role.get(name),
            "tone" => theme.tone.get(name),
            "surface" => theme.surface.get(name),
            _ => None,
        };
        if let Some(c) = hit {
            return Some(c);
        }
    }
    None
}

fn primary_typeface(family: Option<&[String]>) -> String {
    family
        .and_then(|stack| stack.first().cloned())
        .unwrap_or_else(|| "Calibri".to_owned())
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
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

    #[test]
    fn ooxml_emits_well_formed_root() {
        let xml = emit_theme_xml(&summus()).expect("emit summus theme1.xml");
        assert!(xml.starts_with("<?xml"), "XML prolog missing");
        let expected_open = format!(r#"<a:theme xmlns:a="{OOXML_DRAWINGML_NS}""#);
        assert!(
            xml.contains(&expected_open),
            "drawingml namespace must appear: {xml}"
        );
        assert!(xml.contains("</a:theme>"), "closing tag missing");
    }

    #[test]
    fn ooxml_clrscheme_embeds_brand_values_without_hash() {
        let xml = emit_theme_xml(&summus()).expect("emit summus theme1.xml");
        assert!(
            xml.contains(r#"<a:srgbClr val="232E54"/>"#),
            "navy must appear hash-less in srgbClr: {xml}"
        );
        assert!(
            xml.contains(r#"<a:srgbClr val="318891"/>"#),
            "teal must appear hash-less in srgbClr: {xml}"
        );
    }

    #[test]
    fn ooxml_fontscheme_uses_summus_typefaces() {
        let xml = emit_theme_xml(&summus()).expect("emit summus theme1.xml");
        assert!(
            xml.contains(r#"<a:latin typeface="Geist"/>"#),
            "minor (body) typeface must be Geist: {xml}"
        );
        assert!(
            xml.contains(r#"<a:latin typeface="Newsreader"/>"#),
            "major (heading) typeface must be Newsreader: {xml}"
        );
    }

    #[test]
    fn ooxml_carries_theme_name_attribute() {
        let xml = emit_theme_xml(&summus()).expect("emit summus theme1.xml");
        assert!(
            xml.contains(r#"name="summus""#),
            "theme + scheme name attribute must be set"
        );
    }

    #[test]
    fn ooxml_byte_stable_across_runs() {
        let a = emit_theme_xml(&summus()).expect("first");
        let b = emit_theme_xml(&summus()).expect("second");
        assert_eq!(a, b, "two emissions must match byte-for-byte");
    }

    #[test]
    fn ooxml_accent3_uses_chart_series_not_bg() {
        // WHY: without the chart.series fix, alphabetical role ordering
        // would assign bg=#FFFFFF (white) to accent3, making chart series 3
        // invisible on white slide backgrounds. chart.series[2]=neutral=navy
        // must win.
        let xml = emit_theme_xml(&summus()).expect("emit summus theme1.xml");
        // accent3 slot must contain navy (232E54), not bg (FFFFFF)
        let accent3_pos = xml.find("<a:accent3>").expect("accent3 slot must exist");
        let accent3_block = xml
            .get(accent3_pos..accent3_pos + 80)
            .expect("accent3 block fits in xml");
        assert!(
            accent3_block.contains("232E54"),
            "accent3 must be navy (232E54) from chart.series[2]=neutral, not bg (FFFFFF): {accent3_block}"
        );
    }
}
