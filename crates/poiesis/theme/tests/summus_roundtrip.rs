//! End-to-end check: discover the shipped `summus.toml`, resolve it, and
//! verify every sink emits its expected anchor values. This is the
//! integration twin of the per-module unit tests; it exists so a regression
//! in any one sink fails at the crate boundary with a single named test
//! rather than only inside the affected module.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions; serde_json::Value indexing is the canonical accessor"
)]

use std::path::PathBuf;

use poiesis_theme::registry::{Registry, parse_theme_id};
use poiesis_theme::sinks::{emit_css, emit_docvars_json, emit_docvars_yaml, emit_theme_xml};
use poiesis_theme::tokens::HexColor;

fn summus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes")
}

#[test]
fn summus_round_trip_loads_resolves_and_emits_all_sinks() {
    let registry = Registry::load_dir(&summus_dir()).expect("load themes/");
    let id = parse_theme_id("summus").expect("parse summus id");
    let resolved = registry.resolve(&id).expect("resolve summus");

    // ── token correctness ───────────────────────────────────────────────────
    assert_eq!(
        resolved.role.get("navy").map(HexColor::as_str),
        Some("#232E54"),
        "summus navy role hex must match spec 03 seed"
    );
    assert_eq!(
        resolved.tone.get("positive").map(HexColor::as_str),
        Some("#318891"),
        "positive tone must resolve to teal"
    );
    assert_eq!(
        resolved.surface.get("page").map(HexColor::as_str),
        Some("#FFFFFF"),
        "page surface must resolve to bg white"
    );

    // ── CSS sink ────────────────────────────────────────────────────────────
    let css = emit_css(&resolved).expect("css sink");
    for needle in [
        "--color-navy: #232E54;",
        "--color-teal: #318891;",
        "--tone-positive: var(--color-teal);",
        "--surface-page: #FFFFFF;",
        "--scale-title: 64px;",
    ] {
        assert!(
            css.contains(needle),
            "css must contain `{needle}`; got:\n{css}"
        );
    }

    // ── OOXML sink ─────────────────────────────────────────────────────────
    let xml = emit_theme_xml(&resolved).expect("ooxml sink");
    for needle in [
        r#"<a:srgbClr val="232E54"/>"#,
        r#"<a:srgbClr val="318891"/>"#,
        r#"<a:latin typeface="Geist"/>"#,
        r#"<a:latin typeface="Newsreader"/>"#,
        r#"name="summus""#,
    ] {
        assert!(
            xml.contains(needle),
            "ooxml must contain `{needle}`; got:\n{xml}"
        );
    }

    // ── doc-vars sink ──────────────────────────────────────────────────────
    let json = emit_docvars_json(&resolved).expect("docvars json sink");
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("docvars json must parse back");
    assert_eq!(parsed["theme"].as_str(), Some("summus"));
    assert_eq!(parsed["color"]["role"]["navy"].as_str(), Some("#232E54"));
    assert_eq!(
        parsed["color"]["tone"]["positive"].as_str(),
        Some("#318891")
    );
    assert_eq!(parsed["type"]["scale"]["title"].as_u64(), Some(64));

    let yaml = emit_docvars_yaml(&resolved).expect("docvars yaml sink");
    assert!(yaml.contains("theme: summus"), "yaml must carry theme id");
    assert!(
        yaml.contains("positive: \"#318891\""),
        "yaml must carry resolved positive tone"
    );
}

#[test]
fn summus_swap_target_resolution_is_independent_of_emit_order() {
    // Resolve twice in a row and confirm byte-equal output across all
    // sinks. This catches accidental non-determinism (hashmap iteration,
    // timestamp injection, …) at the integration boundary so a downstream
    // brand fingerprinting strategy can rely on stable bytes.
    let registry = Registry::load_dir(&summus_dir()).expect("load themes/");
    let id = parse_theme_id("summus").expect("parse summus id");
    let first = registry.resolve(&id).expect("resolve summus first");
    let second = registry.resolve(&id).expect("resolve summus second");

    assert_eq!(
        emit_css(&first).expect("css 1"),
        emit_css(&second).expect("css 2")
    );
    assert_eq!(
        emit_theme_xml(&first).expect("ooxml 1"),
        emit_theme_xml(&second).expect("ooxml 2")
    );
    assert_eq!(
        emit_docvars_json(&first).expect("json 1"),
        emit_docvars_json(&second).expect("json 2")
    );
    assert_eq!(
        emit_docvars_yaml(&first).expect("yaml 1"),
        emit_docvars_yaml(&second).expect("yaml 2")
    );
}
