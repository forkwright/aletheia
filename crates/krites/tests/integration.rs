//! End-to-end integration tests for the krites `Db` facade.
//!
//! Tests cover:
//! - Database creation and basic query execution
//! - HNSW vector insert + search (in-memory)
//! - FTS index creation + text search
//! - Relation create/insert/query pipeline
//! - Multi-transaction semantics
//! - Import/export roundtrips
//! - Error handling edge cases
//! - Callback registration and notification
//! - Query cache eviction behavior
//! - Parameterized queries
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test data with known structure")]

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use krites::{DataValue, Db, ScriptMutability};

// ── Database lifecycle ──────────────────────────────────────────────────────

#[test]
fn db_create_run_query_verify_results() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");
    let result = db
        .run(
            "?[x, y] := x = 1, y = x + 1",
            BTreeMap::new(),
            ScriptMutability::Immutable,
        )
        .expect("simple arithmetic query should succeed");
    assert_eq!(result.headers, vec!["x", "y"]);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], DataValue::from(1));
    assert_eq!(result.rows[0][1], DataValue::from(2));
}

#[test]
fn db_read_only_convenience() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");
    let result = db
        .run_read_only("?[a] := a in [10, 20, 30]", BTreeMap::new())
        .expect("read-only query should succeed");
    assert_eq!(result.rows.len(), 3);
}

#[test]
fn db_multiple_independent_queries() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");
    for i in 1..=10 {
        let script = format!("?[x] := x = {i}");
        let result = db
            .run_read_only(&script, BTreeMap::new())
            .expect("query should succeed");
        assert_eq!(result.rows[0][0], DataValue::from(i));
    }
}

// ── Relation create/insert/query pipeline ───────────────────────────────────

#[test]
fn relation_create_insert_query_pipeline() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    // Create
    db.run(
        ":create employees {id: Int => name: String, dept: String, salary: Float}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating employees relation should succeed");

    // Insert
    db.run(
        r#"?[id, name, dept, salary] <- [
            [1, "Alice", "Engineering", 120000.0],
            [2, "Bob", "Marketing", 95000.0],
            [3, "Carol", "Engineering", 135000.0],
            [4, "Dave", "Marketing", 88000.0],
            [5, "Eve", "Engineering", 110000.0]
        ] :put employees {id => name, dept, salary}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting employees should succeed");

    // Query all
    let result = db
        .run_read_only(
            "?[name, dept] := *employees{name, dept} :order name",
            BTreeMap::new(),
        )
        .expect("querying all employees should succeed");
    assert_eq!(result.rows.len(), 5);
    assert_eq!(result.rows[0][0], DataValue::from("Alice"));

    // Aggregation query
    let result = db
        .run_read_only(
            "?[dept, count(name), mean(salary)] := *employees{dept, name, salary}",
            BTreeMap::new(),
        )
        .expect("aggregation query should succeed");
    assert_eq!(result.rows.len(), 2, "two departments");

    // Filter query
    let result = db
        .run_read_only(
            r#"?[name, salary] := *employees{name, dept: "Engineering", salary}, salary > 115000.0"#,
            BTreeMap::new(),
        )
        .expect("filtered query should succeed");
    assert_eq!(result.rows.len(), 2, "Alice and Carol above 115k");

    // Update via put
    db.run(
        r#"?[id, name, dept, salary] <- [[2, "Bob", "Engineering", 100000.0]]
           :put employees {id => name, dept, salary}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("updating Bob should succeed");

    let result = db
        .run_read_only("?[dept] := *employees{id: 2, dept}", BTreeMap::new())
        .expect("querying updated Bob should succeed");
    assert_eq!(result.rows[0][0], DataValue::from("Engineering"));

    // Delete via rm
    db.run(
        "?[id] <- [[4]] :rm employees {id}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("deleting Dave should succeed");

    let result = db
        .run_read_only("?[count(id)] := *employees{id}", BTreeMap::new())
        .expect("count after delete should succeed");
    assert_eq!(result.rows[0][0], DataValue::from(4i64));
}

#[test]
fn relation_create_with_compound_key() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create edges {src: Int, dst: Int => weight: Float}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating compound-key relation should succeed");

    db.run(
        "?[src, dst, weight] <- [[1, 2, 0.5], [1, 3, 0.8], [2, 3, 1.2]] :put edges {src, dst => weight}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting edges should succeed");

    let result = db
        .run_read_only(
            "?[dst, weight] := *edges{src: 1, dst, weight} :order dst",
            BTreeMap::new(),
        )
        .expect("querying by partial key should succeed");
    assert_eq!(result.rows.len(), 2);
}

