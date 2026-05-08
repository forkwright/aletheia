use super::*;
use crate::schema;

fn open_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    schema::init(&conn).expect("schema init");
    conn
}

#[test]
fn file_hash_is_deterministic() {
    let data = b"hello world";
    let h1 = file_hash(data);
    let h2 = file_hash(data);
    assert_eq!(h1, h2, "hash must be deterministic");
    assert_eq!(h1.len(), 64, "SHA-256 must be 64 hex chars");
    assert_eq!(
        h1, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        "must match known SHA-256 for 'hello world'"
    );
}

#[test]
fn file_hash_differs_for_different_content() {
    let h_foo = file_hash(b"foo");
    let h_bar = file_hash(b"bar");
    assert_ne!(
        h_foo, h_bar,
        "different inputs must produce different hashes"
    );

    let h_abc = file_hash(b"abc");
    let h_cba = file_hash(b"cba");
    assert_ne!(
        h_abc, h_cba,
        "different inputs must produce different hashes"
    );
}

#[test]
fn index_simple_fn() {
    let conn = open_test_db();
    let src = r"
            pub fn greet(name: &str) -> String {
                format!('Hello, {name}!')
            }
        ";
    // Write to a temp file.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src.replace('\'', "\"")).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();

    index_file(&conn, "test_crate", &path_str, "").expect("index");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE symbol_name = 'greet' AND symbol_kind = 'fn'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(count, 1, "expected 1 'greet' fn symbol");
}

#[test]
fn index_struct_and_impl_trait() {
    let conn = open_test_db();
    let src = r"
            pub struct Foo;
            pub trait Bar {}
            impl Bar for Foo {}
        ";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();

    index_file(&conn, "my_crate", &path_str, "").expect("index");

    let struct_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE symbol_name = 'Foo' AND symbol_kind = 'struct'",
            [],
            |r| r.get(0),
        )
        .expect("query struct");
    assert_eq!(struct_count, 1, "expected Foo struct");

    let impl_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE symbol_kind = 'impl'",
            [],
            |r| r.get(0),
        )
        .expect("query impl");
    assert_eq!(impl_count, 1, "expected 1 impl block");
}

#[test]
fn reindex_clears_old_symbols() {
    let conn = open_test_db();
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let path_str = tmp.path().to_string_lossy().into_owned();

    std::fs::write(tmp.path(), "pub fn alpha() {}").expect("write v1");
    index_file(&conn, "krate", &path_str, "").expect("index v1");

    let c1: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
        .expect("count v1");
    assert_eq!(c1, 1);

    // Overwrite with different content.
    std::fs::write(tmp.path(), "pub fn beta() {} pub fn gamma() {}").expect("write v2");
    index_file(&conn, "krate", &path_str, "").expect("index v2");

    let c2: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
        .expect("count v2");
    assert_eq!(c2, 2, "old symbols must be cleared on re-index");

    // Verify alpha is gone.
    let alpha: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE symbol_name = 'alpha'",
            [],
            |r| r.get(0),
        )
        .expect("count alpha");
    assert_eq!(alpha, 0, "alpha should have been removed");
}

#[test]
fn pub_use_produces_reexport_symbol() {
    let conn = open_test_db();
    let src = "pub use other_crate::SomeType;";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();

    index_file(&conn, "re_crate", &path_str, "").expect("index");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE symbol_kind = 'reexport'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(count, 1, "expected 1 reexport symbol for 'pub use'");
}

