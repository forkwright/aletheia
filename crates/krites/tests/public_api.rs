//! Integration tests for the krites public API.
//!
//! Covers the main surface area used by external consumers:
//! - `Db` creation (in-memory and fjall-backed)
//! - Query execution and result handling
//! - Transaction semantics
//! - Index operations (HNSW, FTS)
//! - Error handling
//! - Import/export, callbacks, query cache
//!
//! # Test Organization
//!
//! Tests are grouped by feature area:
//! - Basic CRUD operations
//! - Query execution
//! - Transaction semantics
//! - Vector/HNSW operations
//! - Full-text search
//! - Import/export and backup
//! - Callbacks and change notifications
//! - Query cache
//! - Error handling
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

// ============================================================================
// Basic Database Operations
// ============================================================================

/// Test opening an in-memory database and basic lifecycle.
#[test]
fn open_mem_db_creates_working_database() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");
    let result = db
        .run_read_only("?[x] := x = 42", BTreeMap::new())
        .expect("simple query should succeed");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], DataValue::from(42));
}

/// Test creating a relation, inserting tuples, querying by key, and dropping.
#[test]
fn relation_crud_lifecycle() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Create relation
    db.run(
        ":create users {id: Int => name: String, email: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    // Insert tuples
    db.run(
        r#"?[id, name, email] <- [[1, "Alice", "alice@example.com"],
                                  [2, "Bob", "bob@example.com"],
                                  [3, "Carol", "carol@example.com"]]
          :put users {id => name, email}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting users should succeed");

    // Query by id
    let result = db
        .run_read_only(
            "?[name, email] := *users{id: 1, name, email}",
            BTreeMap::new(),
        )
        .expect("querying by id should succeed");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], DataValue::from("Alice"));
    assert_eq!(result.rows[0][1], DataValue::from("alice@example.com"));

    // Query all
    let result = db
        .run_read_only("?[id, name] := *users{id, name}", BTreeMap::new())
        .expect("querying all users should succeed");
    assert_eq!(result.rows.len(), 3);

    // Drop relation via system op
    db.run("::remove users", BTreeMap::new(), ScriptMutability::Mutable)
        .expect("dropping relation should succeed");

    // Query after drop should fail
    let result = db.run_read_only("?[id] := *users{id}", BTreeMap::new());
    assert!(result.is_err(), "querying dropped relation should fail");
}

/// Test that insertion respects key uniqueness constraints.
#[test]
fn insert_enforces_key_uniqueness() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    db.run(
        ":create items {id: Int => value: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    // First insert should succeed
    db.run(
        r#"?[id, value] <- [[1, "first"]] :insert items {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("first insert should succeed");

    // Duplicate key insert should fail
    let result = db.run(
        r#"?[id, value] <- [[1, "duplicate"]] :insert items {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(result.is_err(), "duplicate key insert should fail");

    // Put should succeed (upsert behavior)
    db.run(
        r#"?[id, value] <- [[1, "updated"]] :put items {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("put should succeed as upsert");

    // Verify the update
    let result = db
        .run_read_only("?[value] := *items{id: 1, value}", BTreeMap::new())
        .expect("querying should succeed");
    assert_eq!(result.rows[0][0], DataValue::from("updated"));
}

// ============================================================================
// Query Execution
// ============================================================================

/// Test basic Datalog query execution with various features.
#[test]
fn datalog_query_execution() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Create edge relation for graph queries
    db.run(
        ":create edge {from: Int, to: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating edge relation should succeed");

    // Insert edges forming a simple graph: 1->2, 1->3, 2->3, 3->4
    db.run(
        "?[from, to] <- [[1, 2], [1, 3], [2, 3], [3, 4]] :put edge {from, to}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting edges should succeed");

    // Test basic query
    let result = db
        .run_read_only("?[from, to] := *edge{from, to}", BTreeMap::new())
        .expect("querying all edges should succeed");
    assert_eq!(result.rows.len(), 4);

    // Test recursive query (transitive closure)
    let result = db
        .run_read_only(
            "reachable[to] := *edge{from: 1, to}\n\
             reachable[to] := reachable[intermediate], *edge{from: intermediate, to}\n\
             ?[node] := reachable[node]",
            BTreeMap::new(),
        )
        .expect("recursive query should succeed");
    // Should find nodes 2, 3, 4 reachable from 1
    assert_eq!(result.rows.len(), 3);

    // Test aggregation
    let result = db
        .run_read_only(
            "?[count(from)] := *edge{from, to}",
            BTreeMap::new(),
        )
        .expect("aggregation query should succeed");
    assert_eq!(result.rows[0][0], DataValue::from(4i64));

    // Test parameterized query
    let mut params = BTreeMap::new();
    params.insert("start".to_string(), DataValue::from(1i64));
    let result = db
        .run_read_only(
            "?[to] := *edge{from: $start, to}",
            params,
        )
        .expect("parameterized query should succeed");
    assert_eq!(result.rows.len(), 2); // 1->2 and 1->3
}

/// Test query with sorting and limits.
#[test]
fn query_with_sorting_and_limits() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    db.run(
        ":create numbers {n: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    db.run(
        "?[n] <- [[5], [2], [8], [1], [9], [3]] :put numbers {n}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting numbers should succeed");

    // Test with limit
    let result = db
        .run_read_only("?[n] := *numbers{n} :limit 3", BTreeMap::new())
        .expect("query with limit should succeed");
    assert_eq!(result.rows.len(), 3);

    // Test with sorting - use :order -n for descending
    let result = db
        .run_read_only("?[n] := *numbers{n} :order -n :limit 3", BTreeMap::new())
        .expect("sorted query should succeed");
    assert_eq!(result.rows[0][0], DataValue::from(9i64));
    assert_eq!(result.rows[1][0], DataValue::from(8i64));
    assert_eq!(result.rows[2][0], DataValue::from(5i64));
}

// ============================================================================
// Transactions
// ============================================================================

/// Test multi-transaction handle creation.
/// Note: The MultiTransaction receiver returns InternalResult which is private.
/// External users can send commands via the sender channel, but result handling
/// requires internal API access. This test verifies the handle structure exists.
#[test]
fn multi_transaction_handle_creation() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Start a write transaction - the handle provides channels for communication
    let _tx = db.multi_transaction(true);
    // The channels exist but receiver's error type is private.
    // In real usage, external consumers would need to use the internal test helpers
    // or the public API would need to expose a wrapper.
}

