use super::*;
use crate::schema::Store;

fn open_test_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open fjall store");
    (store, dir)
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
    let (store, _dir) = open_test_store();
    let src = r"
            pub fn greet(name: &str) -> String {
                format!('Hello, {name}!')
            }
        ";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src.replace('\'', "\"")).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();
    let content = std::fs::read_to_string(tmp.path()).expect("read");

    index_file(&store, "test_crate", &path_str, "", &content).expect("index");

    let count = store
        .symbols()
        .expect("query")
        .into_iter()
        .filter(|symbol| symbol.symbol_name == "greet" && symbol.symbol_kind == "fn")
        .count();
    assert_eq!(count, 1, "expected 1 'greet' fn symbol");
}

#[test]
fn index_nested_fn_is_excluded() {
    let (store, _dir) = open_test_store();
    let src = r"
        pub fn outer() {
            fn inner_helper() {}
            inner_helper();
        }
    ";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();

    index_file(&store, "test_crate", &path_str, "", src).expect("index");

    let symbols = store.symbols().expect("query");
    let outer_count = symbols
        .iter()
        .filter(|symbol| symbol.symbol_name == "outer" && symbol.symbol_kind == "fn")
        .count();
    assert_eq!(outer_count, 1, "expected module-level outer fn");

    let inner_count = symbols
        .iter()
        .filter(|symbol| symbol.symbol_name == "inner_helper")
        .count();
    assert_eq!(
        inner_count, 0,
        "nested function must not appear as a module-level symbol"
    );
}

#[test]
fn index_struct_and_impl_trait() {
    let (store, _dir) = open_test_store();
    let src = r"
            pub struct Foo;
            pub trait Bar {}
            impl Bar for Foo {}
        ";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();
    let content = std::fs::read_to_string(tmp.path()).expect("read");

    index_file(&store, "my_crate", &path_str, "", &content).expect("index");

    let symbols = store.symbols().expect("query symbols");
    let struct_count = symbols
        .iter()
        .filter(|symbol| symbol.symbol_name == "Foo" && symbol.symbol_kind == "struct")
        .count();
    assert_eq!(struct_count, 1, "expected Foo struct");

    let impl_count = symbols
        .iter()
        .filter(|symbol| symbol.symbol_kind == "impl")
        .count();
    assert_eq!(impl_count, 1, "expected 1 impl block");
}

#[test]
fn reindex_clears_old_symbols() {
    let (store, _dir) = open_test_store();
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let path_str = tmp.path().to_string_lossy().into_owned();

    let v1 = "pub fn alpha() {}";
    std::fs::write(tmp.path(), v1).expect("write v1");
    index_file(&store, "krate", &path_str, "", v1).expect("index v1");

    let c1 = store.symbols().expect("count v1").len();
    assert_eq!(c1, 1);

    let v2 = "pub fn beta() {} pub fn gamma() {}";
    std::fs::write(tmp.path(), v2).expect("write v2");
    index_file(&store, "krate", &path_str, "", v2).expect("index v2");

    let symbols = store.symbols().expect("count v2");
    let c2 = symbols.len();
    assert_eq!(c2, 2, "old symbols must be cleared on re-index");

    let alpha = symbols
        .iter()
        .filter(|symbol| symbol.symbol_name == "alpha")
        .count();
    assert_eq!(alpha, 0, "alpha should have been removed");
}

#[test]
fn pub_use_produces_reexport_symbol() {
    let (store, _dir) = open_test_store();
    let src = "pub use other_crate::SomeType;";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();
    let content = std::fs::read_to_string(tmp.path()).expect("read");

    index_file(&store, "re_crate", &path_str, "", &content).expect("index");

    let count = store
        .symbols()
        .expect("query")
        .into_iter()
        .filter(|symbol| symbol.symbol_kind == "reexport")
        .count();
    assert_eq!(count, 1, "expected 1 reexport symbol for 'pub use'");
}

#[test]
fn pub_use_reexport_populates_to_crate() {
    let (store, _dir) = open_test_store();
    let src = r"pub use hermeneus::types::Message;";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();
    let content = std::fs::read_to_string(tmp.path()).expect("read");

    index_file(&store, "re_crate", &path_str, "", &content).expect("index");

    let reexport_ids: Vec<_> = store
        .symbols()
        .expect("symbols")
        .into_iter()
        .filter(|symbol| symbol.symbol_kind == "reexport")
        .map(|symbol| symbol.id)
        .collect();
    let to_crate = store
        .refs()
        .expect("refs")
        .into_iter()
        .find(|reference| reexport_ids.contains(&reference.from_symbol))
        .expect("reexport ref")
        .to_crate;
    assert_eq!(to_crate, "hermeneus", "reexport must record origin crate");
}