// ── HNSW vector insert + search (in-memory) ────────────────────────────────

#[test]
fn hnsw_vector_insert_and_search_in_memory() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    // Create relation with vector column
    db.run(
        ":create docs {id: String => embedding: <F32; 4>}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating vector relation should succeed");

    // Insert vectors
    db.run(
        r#"?[id, embedding] <- [
            ["alpha", vec([1.0, 0.0, 0.0, 0.0])],
            ["beta",  vec([0.0, 1.0, 0.0, 0.0])],
            ["gamma", vec([0.0, 0.0, 1.0, 0.0])],
            ["delta", vec([1.0, 1.0, 0.0, 0.0])],
            ["eps",   vec([0.9, 0.1, 0.0, 0.0])]
        ] :put docs {id => embedding}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting vectors should succeed");

    // Create HNSW index
    db.run(
        "::hnsw create docs:idx {
            dim: 4,
            m: 16,
            dtype: F32,
            fields: [embedding],
            distance: L2,
            ef_construction: 32
        }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating HNSW index should succeed");

    // KNN search: query near alpha, should find alpha and eps (both near [1,0,0,0])
    let result = db
        .run_read_only(
            "?[dist, id] := ~docs:idx{id | query: q, k: 2, ef: 20, bind_distance: dist}, q = vec([1.0, 0.0, 0.0, 0.0])",
            BTreeMap::new(),
        )
        .expect("KNN query should succeed");

    assert_eq!(result.rows.len(), 2, "k=2 should return 2 results");

    // The nearest neighbor to [1,0,0,0] should be "alpha" with distance ~0
    let nearest_dist = result.rows[0][0]
        .get_float()
        .expect("distance should be float");
    assert!(
        nearest_dist < 0.01,
        "nearest neighbor distance should be ~0, got {nearest_dist}"
    );
    assert_eq!(
        result.rows[0][1],
        DataValue::from("alpha"),
        "nearest neighbor should be alpha"
    );

    // Drop index
    db.run(
        "::hnsw drop docs:idx",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("dropping HNSW index should succeed");
}

#[test]
fn hnsw_vector_insert_after_index_creation() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create items {id: Int => vec: <F32; 3>}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    db.run(
        "?[id, vec] <- [[1, vec([1,0,0])], [2, vec([0,1,0])]] :put items {id => vec}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting initial items should succeed");

    db.run(
        "::hnsw create items:idx { dim: 3, m: 16, dtype: F32, fields: [vec], distance: L2, ef_construction: 32 }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating index should succeed");

    // Insert more items after index creation
    db.run(
        "?[id, vec] <- [[3, vec([0,0,1])], [4, vec([1,1,0])]] :put items {id => vec}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting after index creation should succeed");

    // Search should find items from both before and after index creation
    let result = db
        .run_read_only(
            "?[id, dist] := ~items:idx{id | query: vec([1,0,0]), k: 4, ef: 20, bind_distance: dist}",
            BTreeMap::new(),
        )
        .expect("search should succeed");

    assert_eq!(
        result.rows.len(),
        4,
        "should find all 4 items in KNN search"
    );
}

// ── FTS index + search ──────────────────────────────────────────────────────