/// Test multi-transaction read-only handle creation.
#[test]
fn multi_transaction_read_only_handle() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Create some data first
    db.run(
        ":create readonly_test {id: Int => value: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    // Start a read-only transaction
    let _tx = db.multi_transaction(false);
    // Transaction handle is created successfully
}

// ============================================================================
// HNSW Vector Search
// ============================================================================

/// Test HNSW index creation and KNN search.
#[cfg(feature = "storage-fjall")]
#[test]
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "test vector construction - precision loss acceptable for test data"
)]
fn hnsw_vector_search_lifecycle() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("creating temp dir should succeed");
    let db = Db::open_fjall(&temp_dir).expect("opening fjall database should succeed");

    // Create relation with vector column
    db.run(
        ":create embeddings {id: String => vec: <F32; 128>}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating vector relation should succeed");

    // Insert some vectors
    let vec1: Vec<f32> = (0..128).map(|i| i as f32).collect();
    let vec2: Vec<f32> = (0..128).map(|i| (i * 2) as f32).collect();
    let vec3: Vec<f32> = (0..128).map(|i| (i + 100) as f32).collect();

    let mut params = BTreeMap::new();
    params.insert("v1".to_string(), DataValue::Vec(krites::Vector::F32(
        ndarray::Array1::from(vec1.clone()),
    )));
    params.insert("v2".to_string(), DataValue::Vec(krites::Vector::F32(
        ndarray::Array1::from(vec2),
    )));
    params.insert("v3".to_string(), DataValue::Vec(krites::Vector::F32(
        ndarray::Array1::from(vec3.clone()),
    )));

    db.run(
        r#"?[id, vec] <- [["a", $v1], ["b", $v2], ["c", $v3]] :put embeddings {id => vec}"#,
        params,
        ScriptMutability::Mutable,
    )
    .expect("inserting vectors should succeed");

    // Create HNSW index
    db.run(
        "::hnsw create embeddings:hnsw_idx {\n\
         dim: 128,\n\
         m: 16,\n\
         ef_construction: 64,\n\
         fields: [vec],\n\
         distance: L2\n\
         }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating HNSW index should succeed");

    // Perform KNN search using the index
    let query_vec: Vec<f32> = (0..128).map(|i| i as f32 + 0.5).collect();
    let mut params = BTreeMap::new();
    params.insert("q".to_string(), DataValue::Vec(krites::Vector::F32(
        ndarray::Array1::from(query_vec),
    )));

    let result = db
        .run(
            "?[id, dist] := ~embeddings:hnsw_idx{id | query: $q, k: 2, ef: 32, bind_distance: dist}",
            params,
            ScriptMutability::Immutable,
        )
        .expect("KNN search should succeed");

    // Should return at least one result
    assert!(!result.rows.is_empty(), "KNN search should return results");

    // Drop the index
    db.run(
        "::hnsw drop embeddings:hnsw_idx",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("dropping HNSW index should succeed");
}

