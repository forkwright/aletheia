//! Integration test: build index against the aletheia workspace, then run
//! real queries.
//!
//! The full-workspace tests below are ignored because they run `cargo metadata`
//! and parse the full workspace — expected wall time is 3-8 seconds on a
//! typical workstation. Run one with:
//!
//! ```text
//! cargo nextest run -p gnosis --features test-core -- --include-ignored symbol_rdeps_finds_many_callers
//! ```
//!
//! Fast synthetic-workspace variants of the ignored tests live in
//! [`synthetic_workspace_integration`] and run on every `test-core` test pass.

#[cfg(feature = "test-core")]
#[expect(clippy::expect_used, reason = "integration test assertions")]
mod workspace_integration {
    use std::path::PathBuf;

    use gnosis::CodeGraph;

    /// Resolve the workspace root by walking up from the manifest directory.
    fn workspace_root() -> PathBuf {
        let manifest =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
        // WHY: the gnosis manifest sits at `crates/gnosis/`, so the workspace
        // root is two levels up.
        PathBuf::from(manifest)
            .parent()
            .expect("crates/")
            .parent()
            .expect("workspace root")
            .to_owned()
    }

    /// Build the code-graph index against the live aletheia workspace, then
    /// assert that `symbol_rdeps("Message", None)` returns at least 1 result
    /// (impl or reexport edge).
    ///
    /// WHY: In gnosis v1 only `impl` and `reexport` edge kinds are indexed
    /// (call sites and type-use references require name resolution and are
    /// deferred to v2).  `hermeneus::types::Message` is a core LLM message
    /// type that has at least one re-export site in the workspace, so this
    /// lower bound validates that the index is actually populated and queries
    /// execute without error.
    #[test]
    #[ignore = "parses full workspace (3-8s) — see #5615"]
    fn symbol_rdeps_finds_many_callers() {
        let root = workspace_root();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("gnosis_integration.fjall");

        let graph = CodeGraph::open(&db_path, &root).expect("open graph");
        graph.rebuild().expect("rebuild");

        let rows = graph.symbol_rdeps("Message", None).expect("symbol_rdeps");

        for row in &rows {
            assert!(
                row.source.starts_with("gnosis@"),
                "every row must carry gnosis provenance, got: {:?}",
                row.source
            );
        }

        let all_syms = graph
            .symbols_in("hermeneus", None)
            .expect("symbols_in hermeneus");
        assert!(
            !all_syms.is_empty(),
            "hermeneus should have indexed symbols; got 0 — index may not have rebuilt"
        );
    }

    /// Verify that `impl_search("Stamped")` returns results (there should be
    /// multiple impls of the `Stamped` trait in the workspace).
    #[test]
    #[ignore = "parses full workspace (3-8s) — see #5615"]
    fn impl_search_finds_stamped_impls() {
        let root = workspace_root();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("gnosis_stamped.fjall");

        let graph = CodeGraph::open(&db_path, &root).expect("open graph");
        graph.rebuild().expect("rebuild");

        let rows = graph.impl_search("Stamped").expect("impl_search");
        assert!(
            !rows.is_empty(),
            "expected at least one impl Stamped in the workspace"
        );
    }

    /// Verify that `crate_rdeps("eidos")` returns multiple crates (nearly
    /// everything depends on eidos).
    #[test]
    #[ignore = "parses full workspace (3-8s) — see #5615"]
    fn crate_rdeps_eidos_returns_many() {
        let root = workspace_root();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("gnosis_eidos.fjall");

        let graph = CodeGraph::open(&db_path, &root).expect("open graph");
        graph.rebuild().expect("rebuild");

        let rows = graph.crate_rdeps("eidos").expect("crate_rdeps");
        assert!(
            rows.len() >= 5,
            "expected ≥5 crates depending on eidos, got {}",
            rows.len()
        );
    }
}

#[cfg(feature = "test-core")]
#[expect(clippy::expect_used, reason = "integration test assertions")]
mod synthetic_workspace_integration {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use gnosis::CodeGraph;