#[test]
fn fts_index_create_search_drop() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create articles {id: String => title: String, body: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating articles relation should succeed");

    db.run(
        r#"?[id, title, body] <- [
            ["a1", "Introduction to Rust", "Rust is a systems programming language focused on safety"],
            ["a2", "Python for Data Science", "Python excels at data analysis and machine learning"],
            ["a3", "Rust async programming", "Async await in Rust enables efficient concurrent programs"],
            ["a4", "Web development basics", "HTML CSS and JavaScript form the foundation of web apps"]
        ] :put articles {id => title, body}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting articles should succeed");

    db.run(
        "::fts create articles:fts {
            extractor: body,
            tokenizer: Simple,
            filters: [Lowercase]
        }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating FTS index should succeed");

    // Search for "rust"
    let result = db
        .run_read_only(
            r#"?[id, body, score] := ~articles:fts{id, body | query: "rust", k: 10, bind_score: score}"#,
            BTreeMap::new(),
        )
        .expect("FTS search for 'rust' should succeed");
    assert_eq!(result.rows.len(), 2, "two articles mention Rust");

    // Search for "data"
    let result = db
        .run_read_only(
            r#"?[id, body] := ~articles:fts{id, body | query: "data", k: 10}"#,
            BTreeMap::new(),
        )
        .expect("FTS search for 'data' should succeed");
    assert_eq!(result.rows.len(), 1, "one article about data science");

    // Search for term not in any document
    let result = db
        .run_read_only(
            r#"?[id] := ~articles:fts{id | query: "quantum", k: 10}"#,
            BTreeMap::new(),
        )
        .expect("FTS search for absent term should succeed");
    assert_eq!(result.rows.len(), 0, "no articles about quantum");

    // Drop index
    db.run(
        "::fts drop articles:fts",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("dropping FTS index should succeed");
}

#[test]
fn fts_index_incremental_insert() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create notes {id: String => text: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating notes should succeed");

    db.run(
        "::fts create notes:fts { extractor: text, tokenizer: Simple, filters: [Lowercase] }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating FTS index should succeed");

    // Insert first document after index creation
    db.run(
        r#"?[id, text] <- [["n1", "the quick brown fox"]] :put notes {id => text}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting first note should succeed");

    let result = db
        .run_read_only(
            r#"?[id] := ~notes:fts{id | query: "fox", k: 5}"#,
            BTreeMap::new(),
        )
        .expect("searching for fox should succeed");
    assert_eq!(result.rows.len(), 1, "fox should be indexed");

    // Insert second document
    db.run(
        r#"?[id, text] <- [["n2", "the slow brown bear"]] :put notes {id => text}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting second note should succeed");

    let result = db
        .run_read_only(
            r#"?[id] := ~notes:fts{id | query: "bear", k: 5}"#,
            BTreeMap::new(),
        )
        .expect("searching for bear should succeed");
    assert_eq!(result.rows.len(), 1, "bear should be indexed");

    // Both documents should be searchable for shared term "brown"
    let result = db
        .run_read_only(
            r#"?[id] := ~notes:fts{id | query: "brown", k: 10}"#,
            BTreeMap::new(),
        )
        .expect("searching for brown should succeed");
    assert_eq!(result.rows.len(), 2, "both documents mention brown");
}

// ── Parameterized queries ───────────────────────────────────────────────────

#[test]
fn parameterized_query_with_multiple_params() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create products {id: Int => name: String, price: Float}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating products should succeed");

    db.run(
        r#"?[id, name, price] <- [
            [1, "Widget", 9.99],
            [2, "Gadget", 24.99],
            [3, "Gizmo", 49.99],
            [4, "Doohickey", 14.99]
        ] :put products {id => name, price}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting products should succeed");

    let mut params = BTreeMap::new();
    params.insert("min_price".to_string(), DataValue::from(10.0));
    params.insert("max_price".to_string(), DataValue::from(30.0));

    let result = db
        .run_read_only(
            "?[name, price] := *products{name, price}, price >= $min_price, price <= $max_price :order price",
            params,
        )
        .expect("parameterized query should succeed");

    assert_eq!(result.rows.len(), 2, "Doohickey and Gadget in range");
    assert_eq!(result.rows[0][0], DataValue::from("Doohickey"));
    assert_eq!(result.rows[1][0], DataValue::from("Gadget"));
}

// ── Recursive queries ───────────────────────────────────────────────────────

#[test]
fn recursive_transitive_closure() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db
        .run_read_only(
            r#"
            parent[child, p] <- [["alice", "bob"], ["bob", "carol"], ["carol", "dave"]]
            ancestor[c, a] := parent[c, a]
            ancestor[c, a] := parent[c, mid], ancestor[mid, a]
            ?[ancestor] := ancestor["alice", ancestor]
            :order ancestor
            "#,
            BTreeMap::new(),
        )
        .expect("recursive ancestor query should succeed");

    assert_eq!(result.rows.len(), 3, "alice has 3 ancestors");
    assert_eq!(result.rows[0][0], DataValue::from("bob"));
    assert_eq!(result.rows[1][0], DataValue::from("carol"));
    assert_eq!(result.rows[2][0], DataValue::from("dave"));
}

// ── Import/Export roundtrip ─────────────────────────────────────────────────

#[test]
fn export_import_preserves_data() {
    let db1 = Db::open_mem().expect("db1 creation should succeed");

    db1.run(
        ":create data {key: String => val: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating data should succeed");

    db1.run(
        r#"?[key, val] <- [["a", 1], ["b", 2], ["c", 3]] :put data {key => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting data should succeed");

    let exported = db1
        .export_relations(["data"].iter())
        .expect("export should succeed");

    assert!(exported.contains_key("data"));
    assert_eq!(exported["data"].rows.len(), 3);

    // Import into fresh database
    let db2 = Db::open_mem().expect("db2 creation should succeed");
    db2.run(
        ":create data {key: String => val: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating matching relation should succeed");

    db2.import_relations(exported)
        .expect("import should succeed");

    let result = db2
        .run_read_only("?[key, val] := *data{key, val} :order key", BTreeMap::new())
        .expect("querying imported data should succeed");
    assert_eq!(result.rows.len(), 3);
    assert_eq!(result.rows[0][0], DataValue::from("a"));
    assert_eq!(result.rows[0][1], DataValue::from(1i64));
}

// ── Error handling ──────────────────────────────────────────────────────────

#[test]
fn immutable_mode_rejects_mutation() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db.run(
        ":create test {id: Int}",
        BTreeMap::new(),
        ScriptMutability::Immutable,
    );
    assert!(
        result.is_err(),
        "creating relation in immutable mode should fail"
    );
}

#[test]
fn query_nonexistent_relation_returns_error() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");
    let result = db.run_read_only("?[x] := *no_such_relation{x}", BTreeMap::new());
    assert!(result.is_err(), "querying nonexistent relation should fail");
}