#[test]
fn pub_use_rename_populates_to_crate() {
    let (store, _dir) = open_test_store();
    let src = r"pub use hermeneus::types::Message as Msg;";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();
    let content = std::fs::read_to_string(tmp.path()).expect("read");

    index_file(&store, "re_crate", &path_str, "", &content).expect("index");

    let msg_id = store
        .symbols()
        .expect("symbols")
        .into_iter()
        .find(|symbol| symbol.symbol_name == "Msg")
        .expect("Msg symbol")
        .id;
    let to_crate = store
        .refs()
        .expect("refs")
        .into_iter()
        .find(|reference| reference.from_symbol == msg_id)
        .expect("Msg ref")
        .to_crate;
    assert_eq!(
        to_crate, "hermeneus",
        "rename reexport must record origin crate"
    );
}

#[test]
fn indexed_rdeps_cover_impl_and_reexport_edges_only() {
    let (store, _dir) = open_test_store();
    let src = r"
        pub struct Local;
        impl hermeneus::types::Message for Local {}
        pub use hermeneus::types::Message;
    ";
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), src).expect("write");
    let path_str = tmp.path().to_string_lossy().into_owned();
    let content = std::fs::read_to_string(tmp.path()).expect("read");

    index_file(&store, "re_crate", &path_str, "", &content).expect("index");

    let refs = store.refs().expect("refs");
    let impl_count = refs
        .iter()
        .filter(|reference| {
            reference.to_crate == "hermeneus"
                && reference.to_symbol == "Message"
                && reference.ref_kind == "impl"
        })
        .count();
    let reexport_count = refs
        .iter()
        .filter(|reference| {
            reference.to_crate == "hermeneus"
                && reference.to_symbol == "Message"
                && reference.ref_kind == "reexport"
        })
        .count();
    let other_count = refs
        .iter()
        .filter(|reference| reference.ref_kind != "impl" && reference.ref_kind != "reexport")
        .count();

    assert_eq!(impl_count, 1, "expected one impl edge");
    assert_eq!(reexport_count, 1, "expected one reexport edge");
    assert_eq!(
        other_count, 0,
        "v1 index should emit only impl and reexport edges"
    );
}

#[test]
fn module_path_for_file_module() {
    let (store, _dir) = open_test_store();
    let tmp = tempfile::tempdir().expect("tempdir");
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("create src");
    let foo_dir = src_dir.join("foo");
    std::fs::create_dir_all(&foo_dir).expect("create foo");
    let bar_rs = foo_dir.join("bar.rs");
    let content = "pub fn inside_bar() {}\n";
    std::fs::write(&bar_rs, content).expect("write bar.rs");

    let path_str = bar_rs.to_string_lossy().into_owned();
    let module_path = module_path_from_file_path(&src_dir, &bar_rs);
    assert_eq!(module_path, "foo::bar");

    index_file(&store, "test_crate", &path_str, &module_path, content).expect("index");

    let count = store
        .symbols()
        .expect("query")
        .into_iter()
        .filter(|symbol| symbol.symbol_name == "inside_bar" && symbol.module_path == "foo::bar")
        .count();
    assert_eq!(count, 1, "expected symbol inside foo::bar file module");
}

#[test]
fn index_file_uses_passed_content_not_disk_content() {
    let (store, _dir) = open_test_store();
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let path_str = tmp.path().to_string_lossy().into_owned();

    // WHY: the old implementation re-read the file from disk inside index_file,
    // creating a TOCTOU window between hash computation and parsing. After the
    // refactor, index_file must index the exact content passed to it.
    std::fs::write(tmp.path(), "pub fn alpha() {}").expect("write disk");
    index_file(&store, "krate", &path_str, "", "pub fn beta() {}").expect("index passed content");

    let symbols = store.symbols().expect("query");
    let beta_count = symbols
        .iter()
        .filter(|symbol| symbol.symbol_name == "beta" && symbol.symbol_kind == "fn")
        .count();
    let alpha_count = symbols
        .iter()
        .filter(|symbol| symbol.symbol_name == "alpha" && symbol.symbol_kind == "fn")
        .count();
    assert_eq!(beta_count, 1, "must index symbol from passed content");
    assert_eq!(alpha_count, 0, "must not re-read file from disk");
}

#[test]
fn rebuild_prunes_deleted_files() {
    let (store, _dir) = open_test_store();
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();

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

    rebuild(&store, ws).expect("first rebuild");

    let foo_path = foo_rs.to_string_lossy();
    let count_before = store
        .symbols()
        .expect("count foo symbols before")
        .into_iter()
        .filter(|symbol| symbol.file_path == foo_path)
        .count();
    assert_eq!(
        count_before, 1,
        "foo.rs symbol should exist before deletion"
    );

    std::fs::remove_file(&foo_rs).expect("remove foo.rs");
    std::fs::write(&lib_rs, "pub fn keep() {}").expect("rewrite lib.rs");

    rebuild(&store, ws).expect("second rebuild");

    let count_after = store
        .symbols()
        .expect("count foo symbols after")
        .into_iter()
        .filter(|symbol| symbol.file_path == foo_path)
        .count();
    assert_eq!(count_after, 0, "symbols for deleted file must be pruned");

    let hash_after = store.file_hash(&foo_path).expect("count foo hash after");
    assert!(
        hash_after.is_none(),
        "file_hash for deleted file must be pruned"
    );
}
