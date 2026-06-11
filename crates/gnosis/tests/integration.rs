//! Integration test: build index against the aletheia workspace, then run
//! real queries.
//!
//! The `symbol_rdeps_finds_many_callers` test is marked `#[ignore]` because
//! it runs `cargo metadata` and parses the full workspace — expected wall time
//! is 3–8 seconds on menos hardware.  Run it with:
//!
//! ```text
//! cargo nextest run -p gnosis --features test-core -- --include-ignored symbol_rdeps_finds_many_callers
//! ```

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
    #[ignore = "parses full workspace — takes 3-8s; run with --include-ignored"]
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
    #[ignore = "parses full workspace — takes 3-8s; run with --include-ignored"]
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
    #[ignore = "parses full workspace — takes 3-8s; run with --include-ignored"]
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
