//! Integration tests for the public `Db` facade: delegated methods and error behavior.
#![cfg(feature = "engine-tests")]

use std::collections::BTreeMap;

use aletheia_mneme::engine::{DataValue, Db, ScriptMutability};

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
