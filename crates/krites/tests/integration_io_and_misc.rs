#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test data with known structure")]
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use krites::{DataValue, Db, ScriptMutability};

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

    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let _ = db.run_read_only("?[x] := x = 2", BTreeMap::new());

    let stats = db.cache_stats().unwrap();
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.len, 2);

    let _ = db.run_read_only("?[x] := x = 3", BTreeMap::new());
    let stats = db.cache_stats().unwrap();
    assert_eq!(stats.misses, 3);
    assert_eq!(stats.len, 2, "cache size should remain at capacity");

    let _ = db.run_read_only("?[x] := x = 1", BTreeMap::new());
    let stats = db.cache_stats().unwrap();
    assert_eq!(stats.misses, 4, "evicted query should miss");
}

#[test]
fn cache_with_mutations_tracks_separately() {
    let db = Db::open_mem()
        .expect("in-memory db creation should succeed")
        .with_cache(NonZeroUsize::new(16).unwrap());

    let _ = db.run(
        "?[x] := x = 1",
        BTreeMap::new(),
        ScriptMutability::Immutable,
    );
    let _ = db.run("?[x] := x = 1", BTreeMap::new(), ScriptMutability::Mutable);

    let stats = db.cache_stats().unwrap();
    // NOTE: the cache keys on the normalized query string, not on mutability.
    assert_eq!(stats.hits + stats.misses, 2, "both runs should be tracked");
}

// ── Callback notifications ──────────────────────────────────────────────────

#[test]
fn callback_receives_put_and_rm_notifications() {
    let db = Db::open_mem().expect("in-memory db creation should succeed");

    let (_id, rx) = db.register_callback("cb_test", 16);

    db.run(
        ":create cb_test {id: Int => val: String}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating relation should succeed");

    db.run(
        r#"?[id, val] <- [[1, "first"]] :put cb_test {id => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("put should succeed");

    db.run(
        "?[id] <- [[1]] :rm cb_test {id}",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("rm should succeed");

    // WHY: callback delivery is asynchronous; allow time before draining.
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

    let result = db.run(
        r#"?[key, val] <- [["a", 1]] :ensure kv {key => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(
        result.is_err(),
        "ensure should fail when row does not exist"
    );

    db.run(
        r#"?[key, val] <- [["a", 1]] :put kv {key => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting row should succeed");

    db.run(
        r#"?[key, val] <- [["a", 1]] :ensure kv {key => val}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("ensure should succeed when row exists with matching value");
}
