use std::fmt::Write;

use indexmap::IndexMap;
use serde_json::{Map, Value, json};
use snafu::{IntoError, ResultExt};

use crate::error::{SinkSnafu, ThemeError};
use crate::resolved::ResolvedTheme;

/// Emit the theme as a flat doc-vars JSON object, suitable for piping into
/// Pandoc as `-M name=value` or for embedding in a `reference.docx`/`reference.odt`
/// generation step.
///
/// Key naming follows the same convention as the CSS sink (`color.<role>`,
/// `tone.<name>`, `surface.<name>`, `type.<role>.<slot>`, `space.<name>`,
/// `grid.<slot>`, `table.<slot>`, `chart.series.<n>`), so a downstream
/// template can reference the same logical name regardless of which sink it
/// reads from.
///
/// The output is deterministic: every map is emitted in declaration order.
/// Numeric values become JSON numbers (`64`, not `"64"`); colors become
/// uppercased `#RRGGBB` strings; family stacks become arrays of strings.
///
/// # Errors
///
/// Returns [`ThemeError::Sink`] only if JSON serialization fails (structurally
/// impossible for the value shapes this function produces — the variant
/// exists for API symmetry with the other sinks).
pub fn emit_docvars_json(theme: &ResolvedTheme) -> Result<String, ThemeError> {
    let value = build_docvars_value(theme);
    // WHY: pretty-printing a serde_json::Value into a String allocates and
    // formats — the failure modes (out-of-memory, integer overflow in the
    // formatter) are not surfaceable as ThemeError. Treat a serialization
    // failure as a sink emission error rather than panicking.
    serde_json::to_string_pretty(&value).map_err(|_err| {
        SinkSnafu {
            sink: "docvars".to_owned(),
        }
        .into_error(std::fmt::Error)
    })
}

/// Emit the doc-vars map as a flat YAML metadata block. The shape matches the
/// JSON sink; the format is what Pandoc's `--metadata-file` expects.
///
/// # Errors
///
/// Returns [`ThemeError::Sink`] if `std::fmt::Write` fails (unreachable for
/// `String`).
pub fn emit_docvars_yaml(theme: &ResolvedTheme) -> Result<String, ThemeError> {
    let mut out = String::new();
    write_yaml(&mut out, theme).context(SinkSnafu {
        sink: "docvars".to_owned(),
    })?;
    Ok(out)
}

fn build_docvars_value(theme: &ResolvedTheme) -> Value {
    let mut root = Map::new();
    root.insert("theme".to_owned(), Value::String(theme.id.to_string()));
    if let Some(title) = &theme.title {
        root.insert("theme_title".to_owned(), Value::String(title.clone()));
    }

    let mut color_obj = Map::new();
    let mut role_obj = Map::new();
    for (name, hex) in &theme.role {
        role_obj.insert(name.clone(), Value::String(hex.to_string()));
    }
    color_obj.insert("role".to_owned(), Value::Object(role_obj));
    let mut tone_obj = Map::new();
    for (name, hex) in &theme.tone {
        tone_obj.insert(name.clone(), Value::String(hex.to_string()));
    }
    color_obj.insert("tone".to_owned(), Value::Object(tone_obj));
    let mut surface_obj = Map::new();
    for (name, hex) in &theme.surface {
        surface_obj.insert(name.clone(), Value::String(hex.to_string()));
    }
    color_obj.insert("surface".to_owned(), Value::Object(surface_obj));
    root.insert("color".to_owned(), Value::Object(color_obj));

    let mut type_obj = Map::new();
    let mut family_obj = Map::new();
    for (name, stack) in &theme.r#type.family {
        let arr: Vec<Value> = stack.iter().map(|s| Value::String(s.clone())).collect();
        family_obj.insert(name.clone(), Value::Array(arr));
    }
    type_obj.insert("family".to_owned(), Value::Object(family_obj));
    let mut scale_obj = Map::new();
    for (name, px) in &theme.r#type.scale {
        scale_obj.insert(name.clone(), json!(*px));
    }
    type_obj.insert("scale".to_owned(), Value::Object(scale_obj));
    let mut role_typ_obj = Map::new();
    for (name, role) in &theme.r#type.role {
        let mut slot = Map::new();
        if let Some(family) = &role.family {
            slot.insert("family".to_owned(), Value::String(family.clone()));
        }
        if let Some(weight) = role.weight {
            slot.insert("weight".to_owned(), json!(weight));
        }
        if let Some(size) = &role.size {
            slot.insert("size".to_owned(), Value::String(size.clone()));
        }
        if let Some(tracking) = role.tracking {
            slot.insert("tracking".to_owned(), json!(tracking));
        }
        if let Some(leading) = role.leading {
            slot.insert("leading".to_owned(), json!(leading));
        }
        if let Some(color) = &role.color {
            slot.insert("color".to_owned(), Value::String(color.clone()));
        }
        role_typ_obj.insert(name.clone(), Value::Object(slot));
    }
    type_obj.insert("role".to_owned(), Value::Object(role_typ_obj));
    root.insert("type".to_owned(), Value::Object(type_obj));

    if !theme.space.slots.is_empty() {
        let mut space_obj = Map::new();
        for (name, px) in &theme.space.slots {
            space_obj.insert(name.clone(), json!(*px));
        }
        root.insert("space".to_owned(), Value::Object(space_obj));
    }

    let mut grid_obj = Map::new();
    if let Some([w, h]) = theme.grid.base_canvas {
        grid_obj.insert("canvas_w".to_owned(), json!(w));
        grid_obj.insert("canvas_h".to_owned(), json!(h));
    }
    if let Some(columns) = theme.grid.columns {
        grid_obj.insert("columns".to_owned(), json!(columns));
    }
    if let Some(gutter) = theme.grid.gutter {
        grid_obj.insert("gutter".to_owned(), json!(gutter));
    }
    if let Some(margin) = theme.grid.margin {
        grid_obj.insert("margin".to_owned(), json!(margin));
    }
    if !grid_obj.is_empty() {
        root.insert("grid".to_owned(), Value::Object(grid_obj));
    }

    let mut chart_series: IndexMap<String, String> = IndexMap::new();
    for (i, name) in theme.chart.series.iter().enumerate() {
        chart_series.insert(format!("series_{}", i + 1), name.clone());
    }
    if !chart_series.is_empty() {
        let mut chart_obj = Map::new();
        for (k, v) in chart_series {
            chart_obj.insert(k, Value::String(v));
        }
        root.insert("chart".to_owned(), Value::Object(chart_obj));
    }

    Value::Object(root)
}

