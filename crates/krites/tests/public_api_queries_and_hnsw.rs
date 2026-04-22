#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::doc_markdown, reason = "test documentation")]

use std::collections::BTreeMap;

use krites::{DataValue, Db, ScriptMutability};

// Split: Basic ops + Query execution + Transactions + HNSW vector search.

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
        .run_read_only("?[count(from)] := *edge{from, to}", BTreeMap::new())
        .expect("aggregation query should succeed");
    assert_eq!(result.rows[0][0], DataValue::from(4i64));

    // Test parameterized query
    let mut params = BTreeMap::new();
    params.insert("start".to_string(), DataValue::from(1i64));
    let result = db
        .run_read_only("?[to] := *edge{from: $start, to}", params)
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
    params.insert(
        "v1".to_string(),
        DataValue::Vec(krites::Vector::F32(ndarray::Array1::from(vec1.clone()))),
    );
    params.insert(
        "v2".to_string(),
        DataValue::Vec(krites::Vector::F32(ndarray::Array1::from(vec2))),
    );
    params.insert(
        "v3".to_string(),
        DataValue::Vec(krites::Vector::F32(ndarray::Array1::from(vec3.clone()))),
    );

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
    params.insert(
        "q".to_string(),
        DataValue::Vec(krites::Vector::F32(ndarray::Array1::from(query_vec))),
    );

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
        .run_read_only(
            "?[id, embedding] := *vectors{id, embedding}",
            BTreeMap::new(),
        )
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
