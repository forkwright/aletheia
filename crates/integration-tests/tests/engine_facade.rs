//! Integration tests for the public `Db` facade: delegated methods and error behavior.
#![cfg(feature = "engine-tests")]
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "integration tests: assertions panic on unexpected structure"
)]

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
fn backup_db_returns_unsupported_error() {
    let db = Db::open_mem().expect("open mem");
    let result = db.backup_db("/tmp/nonexistent.db");
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("storage-sqlite"),
        "expected mention of storage-sqlite, got: {msg}"
    );
}

#[test]
fn restore_backup_returns_unsupported_error() {
    let db = Db::open_mem().expect("open mem");
    let result = db.restore_backup("/tmp/nonexistent.db");
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("storage-sqlite"),
        "expected mention of storage-sqlite, got: {msg}"
    );
}

#[test]
fn import_from_backup_returns_unsupported_error() {
    let db = Db::open_mem().expect("open mem");
    let result = db.import_from_backup("/tmp/nonexistent.db", &[]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("storage-sqlite"),
        "expected mention of storage-sqlite, got: {msg}"
    );
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