fn write_yaml(out: &mut String, theme: &ResolvedTheme) -> std::fmt::Result {
    writeln!(out, "---")?;
    writeln!(out, "theme: {}", theme.id)?;
    if let Some(title) = &theme.title {
        writeln!(out, "theme_title: {}", yaml_str(title))?;
    }
    writeln!(out, "color:")?;
    writeln!(out, "  role:")?;
    for (name, hex) in &theme.role {
        writeln!(out, "    {name}: \"{}\"", hex.as_str())?;
    }
    writeln!(out, "  tone:")?;
    for (name, hex) in &theme.tone {
        writeln!(out, "    {name}: \"{}\"", hex.as_str())?;
    }
    writeln!(out, "  surface:")?;
    for (name, hex) in &theme.surface {
        writeln!(out, "    {name}: \"{}\"", hex.as_str())?;
    }
    writeln!(out, "type:")?;
    writeln!(out, "  family:")?;
    for (name, stack) in &theme.r#type.family {
        write!(out, "    {name}: [")?;
        for (i, s) in stack.iter().enumerate() {
            if i > 0 {
                write!(out, ", ")?;
            }
            write!(out, "{}", yaml_str(s))?;
        }
        writeln!(out, "]")?;
    }
    writeln!(out, "  scale:")?;
    for (name, px) in &theme.r#type.scale {
        writeln!(out, "    {name}: {px}")?;
    }
    writeln!(out, "---")?;
    Ok(())
}

fn yaml_str(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| ":[]{},\"\n".contains(c)) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions; serde_json::Value index op is the canonical accessor"
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

    #[test]
    fn json_contains_theme_id() {
        let json = emit_docvars_json(&summus()).expect("emit json");
        assert!(json.contains("\"theme\": \"summus\""));
    }

    #[test]
    fn json_carries_resolved_tone_hex() {
        let json = emit_docvars_json(&summus()).expect("emit json");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let positive = &parsed["color"]["tone"]["positive"];
        assert_eq!(positive, "#318891", "positive tone must carry teal hex");
    }

    #[test]
    fn json_byte_stable_across_runs() {
        let a = emit_docvars_json(&summus()).expect("first");
        let b = emit_docvars_json(&summus()).expect("second");
        assert_eq!(a, b, "two emissions must match byte-for-byte");
    }

    #[test]
    fn yaml_emits_resolved_hex_for_tone() {
        let yaml = emit_docvars_yaml(&summus()).expect("emit yaml");
        assert!(
            yaml.contains("positive: \"#318891\""),
            "positive tone must appear in yaml with the teal hex: {yaml}"
        );
    }

    #[test]
    fn yaml_starts_and_ends_with_doc_markers() {
        let yaml = emit_docvars_yaml(&summus()).expect("emit yaml");
        assert!(
            yaml.starts_with("---\n"),
            "yaml must start with document marker"
        );
        assert!(
            yaml.trim_end().ends_with("---"),
            "yaml must end with document marker"
        );
    }
}
