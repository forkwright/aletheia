#![expect(
    clippy::expect_used,
    reason = "test assertions in feature-gated engine tests"
)]

use super::super::{KnowledgeConfig, KnowledgeStore};

/// Regression guard for #6358: `KnowledgeConfig::rule_dir` must actually wire
/// the on-disk `.mnm` rule into the running store's query engine, not merely
/// exist as an unused field. If the production wiring in
/// `KnowledgeStore::open_mem_with_config`/`open_fjall` regresses to calling
/// `krites::Db::attach_rule_store` conditionally-but-wrongly, or drops the
/// call entirely, this query fails because `wiring_probe` is never defined.
#[tokio::test]
async fn rule_dir_config_wires_hot_reloaded_rule_into_queries() {
    let dir = tempfile::tempdir().expect("create temp rule dir");
    tokio::fs::write(
        dir.path().join("wiring_probe.mnm"),
        "wiring_probe[marker] := marker = \"hot-reload-wired\"\n",
    )
    .await
    .expect("write probe rule file");

    let config = KnowledgeConfig {
        rule_dir: Some(dir.path().to_path_buf()),
        ..KnowledgeConfig::default()
    };
    let store =
        KnowledgeStore::open_mem_with_config(config).expect("open store with rule_dir configured");

    let result = store
        .run_query(
            "?[marker] := wiring_probe[marker]",
            std::collections::BTreeMap::new(),
        )
        .expect("query against the hot-reloaded rule should succeed");

    assert_eq!(
        result.get_string(0, "marker").as_deref(),
        Some("hot-reload-wired"),
        "the on-disk .mnm rule should be live and queryable via KnowledgeConfig::rule_dir alone"
    );
}

/// Counterpart to the guard above: without `rule_dir` set, no rule store is
/// attached, so a query against an undeclared rule name fails as expected.
/// Pins the "off by default" half of the contract.
#[test]
fn without_rule_dir_no_rule_store_is_attached() {
    let store = KnowledgeStore::open_mem().expect("open store with default config");

    let outcome = store.run_query(
        "?[marker] := wiring_probe[marker]",
        std::collections::BTreeMap::new(),
    );

    assert!(
        outcome.is_err(),
        "no rule store is attached without rule_dir, so an undeclared rule name must fail"
    );
}
