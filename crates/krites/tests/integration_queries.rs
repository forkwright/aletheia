#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions")]

use std::collections::BTreeMap;

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

    db.run(
        ":create employees {id: Int => name: String, dept: String, salary: Float}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating employees relation should succeed");

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

    let result = db
        .run_read_only(
            "?[name, dept] := *employees{name, dept} :order name",
            BTreeMap::new(),
        )
        .expect("querying all employees should succeed");
    assert_eq!(result.rows.len(), 5);
    assert_eq!(result.rows[0][0], DataValue::from("Alice"));

    let result = db
        .run_read_only(
            "?[dept, count(name), mean(salary)] := *employees{dept, name, salary}",
            BTreeMap::new(),
        )
        .expect("aggregation query should succeed");
    assert_eq!(result.rows.len(), 2, "two departments");

    let result = db
        .run_read_only(
            r#"?[name, salary] := *employees{name, dept: "Engineering", salary}, salary > 115000.0"#,
            BTreeMap::new(),
        )
        .expect("filtered query should succeed");
    assert_eq!(result.rows.len(), 2, "Alice and Carol above 115k");

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

    db.run(
        ":create docs {id: String => embedding: <F32; 4>}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating vector relation should succeed");

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

    db.run(
        "?[id, vec] <- [[3, vec([0,0,1])], [4, vec([1,1,0])]] :put items {id => vec}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting after index creation should succeed");

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

    let result = db
        .run_read_only(
            r#"?[id, body, score] := ~articles:fts{id, body | query: "rust", k: 10, bind_score: score}"#,
            BTreeMap::new(),
        )
        .expect("FTS search for 'rust' should succeed");
    assert_eq!(result.rows.len(), 2, "two articles mention Rust");

    let result = db
        .run_read_only(
            r#"?[id, body] := ~articles:fts{id, body | query: "data", k: 10}"#,
            BTreeMap::new(),
        )
        .expect("FTS search for 'data' should succeed");
    assert_eq!(result.rows.len(), 1, "one article about data science");

    let result = db
        .run_read_only(
            r#"?[id] := ~articles:fts{id | query: "quantum", k: 10}"#,
            BTreeMap::new(),
        )
        .expect("FTS search for absent term should succeed");
    assert_eq!(result.rows.len(), 0, "no articles about quantum");

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