#[test]
fn pub_use_reexport_populates_to_crate() {
    let conn = open_test_db();
    let src = r"pub use hermeneus::types::Message;";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();

    index_file(&conn, "re_crate", &path_str, "").expect("index");

    let to_crate: String = conn
        .query_row(
            "SELECT to_crate FROM symbol_refs
                 JOIN symbols ON symbols.id = symbol_refs.from_symbol
                 WHERE symbols.symbol_kind = 'reexport'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(to_crate, "hermeneus", "reexport must record origin crate");
}

#[test]
fn pub_use_rename_populates_to_crate() {
    let conn = open_test_db();
    let src = r"pub use hermeneus::types::Message as Msg;";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();

    index_file(&conn, "re_crate", &path_str, "").expect("index");

    let to_crate: String = conn
        .query_row(
            "SELECT to_crate FROM symbol_refs
                 JOIN symbols ON symbols.id = symbol_refs.from_symbol
                 WHERE symbols.symbol_name = 'Msg'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        to_crate, "hermeneus",
        "rename reexport must record origin crate"
    );
}

#[test]
fn indexed_rdeps_cover_impl_and_reexport_edges_only() {
    let conn = open_test_db();
    let src = r"
        pub struct Local;
        impl hermeneus::types::Message for Local {}
        pub use hermeneus::types::Message;
    ";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();

    index_file(&conn, "re_crate", &path_str, "").expect("index");

    let impl_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbol_refs
                 WHERE to_crate = 'hermeneus' AND to_symbol = 'Message' AND ref_kind = 'impl'",
            [],
            |r| r.get(0),
        )
        .expect("query impl refs");
    let reexport_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbol_refs
                 WHERE to_crate = 'hermeneus' AND to_symbol = 'Message' AND ref_kind = 'reexport'",
            [],
            |r| r.get(0),
        )
        .expect("query reexport refs");
    let other_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbol_refs WHERE ref_kind NOT IN ('impl', 'reexport')",
            [],
            |r| r.get(0),
        )
        .expect("query other refs");

    assert_eq!(impl_count, 1, "expected one impl edge");
    assert_eq!(reexport_count, 1, "expected one reexport edge");
    assert_eq!(
        other_count, 0,
        "v1 index should emit only impl and reexport edges"
    );
}

#[test]
fn module_path_for_file_module() {
    let conn = open_test_db();
    let tmp = tempfile::tempdir().expect("tempdir");
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("create src");
    let foo_dir = src_dir.join("foo");
    std::fs::create_dir_all(&foo_dir).expect("create foo");
    let bar_rs = foo_dir.join("bar.rs");
    std::fs::write(&bar_rs, "pub fn inside_bar() {}\n").expect("write bar.rs");

    let path_str = bar_rs.to_string_lossy().into_owned();
    let module_path = module_path_from_file_path(&src_dir, &bar_rs);
    assert_eq!(module_path, "foo::bar");

    index_file(&conn, "test_crate", &path_str, &module_path).expect("index");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols
                 WHERE symbol_name = 'inside_bar' AND module_path = 'foo::bar'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(count, 1, "expected symbol inside foo::bar file module");
}

#[test]
fn rebuild_prunes_deleted_files() {
    let conn = open_test_db();
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();

    // Minimal workspace.
    std::fs::write(
        ws.join("Cargo.toml"),
        r#"
[workspace]
members = ["tmp_crate"]
resolver = "2"
"#,
    )
    .expect("write root Cargo.toml");

    let crate_dir = ws.join("tmp_crate");
    std::fs::create_dir_all(crate_dir.join("src")).expect("create src");
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        r#"
[package]
name = "tmp_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write crate Cargo.toml");

    let lib_rs = crate_dir.join("src/lib.rs");
    let foo_rs = crate_dir.join("src/foo.rs");
    std::fs::write(&lib_rs, "pub mod foo;\npub fn keep() {}").expect("write lib.rs");
    std::fs::write(&foo_rs, "pub fn vanish() {}").expect("write foo.rs");

    rebuild(&conn, ws).expect("first rebuild");

    let count_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE file_path = ?1",
            [&foo_rs.to_string_lossy()],
            |r| r.get(0),
        )
        .expect("count foo symbols before");
    assert_eq!(
        count_before, 1,
        "foo.rs symbol should exist before deletion"
    );

    // Delete foo.rs and rebuild.
    std::fs::remove_file(&foo_rs).expect("remove foo.rs");
    std::fs::write(&lib_rs, "pub fn keep() {}").expect("rewrite lib.rs");

    rebuild(&conn, ws).expect("second rebuild");

    let count_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE file_path = ?1",
            [&foo_rs.to_string_lossy()],
            |r| r.get(0),
        )
        .expect("count foo symbols after");
    assert_eq!(count_after, 0, "symbols for deleted file must be pruned");

    let hash_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_hashes WHERE file_path = ?1",
            [&foo_rs.to_string_lossy()],
            |r| r.get(0),
        )
        .expect("count foo hash after");
    assert_eq!(hash_after, 0, "file_hash for deleted file must be pruned");
}