/// Test in-memory HNSW operations (without requiring storage-fjall).
#[test]
fn vector_operations_in_memory() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Create relation with vector column
    db.run(
        ":create vectors {id: String => embedding: <F32; 8>}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating vector relation should succeed");

    // Insert vectors using vec() constructor
    db.run(
        r#"?[id, embedding] <- [["doc1", vec([1,2,3,4,5,6,7,8])],
                              ["doc2", vec([8,7,6,5,4,3,2,1])],
                              ["doc3", vec([1,1,1,1,1,1,1,1])]]
          :put vectors {id => embedding}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting vectors should succeed");

    // Query vectors back
    let result = db
        .run_read_only("?[id, embedding] := *vectors{id, embedding}", BTreeMap::new())
        .expect("querying vectors should succeed");
    assert_eq!(result.rows.len(), 3);

    // Test vector distance functions
    let result = db
        .run_read_only(
            "?[l2] := v = vec([1,2,3,4,5,6,7,8]), l2 = l2_dist(v, v)",
            BTreeMap::new(),
        )
        .expect("L2 distance query should succeed");
    // L2 distance of a vector to itself should be 0
    assert!(result.rows[0][0].get_float().unwrap() < 0.001);
}

// ============================================================================
// Full-Text Search
// ============================================================================