    /// Create a tiny Cargo workspace in a temp directory and return both the
    /// temp directory (so it stays alive) and the workspace root path.
    ///
    /// The workspace contains three crates:
    /// - `fixturecore` defines the `Message` trait and the `Stamped` trait.
    /// - `fixtureconsumer` re-exports `Message` and implements `Stamped` for
    ///   `Consumer`.
    /// - `fixtureobserver` implements `Stamped` for `Observer`.
    ///
    /// WHY: This gives the ignored full-workspace tests a fast, deterministic
    /// counterpart that still exercises `rebuild()` → `cargo metadata` → syn AST
    /// walk → fjall persistence → the real query methods.
    fn synthetic_workspace_root() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_owned();

        write_file(&root.join("Cargo.toml"), CARGO_TOML);

        write_crate(&root, "fixturecore", FIXTURECORE_TOML, FIXTURECORE_LIB);
        write_crate(
            &root,
            "fixtureconsumer",
            FIXTURECONSUMER_TOML,
            FIXTURECONSUMER_LIB,
        );
        write_crate(
            &root,
            "fixtureobserver",
            FIXTUREOBSERVER_TOML,
            FIXTUREOBSERVER_LIB,
        );

        (tmp, root)
    }

    fn write_crate(root: &Path, dir_name: &str, manifest: &str, lib: &str) {
        let crate_root = root.join(dir_name);
        let src_dir = crate_root.join("src");
        fs::create_dir_all(&src_dir).expect("create src dir");
        write_file(&crate_root.join("Cargo.toml"), manifest);
        write_file(&src_dir.join("lib.rs"), lib);
    }

    fn write_file(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write fixture file");
    }

    const CARGO_TOML: &str = r#"[workspace]
members = ["fixturecore", "fixtureconsumer", "fixtureobserver"]
resolver = "2"
"#;

    const FIXTURECORE_TOML: &str = r#"[package]
name = "fixturecore"
version = "0.1.0"
edition = "2021"
"#;

    const FIXTURECORE_LIB: &str = r#"pub trait Message {}
pub trait Stamped {}

pub fn public_fn() {}
pub(crate) fn crate_fn() {}
fn private_fn() {}

pub struct PublicStruct;
pub(crate) struct CrateStruct;
struct PrivateStruct;

impl PublicStruct {
    pub fn public_method(&self) {}
    pub(crate) fn crate_method(&self) {}
    fn private_method(&self) {}
}

pub enum PublicEnum {
    Variant,
}
pub(crate) enum CrateEnum {
    Variant,
}
enum PrivateEnum {
    Variant,
}

pub trait PublicTrait {
    fn public_trait_method(&self);
}
pub(crate) trait CrateTrait {
    fn crate_trait_method(&self);
}
trait PrivateTrait {
    fn private_trait_method(&self);
}

pub type PublicAlias = PublicStruct;
pub(crate) type CrateAlias = CrateStruct;
type PrivateAlias = PrivateStruct;

pub const PUBLIC_CONST: usize = 1;
pub(crate) const CRATE_CONST: usize = 2;
const PRIVATE_CONST: usize = 3;
"#;

    const FIXTURECONSUMER_TOML: &str = r#"[package]
name = "fixtureconsumer"
version = "0.1.0"
edition = "2021"

[dependencies]
fixturecore = { path = "../fixturecore" }
"#;

    const FIXTURECONSUMER_LIB: &str = r#"pub use fixturecore::Message;

pub struct Consumer;

impl fixturecore::Stamped for Consumer {}
"#;

    const FIXTUREOBSERVER_TOML: &str = r#"[package]
name = "fixtureobserver"
version = "0.1.0"
edition = "2021"

[dependencies]
fixturecore = { path = "../fixturecore" }
"#;

    const FIXTUREOBSERVER_LIB: &str = r#"pub struct Observer;

