//! Integration tests for the public `Db` facade: delegated methods and error behavior.
#![cfg(feature = "engine-tests")]
// Integration tests: assertions panic on unexpected structure.
#![allow(clippy::expect_used)]
#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use mneme::engine::{DataValue, Db, ScriptMutability};

#[test]
fn run_read_only_returns_data() {
    let db = Db::open_mem().expect("open mem");
    db.run(
        ":create test { k: String => v: Int }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("create relation");
    db.run(
        "?[k, v] <- [['alice', 42]] :put test { k => v }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("insert");

    let result = db
        .run_read_only("?[k, v] := *test[k, v]", BTreeMap::new())
        .expect("read-only query");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], DataValue::Str("alice".into()));
    assert_eq!(result.rows[0][1], DataValue::from(42i64));
}

#[test]
fn relation_snapshot_round_trip_is_supported() {
    let db = Db::open_mem().expect("open mem");
    db.run(
        ":create source { id: Int => value: String }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("create source relation");
    db.run(
        r#"?[id, value] <- [[1, "alpha"], [2, "beta"]] :put source { id => value }"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("insert source rows");

    let snapshot = db
        .export_relations(["source"].iter())
        .expect("export relation snapshot");
    assert_eq!(snapshot["source"].rows.len(), 2);

    let restored = Db::open_mem().expect("open restored mem");
    restored
        .run(
            ":create source { id: Int => value: String }",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        )
        .expect("create restored source relation");
    restored
        .import_relations(snapshot)
        .expect("import relation snapshot");

    let result = restored
        .run_read_only(
            "?[id, value] := *source{id, value} :order id",
            BTreeMap::new(),
        )
        .expect("query restored rows");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0][1], DataValue::from("alpha"));
    assert_eq!(result.rows[1][1], DataValue::from("beta"));
}

#[test]
fn insert_enforces_key_uniqueness() {
    let db = Db::open_mem().expect("open mem");

    db.run(
        ":create facts { id: String, valid_from: String => content: String }",
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("create facts relation");

    db.run(
        r#"?[id, valid_from, content] <- [["f-1", "2026-01-01", "original"]] :insert facts {id, valid_from => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("first insert should succeed");

    let result = db.run(
        r#"?[id, valid_from, content] <- [["f-1", "2026-01-01", "duplicate"]] :insert facts {id, valid_from => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    );
    assert!(
        result.is_err(),
        "duplicate composite key insert should fail"
    );

    db.run(
        r#"?[id, valid_from, content] <- [["f-1", "2026-01-01", "updated"]] :put facts {id, valid_from => content}"#,
        BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("put should succeed as upsert");

    let rows = db
        .run_read_only(
            "?[content] := *facts{id: 'f-1', valid_from: '2026-01-01', content}",
            BTreeMap::new(),
        )
        .expect("query");
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0][0], DataValue::Str("updated".into()));
}
