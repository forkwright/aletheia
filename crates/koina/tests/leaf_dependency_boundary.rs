//! Manifest-level guard for koina's leaf/foundation contract.
//!
//! WHY (#5577): koina is the workspace's foundation crate — every other crate
//! depends on it, so anything in koina's manifest lands in everything. Its own
//! `clippy.toml` says HTTP clients belong in hermeneus or pylon, but that guard
//! is a `disallowed-types` list: it only fires when a banned type is *named* in
//! koina's source. koina declared `reqwest` purely to reach `reqwest::Url` — a
//! re-export of `url::Url` — which named nothing on the banned list and so
//! pushed the whole reqwest/hyper/rustls stack onto every dependent crate
//! unnoticed.
//!
//! The type-level guard cannot see a dependency that is declared but never
//! named. This test closes that gap by asserting against the manifest itself.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;

/// Crates that pull an HTTP client runtime and must not appear in koina's
/// dependency table. URL parsing belongs to `url`; HTTP belongs to hermeneus
/// or pylon.
const FORBIDDEN_DEPENDENCIES: &[&str] = &["reqwest", "hyper", "axum", "tower-http", "rusqlite"];

#[test]
fn koina_manifest_declares_no_http_stack() {
    let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let manifest: toml::Table = fs::read_to_string(manifest_path)
        .expect("koina Cargo.toml is readable")
        .parse()
        .expect("koina Cargo.toml parses as TOML");

    let mut offenders = Vec::new();
    for table in ["dependencies", "build-dependencies", "dev-dependencies"] {
        let Some(deps) = manifest.get(table).and_then(toml::Value::as_table) else {
            continue;
        };
        for forbidden in FORBIDDEN_DEPENDENCIES {
            if deps.contains_key(*forbidden) {
                offenders.push(format!("{table}.{forbidden}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "koina is the workspace foundation crate: every dependency here reaches \
         every other crate. Remove {offenders:?} and use a leaf-appropriate \
         alternative (url::Url for URL parsing; HTTP belongs in hermeneus/pylon)."
    );
}
