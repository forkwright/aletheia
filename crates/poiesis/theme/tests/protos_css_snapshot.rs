//! Byte-stable CSS snapshot test for the protos theme.
//!
//! Locks the `emit_css` output for `protos` against the committed fixture in
//! `tests/fixtures/protos.css`. This is the "reference-CSS byte parity" check
//! from B-002: once the fixture is committed, any unintentional CSS change
//! surfaces as a diff here rather than silently drifting downstream.
//!
//! To regenerate the fixture after an intentional change:
//! ```sh
//! UPDATE_SNAPSHOTS=1 cargo test -p poiesis-theme -- protos_css_snapshot
//! ```

#![expect(clippy::expect_used, reason = "test assertions")]

use std::path::PathBuf;

use poiesis_theme::registry::{Registry, parse_theme_id};
use poiesis_theme::sinks::emit_css;

fn protos_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes")
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("protos.css")
}

fn resolve_protos() -> poiesis_theme::ResolvedTheme {
    let registry = Registry::load_dir(&protos_dir()).expect("load themes/");
    let id = parse_theme_id("protos").expect("parse protos id");
    registry.resolve(&id).expect("resolve protos")
}

#[test]
fn protos_css_snapshot() {
    let css = emit_css(&resolve_protos()).expect("emit_css must not fail for protos");
    let fixture = fixture_path();

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::create_dir_all(fixture.parent().expect("fixture has parent"))
            .expect("create fixtures dir");
        std::fs::write(&fixture, &css).expect("write protos.css fixture");
        eprintln!("wrote fixture: {}", fixture.display());
        return;
    }

    let expected = std::fs::read_to_string(&fixture).unwrap_or_else(|_| {
        panic!(
            "tests/fixtures/protos.css not found — run once with UPDATE_SNAPSHOTS=1:\n  \
             UPDATE_SNAPSHOTS=1 cargo test -p poiesis-theme -- protos_css_snapshot"
        )
    });
    assert_eq!(
        css, expected,
        "protos CSS output changed vs committed fixture;\n\
         if intentional, regenerate with UPDATE_SNAPSHOTS=1"
    );
}
