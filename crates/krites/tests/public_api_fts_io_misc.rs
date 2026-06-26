#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions - indexing is safe with known test data"
)]
#![expect(clippy::doc_markdown, reason = "test documentation")]
#![expect(clippy::approx_constant, reason = "test values")]
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::time::Duration;

use krites::{DataValue, Db, NamedRows, ScriptMutability};
use serde_json::json;

/// Test FTS index creation, text search, and index lifecycle.
#[test]
fn fts_index_lifecycle() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    db.run(
        ":create documents {id: String => content: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating documents relation should succeed");

    db.run(
        r#"?[id, content] <- [["doc1", "The quick brown fox jumps over the lazy dog"],
                            ["doc2", "A quick brown dog outpaces the lazy fox"],
                            ["doc3", "Slow and steady wins the race"]]
          :put documents {id => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting documents should succeed");

    db.run(
        "::fts create documents:fts_idx {\n\
         extractor: content,\n\
         tokenizer: Simple,\n\
         filters: [Lowercase]\n\
         }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating FTS index should succeed");

    let result = db
        .run_read_only(
            "?[id, content, score] := ~documents:fts_idx{id, content | query: \"quick\", k: 10, bind_score: score}",
            BTreeMap::new(),
        )
        .expect("FTS search should succeed");
    assert_eq!(
        result.rows.len(),
        2,
        "should find documents containing 'quick'"
    );

    // NOTE: FTS search requires the k parameter.
    let result = db
        .run_read_only(
            "?[id, content, score] := ~documents:fts_idx{id, content | query: \"fox\", k: 10, bind_score: score}",
            BTreeMap::new(),
        )
        .expect("FTS search for 'fox' should succeed");
    assert_eq!(
        result.rows.len(),
        2,
        "should find documents containing 'fox'"
    );

    // NOTE: documents inserted after index creation are auto-indexed.
    db.run(
        r#"?[id, content] <- [["doc4", "The quick red fox is very fast"]] :put documents {id => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting additional document should succeed");

    let result = db
        .run_read_only(
            "?[id, content] := ~documents:fts_idx{id, content | query: \"red\", k: 10}",
            BTreeMap::new(),
        )
        .expect("FTS search for 'red' should succeed");
    assert_eq!(
        result.rows.len(),
        1,
        "should find document containing 'red'"
    );

    db.run(
        "::fts drop documents:fts_idx",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("dropping FTS index should succeed");
}

/// Multi-term NEAR must chain left-to-right: each term must be proximate to
/// the immediately preceding term, not just to the first term.
#[test]
fn fts_near_chains_left_to_right_across_three_terms() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    db.run(
        ":create documents {id: String => content: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating documents relation should succeed");

    db.run(
        r#"?[id, content] <- [["match", "fox quick brown"],
                            ["false_positive", "fox quick filler filler fox brown"]]
          :put documents {id => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting documents should succeed");

    db.run(
        "::fts create documents:fts_idx {\n\
         extractor: content,\n\
         tokenizer: Simple,\n\
         filters: [Lowercase]\n\
         }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating FTS index should succeed");

    let result = db
        .run_read_only(
            "?[id] := ~documents:fts_idx{id, content | query: \"NEAR/2(fox quick brown)\", k: 10}",
            BTreeMap::new(),
        )
        .expect("NEAR query should succeed");

    let ids: Vec<_> = result
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(|v| v.get_str()).map(str::to_owned))
        .collect();
    assert_eq!(
        ids,
        vec!["match"],
        "three-term NEAR must require proximity between adjacent terms, not just to the first term"
    );
}

/// Test exporting and importing relations.
#[test]
fn export_import_relations() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    db.run(
        ":create source {id: Int => value: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating source relation should succeed");

    db.run(
        r#"?[id, value] <- [[1, "a"], [2, "b"], [3, "c"]] :put source {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting data should succeed");

    let exported = db
        .export_relations(["source"].iter())
        .expect("exporting relation should succeed");

    assert!(exported.contains_key("source"));
    assert_eq!(exported["source"].rows.len(), 3);

    let db2 = Db::open_mem().expect("opening second database should succeed");

    db2.run(
        ":create source {id: Int => value: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating source relation in db2 should succeed");

    db2.import_relations(exported)
        .expect("importing relations should succeed");

    let result = db2
        .run_read_only("?[id, value] := *source{id, value}", BTreeMap::new())
        .expect("querying imported data should succeed");
    assert_eq!(result.rows.len(), 3);
}

/// Test NamedRows JSON serialization roundtrip.
#[test]
fn named_rows_json_roundtrip() {
    let original = NamedRows::new(
        vec!["id".to_string(), "name".to_string()],
        vec![
            vec![DataValue::from(1i64), DataValue::from("Alice")],
            vec![DataValue::from(2i64), DataValue::from("Bob")],
        ],
    );

    let json = original.into_json();
    assert_eq!(json["headers"], json!(["id", "name"]));
    assert_eq!(json["rows"], json!([[1, "Alice"], [2, "Bob"]]));
}

/// Test registering callbacks and receiving change notifications.
#[test]
fn callback_receives_changes() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    let (_callback_id, receiver) = db.register_callback("changes_test", 10);

    db.run(
        ":create changes_test {id: Int => value: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    db.run(
        r#"?[id, value] <- [[1, "first"], [2, "second"]] :put changes_test {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting should succeed");

    db.run(
        r#"?[id, value] <- [[1, "updated"]] :put changes_test {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("updating should succeed");

    // WHY: callback delivery is asynchronous; allow time before draining.
    std::thread::sleep(Duration::from_millis(50));

    let mut callbacks = Vec::new();
    while let Ok(cb) = receiver.try_recv() {
        callbacks.push(cb);
    }

    assert!(!callbacks.is_empty(), "should have received callbacks");
}

/// Test query cache statistics tracking.
#[test]
fn query_cache_tracks_hits_and_misses() {
    let db = Db::open_mem()
        .expect("opening database should succeed")
        .with_cache(NonZeroUsize::new(100).expect("non-zero cache size"));

    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 0);

    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);

    let _ = db.run_read_only("?[x] := x = 2", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 1);
}