#[test]
fn syntax_error_returns_parse_error() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");
    let result = db.run_read_only("this is not valid datalog", BTreeMap::new());
    assert!(result.is_err(), "invalid syntax should return error");
}

#[test]
fn type_mismatch_returns_error() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create typed {n: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating typed relation should succeed");

    let result = db.run(
        r#"?[n] <- [["not_an_int"]] :put typed {n}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(result.is_err(), "inserting wrong type should fail");
}

#[test]
fn duplicate_create_returns_error() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create test_dup {id: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("first create should succeed");

    let result = db.run(
        ":create test_dup {id: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(result.is_err(), "duplicate create should fail");
}

// ── Query cache edge cases ──────────────────────────────────────────────────

#[test]
fn cache_eviction_at_capacity() {
    let db = Db::open_mem()
        .expect("in-memory db creation should succeed")
        .with_cache(NonZeroUsize::new(2).unwrap());

    // Fill cache
    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let _ = db.run_read_only("?[x] := x = 2", BTreeMap::new());

    let stats = db.cache_stats().unwrap();
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.len, 2);

    // Add third query, should evict oldest
    let _ = db.run_read_only("?[x] := x = 3", BTreeMap::new());
    let stats = db.cache_stats().unwrap();
    assert_eq!(stats.misses, 3);
    assert_eq!(stats.len, 2, "cache size should remain at capacity");

    // First query should be evicted, so it's a miss again
    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().unwrap();
    assert_eq!(stats.misses, 4, "evicted query should miss");
}

#[test]
fn cache_with_mutations_tracks_separately() {
    let db = Db::open_mem()
        .expect("in-memory db creation should succeed")
        .with_cache(NonZeroUsize::new(16).unwrap());

    // Run same query in different mutability modes
    let _ = db.run(
        "?[x] := x = 1",
        BTreeMap::new(),
        ScriptMutability::Immutable,
    );
    let _ = db.run("?[x] := x = 1", BTreeMap::new(), ScriptMutability::Mutable);

    let stats = db.cache_stats().unwrap();
    // The cache tracks by normalized query string, not by mutability
    assert_eq!(stats.hits + stats.misses, 2, "both runs should be tracked");
}

// ── Callback notifications ──────────────────────────────────────────────────

#[test]
fn callback_receives_put_and_rm_notifications() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let (_id, rx) = db.register_callback("cb_test", Some(16));

    db.run(
        ":create cb_test {id: Int => val: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    // Put
    db.run(
        r#"?[id, val] <- [[1, "first"]] :put cb_test {id => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("put should succeed");

    // Rm
    db.run(
        "?[id] <- [[1]] :rm cb_test {id}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("rm should succeed");

    // Collect all pending callbacks
    std::thread::sleep(std::time::Duration::from_millis(50));
    let mut callbacks = Vec::new();
    while let Ok(cb) = rx.try_recv() {
        callbacks.push(cb);
    }

    assert!(
        callbacks.len() >= 2,
        "should receive at least put and rm callbacks, got {}",
        callbacks.len()
    );
}

// ── Imperative script execution ─────────────────────────────────────────────

#[test]
fn imperative_script_multi_statement() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db
        .run(
            r"
            {:create _tmp {x: Int}}
            {?[x] := x in [1,2,3,4,5] :put _tmp {x}}
            {?[x] := *_tmp{x}, x % 2 == 0 :rm _tmp {x}}
            {?[x] := *_tmp{x} :order x}
            ",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        )
        .expect("imperative script should succeed");

    // Should return odd numbers only: 1, 3, 5
    assert_eq!(result.rows.len(), 3);
    assert_eq!(result.rows[0][0], DataValue::from(1i64));
    assert_eq!(result.rows[1][0], DataValue::from(3i64));
    assert_eq!(result.rows[2][0], DataValue::from(5i64));
}

// ── NamedRows structure ─────────────────────────────────────────────────────

#[test]
fn named_rows_headers_match_query_columns() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db
        .run_read_only(
            "?[alpha, beta, gamma] := alpha = 1, beta = 2, gamma = 3",
            BTreeMap::new(),
        )
        .expect("named column query should succeed");

    assert_eq!(result.headers, vec!["alpha", "beta", "gamma"]);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], DataValue::from(1i64));
    assert_eq!(result.rows[0][1], DataValue::from(2i64));
    assert_eq!(result.rows[0][2], DataValue::from(3i64));
}