/// Test FTS index creation, text search, and index lifecycle.
#[test]
fn fts_index_lifecycle() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Create relation with text column
    db.run(
        ":create documents {id: String => content: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating documents relation should succeed");

    // Insert documents
    db.run(
        r#"?[id, content] <- [["doc1", "The quick brown fox jumps over the lazy dog"],
                            ["doc2", "A quick brown dog outpaces the lazy fox"],
                            ["doc3", "Slow and steady wins the race"]]
          :put documents {id => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting documents should succeed");

    // Create FTS index
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

    // Search for "quick"
    let result = db
        .run_read_only(
            "?[id, content, score] := ~documents:fts_idx{id, content | query: \"quick\", k: 10, bind_score: score}",
            BTreeMap::new(),
        )
        .expect("FTS search should succeed");
    assert_eq!(result.rows.len(), 2, "should find documents containing 'quick'");

    // Search for "fox" - FTS requires k parameter in the search options
    let result = db
        .run_read_only(
            "?[id, content, score] := ~documents:fts_idx{id, content | query: \"fox\", k: 10, bind_score: score}",
            BTreeMap::new(),
        )
        .expect("FTS search for 'fox' should succeed");
    assert_eq!(result.rows.len(), 2, "should find documents containing 'fox'");

    // Add more documents after index creation (should be auto-indexed)
    db.run(
        r#"?[id, content] <- [["doc4", "The quick red fox is very fast"]] :put documents {id => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting additional document should succeed");

    // Search again - should find the new document
    let result = db
        .run_read_only(
            "?[id, content] := ~documents:fts_idx{id, content | query: \"red\", k: 10}",
            BTreeMap::new(),
        )
        .expect("FTS search for 'red' should succeed");
    assert_eq!(result.rows.len(), 1, "should find document containing 'red'");

    // Drop the FTS index
    db.run(
        "::fts drop documents:fts_idx",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("dropping FTS index should succeed");
}

// ============================================================================
// Import/Export
// ============================================================================

/// Test exporting and importing relations.
#[test]
fn export_import_relations() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Create and populate relations
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

    // Export the relation
    let exported = db
        .export_relations(["source"].iter())
        .expect("exporting relation should succeed");

    assert!(exported.contains_key("source"));
    assert_eq!(exported["source"].rows.len(), 3);

    // Create new database and import into a relation with matching schema
    let db2 = Db::open_mem().expect("opening second database should succeed");

    db2.run(
        ":create source {id: Int => value: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating source relation in db2 should succeed");

    db2.import_relations(exported)
        .expect("importing relations should succeed");

    // Verify imported data
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

// ============================================================================
// Callbacks
// ============================================================================

/// Test registering callbacks and receiving change notifications.
#[test]
fn callback_receives_changes() {
    let db = Db::open_mem().expect("opening in-memory database should succeed");

    // Register callback before creating relation
    let (_callback_id, receiver) = db.register_callback("changes_test", Some(10));

    // Create relation
    db.run(
        ":create changes_test {id: Int => value: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    // Insert data
    db.run(
        r#"?[id, value] <- [[1, "first"], [2, "second"]] :put changes_test {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting should succeed");

    // Update data (should trigger callback with old and new values)
    db.run(
        r#"?[id, value] <- [[1, "updated"]] :put changes_test {id => value}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("updating should succeed");

    // Give callbacks time to be delivered
    std::thread::sleep(Duration::from_millis(50));

    // Collect callbacks
    let mut callbacks = Vec::new();
    while let Ok(cb) = receiver.try_recv() {
        callbacks.push(cb);
    }

    // Should have received callbacks for the puts
    assert!(!callbacks.is_empty(), "should have received callbacks");
}

// ============================================================================
// Query Cache
// ============================================================================

/// Test query cache statistics tracking.
#[test]
fn query_cache_tracks_hits_and_misses() {
    let db = Db::open_mem()
        .expect("opening database should succeed")
        .with_cache(NonZeroUsize::new(100).expect("non-zero cache size"));

    // First query should be a miss
    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 0);

    // Same query should be a hit
    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);

    // Different query should be another miss
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

    // First query with standard spacing
    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.misses, 1);

    // Same query with extra whitespace should hit
    let _ = db.run_read_only("  ?[x]   :=  x  =  1  ", BTreeMap::new());
    let stats = db.cache_stats().expect("cache stats should be available");
    assert_eq!(stats.hits, 1);
}

// ============================================================================
// Error Handling
// ============================================================================

/// Test error variants for malformed queries.
#[test]
fn malformed_query_errors() {
    let db = Db::open_mem().expect("opening database should succeed");

    // Syntax error
    let result = db.run_read_only("?[x] :=", BTreeMap::new());
    assert!(result.is_err(), "incomplete query should error");

    // Unknown relation
    let result = db.run_read_only("?[x] := *nonexistent{x}", BTreeMap::new());
    assert!(result.is_err(), "query on unknown relation should error");

    // Type error in query
    db.run(
        ":create typed {n: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");
    let result = db.run_read_only(r#"?[n] := *typed{n: "string"}"#, BTreeMap::new());
    assert!(
        result.is_err(),
        "type mismatch in query should error"
    );
}

/// Test error on invalid relation operations.
#[test]
fn invalid_relation_operations() {
    let db = Db::open_mem().expect("opening database should succeed");

    // Create relation
    db.run(
        ":create existing {id: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    // Creating same relation again should fail
    let result = db.run(
        ":create existing {id: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(result.is_err(), "creating duplicate relation should fail");

    // Dropping non-existent relation should fail
    let result = db.run(
        "::remove nonexistent_relation",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(result.is_err(), "dropping non-existent relation should fail");
}

// ============================================================================
// Fixed Rules (Graph Algorithms)
// ============================================================================

/// Test PageRank graph algorithm via fixed rules.
/// Test PageRank graph algorithm via fixed rules.
#[test]
#[cfg(feature = "graph-algo")]
fn pagerank_graph_algorithm() {
    let db = Db::open_mem().expect("opening database should succeed");

    // Run PageRank with inline edge definitions
    // Based on internal tests, PageRank uses inline relation syntax
    let result = db
        .run_read_only(
            r"
            edges[] <- [[1, 2], [2, 3], [3, 4], [4, 1]]
            ?[node, score] <~ PageRank(edges[_, _])
            ",
            BTreeMap::new(),
        )
        .expect("PageRank query should succeed");

    // Should have scores for all nodes
    assert!(!result.rows.is_empty(), "PageRank should return results");
    assert_eq!(result.rows.len(), 4, "PageRank should return 4 nodes");
}
/// Test BFS shortest path algorithm.
#[test]
#[cfg(feature = "graph-algo")]
fn shortest_path_bfs() {
    let db = Db::open_mem().expect("opening database should succeed");

    // Run ShortestPathBFS with inline relation definitions
    // Based on internal tests, the rule expects inline data definitions
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

// ============================================================================
// Data Types
// ============================================================================

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

    // Insert various types
    db.run(
        r#"?[id, s, b, n] <- [[1, "hello", true, 3.14]] :put typed_data {id => s, b, n}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting typed data should succeed");

    // Query back and verify
    let result = db
        .run_read_only("?[s, b, n] := *typed_data{id: 1, s, b, n}", BTreeMap::new())
        .expect("querying typed data should succeed");

    assert_eq!(result.rows[0][0], DataValue::from("hello"));
    assert_eq!(result.rows[0][1], DataValue::from(true));
    assert!(result.rows[0][2].get_float().unwrap() - 3.14 < 0.001);
}