/// Test that cache normalizes whitespace.
#[test]
fn query_cache_normalizes_whitespace() {
    let db = Db::open_mem()
        .expect("opening database should succeed")
        .with_cache(NonZeroUsize::new(100).expect("non-zero cache size"));

    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.misses, 1);

    let _ = db.run_read_only("  ?[x]   :=  x  =  1  ", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.hits, 1);
}

/// Test error variants for malformed queries.
#[test]
fn malformed_query_errors() {
    let db = Db::open_mem().expect("opening database should succeed");

    let result = db.run_read_only("?[x] :=", BTreeMap::new());
    assert!(result.is_err(), "incomplete query should error");

    let result = db.run_read_only("?[x] := *nonexistent{x}", BTreeMap::new());
    assert!(result.is_err(), "query on unknown relation should error");

    db.run(
        ":create typed {n: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");
    let result = db.run_read_only(r#"?[n] := *typed{n: "string"}"#, BTreeMap::new());
    assert!(result.is_err(), "type mismatch in query should error");
}

/// Test error on invalid relation operations.
#[test]
fn invalid_relation_operations() {
    let db = Db::open_mem().expect("opening database should succeed");

    db.run(
        ":create existing {id: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    let result = db.run(
        ":create existing {id: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(result.is_err(), "creating duplicate relation should fail");

    let result = db.run(
        "::remove nonexistent_relation",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(
        result.is_err(),
        "dropping non-existent relation should fail"
    );
}

/// Test PageRank graph algorithm via fixed rules.
#[test]
#[cfg(feature = "graph-algo")]
fn pagerank_graph_algorithm() {
    let db = Db::open_mem().expect("opening database should succeed");

    let result = db
        .run_read_only(
            r"
            edges[] <- [[1, 2], [2, 3], [3, 4], [4, 1]]
            ?[node, score] <~ PageRank(edges[_, _])
            ",
            BTreeMap::new(),
        )
        .expect("PageRank query should succeed");

    assert!(!result.rows.is_empty(), "PageRank should return results");
    assert_eq!(result.rows.len(), 4, "PageRank should return 4 nodes");
}
/// Test BFS shortest path algorithm.
#[test]
#[cfg(feature = "graph-algo")]
fn shortest_path_bfs() {
    let db = Db::open_mem().expect("opening database should succeed");

    let result = db
        .run_read_only(
            r#"
            edges[src, dst] <- [["A", "B"], ["B", "C"], ["C", "D"], ["A", "D"]]
            start[] <- [["A"]]
            end[] <- [["D"]]
            ?[fr, to, path] <~ ShortestPathBFS(edges[], start[], end[])
            "#,
            BTreeMap::new(),
        )
        .expect("BFS query should succeed");

    assert!(!result.rows.is_empty(), "should return paths");
}

/// Test various DataValue types.
#[test]
fn data_value_types() {
    let db = Db::open_mem().expect("opening database should succeed");

    db.run(
        ":create typed_data {id: Int => s: String, b: Bool, n: Float}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    db.run(
        r#"?[id, s, b, n] <- [[1, "hello", true, 3.14]] :put typed_data {id => s, b, n}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting typed data should succeed");

    let result = db
        .run_read_only("?[s, b, n] := *typed_data{id: 1, s, b, n}", BTreeMap::new())
        .expect("querying typed data should succeed");

    assert_eq!(result.rows[0][0], DataValue::from("hello"));
    assert_eq!(result.rows[0][1], DataValue::from(true));
    assert!(result.rows[0][2].get_float().unwrap() - 3.14 < 0.001);
}