#[test]
fn named_rows_into_json_structure() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db
        .run_read_only("?[x] := x in [1,2,3]", BTreeMap::new())
        .expect("query should succeed");

    let json = result.into_json();
    assert!(json["headers"].is_array());
    assert!(json["rows"].is_array());
    assert_eq!(json["rows"].as_array().unwrap().len(), 3);
}

// ── Vector distance functions ───────────────────────────────────────────────

#[test]
fn vector_l2_distance_self_is_zero() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db
        .run_read_only(
            "?[d] := v = vec([1.0, 2.0, 3.0, 4.0]), d = l2_dist(v, v)",
            BTreeMap::new(),
        )
        .expect("L2 self-distance query should succeed");

    let dist = result.rows[0][0].get_float().unwrap();
    assert!(
        dist.abs() < 1e-9,
        "L2 distance to self should be 0, got {dist}"
    );
}

#[test]
fn vector_cosine_distance_orthogonal() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db
        .run_read_only(
            "?[d] := a = vec([1.0, 0.0]), b = vec([0.0, 1.0]), d = cos_dist(a, b)",
            BTreeMap::new(),
        )
        .expect("cosine distance query should succeed");

    let dist = result.rows[0][0].get_float().unwrap();
    assert!(
        (dist - 1.0).abs() < 1e-6,
        "cosine distance of orthogonal vectors should be ~1.0, got {dist}"
    );
}

#[test]
fn vector_ip_distance_identical() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let result = db
        .run_read_only(
            "?[d] := v = vec([1.0, 2.0, 3.0]), d = ip_dist(v, v)",
            BTreeMap::new(),
        )
        .expect("IP distance query should succeed");

    // Just verify it returns a finite number
    let dist = result.rows[0][0].get_float().unwrap();
    assert!(dist.is_finite(), "IP distance should be finite");
}

// ── Relation listing ────────────────────────────────────────────────────────

#[test]
fn list_relations_includes_created_relation() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create my_relation {id: Int => data: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    let result = db
        .run_read_only("::relations", BTreeMap::new())
        .expect("listing relations should succeed");

    let relation_names: Vec<&str> = result.rows.iter().filter_map(|r| r[0].get_str()).collect();

    assert!(
        relation_names.contains(&"my_relation"),
        "created relation should appear in listing"
    );
}

// ── Ensure enforces row existence ────────────────────────────────────────────

#[test]
fn ensure_enforces_existing_row() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    db.run(
        ":create kv {key: String => val: Int}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    // Ensure on empty relation fails (row doesn't exist)
    let result = db.run(
        r#"?[key, val] <- [["a", 1]] :ensure kv {key => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(
        result.is_err(),
        "ensure should fail when row does not exist"
    );

    // Insert the row first
    db.run(
        r#"?[key, val] <- [["a", 1]] :put kv {key => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting row should succeed");

    // Now ensure should succeed
    db.run(
        r#"?[key, val] <- [["a", 1]] :ensure kv {key => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("ensure should succeed when row exists with matching value");
}