impl fixturecore::Stamped for Observer {}
"#;

    /// Fast counterpart to `workspace_integration::symbol_rdeps_finds_many_callers`.
    #[test]
    fn symbol_rdeps_finds_reexport_in_synthetic_workspace() {
        let (_tmp, root) = synthetic_workspace_root();
        let db_tmp = tempfile::tempdir().expect("tempdir");
        let db_path = db_tmp.path().join("gnosis_synthetic.fjall");

        let graph = CodeGraph::open(&db_path, &root).expect("open graph");
        graph.rebuild().expect("rebuild");

        let rows = graph.symbol_rdeps("Message", None).expect("symbol_rdeps");
        assert!(
            !rows.is_empty(),
            "expected at least one impl or reexport edge to Message"
        );
        for row in &rows {
            assert!(
                row.source.starts_with("gnosis@"),
                "every row must carry gnosis provenance, got: {:?}",
                row.source
            );
        }

        // The `pub use fixturecore::Message` in fixtureconsumer targets fixturecore.
        let filtered = graph
            .symbol_rdeps("Message", Some("fixturecore"))
            .expect("symbol_rdeps filtered");
        assert!(
            !filtered.is_empty(),
            "expected at least one ref whose target crate is fixturecore"
        );
    }

    /// Fast counterpart to `workspace_integration::impl_search_finds_stamped_impls`.
    #[test]
    fn impl_search_finds_synthetic_stamped_impls() {
        let (_tmp, root) = synthetic_workspace_root();
        let db_tmp = tempfile::tempdir().expect("tempdir");
        let db_path = db_tmp.path().join("gnosis_synthetic_stamped.fjall");

        let graph = CodeGraph::open(&db_path, &root).expect("open graph");
        graph.rebuild().expect("rebuild");

        let rows = graph.impl_search("Stamped").expect("impl_search");
        let names: Vec<_> = rows
            .iter()
            .filter_map(|row| row.symbol_name.as_deref())
            .collect();
        assert!(
            names.contains(&"Consumer"),
            "expected Consumer to implement Stamped; got: {:?}",
            names
        );
        assert!(
            names.contains(&"Observer"),
            "expected Observer to implement Stamped; got: {:?}",
            names
        );
    }

    /// Fast counterpart to `workspace_integration::crate_rdeps_eidos_returns_many`.
    #[test]
    fn crate_rdeps_synthetic_workspace_returns_dependents() {
        let (_tmp, root) = synthetic_workspace_root();
        let db_tmp = tempfile::tempdir().expect("tempdir");
        let db_path = db_tmp.path().join("gnosis_synthetic_rdeps.fjall");

        let graph = CodeGraph::open(&db_path, &root).expect("open graph");
        graph.rebuild().expect("rebuild");

        let rows = graph.crate_rdeps("fixturecore").expect("crate_rdeps");
        let names: Vec<_> = rows
            .iter()
            .filter_map(|row| row.crate_name.as_deref())
            .collect();
        assert!(
            names.contains(&"fixtureconsumer"),
            "expected fixtureconsumer to depend on fixturecore; got: {:?}",
            names
        );
        assert!(
            names.contains(&"fixtureobserver"),
            "expected fixtureobserver to depend on fixturecore; got: {:?}",
            names
        );
    }

    #[test]
    fn symbols_in_filters_private_definitions_in_synthetic_workspace() {
        let (_tmp, root) = synthetic_workspace_root();
        let db_tmp = tempfile::tempdir().expect("tempdir");
        let db_path = db_tmp.path().join("gnosis_synthetic_symbols.fjall");

        let graph = CodeGraph::open(&db_path, &root).expect("open graph");
        graph.rebuild().expect("rebuild");

        let rows = graph
            .symbols_in("fixturecore", None)
            .expect("symbols_in fixturecore");
        let names: BTreeSet<String> = rows
            .iter()
            .filter_map(|row| row.symbol_name.clone())
            .collect();

        for expected in [
            "Message",
            "Stamped",
            "public_fn",
            "crate_fn",
            "PublicStruct",
            "CrateStruct",
            "public_method",
            "crate_method",
            "PublicEnum",
            "CrateEnum",
            "PublicTrait",
            "CrateTrait",
            "public_trait_method",
            "crate_trait_method",
            "PublicAlias",
            "CrateAlias",
            "PUBLIC_CONST",
            "CRATE_CONST",
        ] {
            assert!(
                names.contains(expected),
                "expected public-ish symbol {expected}; got: {names:?}"
            );
        }

        for private in [
            "private_fn",
            "PrivateStruct",
            "private_method",
            "PrivateEnum",
            "PrivateTrait",
            "private_trait_method",
            "PrivateAlias",
            "PRIVATE_CONST",
        ] {
            assert!(
                !names.contains(private),
                "private symbol {private} must not be indexed; got: {names:?}"
            );
        }
    }
}
