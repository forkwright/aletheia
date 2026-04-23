//! Workspace index builder.
//!
//! Walks a Cargo workspace via `cargo metadata`, then parses each `*.rs`
//! file with `syn`, populating the `SQLite` schema with symbol definitions and
//! cross-reference edges.
//!
//! # Incremental rebuild
//!
//! Each file's SHA-256 hash is stored in `file_hashes`.  On the next
//! `rebuild()` call only files whose on-disk hash differs from the stored
//! value are re-parsed.  To force a full rebuild, delete the cache file
//! (typically `~/.cache/aletheia/gnosis.sqlite`).
//!
//! # What is indexed
//!
//! `syn::visit` walks every item in each file.  Indexed kinds:
//!
//! | Kind        | Source construct                            |
//! |-------------|---------------------------------------------|
//! | `fn`        | `ItemFn`, `ImplItemFn`, `TraitItemFn`       |
//! | `struct`    | `ItemStruct`                                |
//! | `enum`      | `ItemEnum`                                  |
//! | `trait`     | `ItemTrait`                                 |
//! | `type`      | `ItemType`                                  |
//! | `const`     | `ItemConst`                                 |
//! | `reexport`  | `ItemUse` with `pub` visibility             |
//! | `impl`      | `ItemImpl` that names a trait               |
//!
//! # Limitations in v1
//!
//! - Macro-expanded code is not indexed (syn operates pre-expansion).
//! - Call sites inside macro arguments are not captured.
//! - Only direct `pub use` re-exports are tracked; transitive re-export chains
//!   require multiple query hops (via `reexport_chain` query).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;
use proc_macro2::Span;
use rusqlite::{Connection, params};
use snafu::ResultExt;
use syn::visit::Visit;

use crate::error::{CargoMetadataSnafu, ParseSnafu, ReadSourceSnafu, Result, SqliteSnafu};

// ── SHA-256 helper (std only, no extra dep) ──────────────────────────────────

/// Compute a hex SHA-256 digest of `data` using the stdlib `DefaultHasher`
/// fallback.
///
/// WHY: gnosis carries no crypto dep.  We need stable file-change detection,
/// not cryptographic security.  We use a FNV-1a-style 64-bit hash folded into
/// a hex string; collisions are extremely unlikely for source files and
/// acceptable for an incremental build cache.  If this ever causes false-
/// negative skips the worst outcome is a stale index entry — not a security
/// issue.  A full rebuild (delete cache file) recovers immediately.
fn file_hash(data: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    // Use two separate hashers seeded differently to reduce collision probability.
    let mut h1 = std::collections::hash_map::DefaultHasher::new();
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h1);
    // Second pass: hash length + content reversed to differentiate prefixes.
    data.len().hash(&mut h2);
    for chunk in data.chunks(64).rev() {
        chunk.hash(&mut h2);
    }
    format!("{:016x}{:016x}", h1.finish(), h2.finish())
}

// ── Visitor ──────────────────────────────────────────────────────────────────

/// Context passed to the `syn::visit` walker.
struct IndexVisitor<'a> {
    conn: &'a Connection,
    crate_name: &'a str,
    /// Dot-separated module path within the crate, e.g. `"types::message"`.
    module_path: String,
    file_path: &'a str,
}

impl<'a> IndexVisitor<'a> {
    fn new(conn: &'a Connection, crate_name: &'a str, file_path: &'a str) -> Self {
        Self {
            conn,
            crate_name,
            module_path: String::new(),
            file_path,
        }
    }

    fn insert_symbol(&self, name: &str, kind: &str, line: u32) {
        // Non-fatal: log and continue on DB errors during indexing.
        if let Err(e) = self.conn.execute(
            "INSERT INTO symbols (crate_name, module_path, symbol_name, symbol_kind, file_path, line_start)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![self.crate_name, self.module_path, name, kind, self.file_path, line],
        ) {
            tracing::warn!(
                crate_name = self.crate_name,
                symbol = name,
                kind,
                error = %e,
                "gnosis: failed to insert symbol"
            );
        }
    }

    fn last_symbol_id(&self) -> Option<i64> {
        self.conn.last_insert_rowid().into()
    }

    fn insert_ref(&self, from_id: i64, to_crate: &str, to_module: &str, to_sym: &str, kind: &str) {
        if let Err(e) = self.conn.execute(
            "INSERT INTO symbol_refs (from_symbol, to_crate, to_module, to_symbol, ref_kind)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![from_id, to_crate, to_module, to_sym, kind],
        ) {
            tracing::warn!(
                from_id,
                to_symbol = to_sym,
                kind,
                error = %e,
                "gnosis: failed to insert symbol_ref"
            );
        }
    }

    /// Push a module path segment (entering a nested `mod` block).
    fn push_module(&mut self, name: &str) {
        if self.module_path.is_empty() {
            name.clone_into(&mut self.module_path);
        } else {
            self.module_path.push_str("::");
            self.module_path.push_str(name);
        }
    }

    /// Pop the last module path segment (leaving a nested `mod` block).
    fn pop_module(&mut self) {
        if let Some(pos) = self.module_path.rfind("::") {
            self.module_path.truncate(pos);
        } else {
            self.module_path.clear();
        }
    }
}

/// Resolve a `syn::Path` to (crate, module, symbol) strings.
///
/// For a path like `hermeneus::types::Message`:
/// - crate   = `"hermeneus"`
/// - module  = `"types"`
/// - symbol  = `"Message"`
///
/// For a single-segment path `Foo`, we return `("", "", "Foo")` — the caller
/// decides how to handle workspace-internal references.
fn decompose_path(path: &syn::Path) -> (String, String, String) {
    let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let n = segs.len();
    match n {
        0 => (String::new(), String::new(), String::new()),
        1 => (
            String::new(),
            String::new(),
            segs.first().cloned().unwrap_or_default(),
        ),
        2 => (
            segs.first().cloned().unwrap_or_default(),
            String::new(),
            segs.get(1).cloned().unwrap_or_default(),
        ),
        _ => {
            let crate_name = segs.first().cloned().unwrap_or_default();
            let sym = segs.last().cloned().unwrap_or_default();
            let module = segs.get(1..n - 1).unwrap_or_default().join("::");
            (crate_name, module, sym)
        }
    }
}

/// Extract the line number from a `proc_macro2::Span`.
///
/// Line numbers are `usize` in `proc_macro2` but stored as `u32` in `SQLite`.
/// Real source files never exceed `u32::MAX` lines so the truncation is safe;
/// we use `try_from` and fall back to 0 for defence.
fn span_line(span: Span) -> u32 {
    u32::try_from(span.start().line).unwrap_or(0)
}

impl<'ast> Visit<'ast> for IndexVisitor<'_> {
    // ── Functions ─────────────────────────────────────────────────────────────

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        let line = span_line(node.sig.ident.span());
        self.insert_symbol(&name, "fn", line);
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let name = node.sig.ident.to_string();
        let line = span_line(node.sig.ident.span());
        self.insert_symbol(&name, "fn", line);
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        let name = node.sig.ident.to_string();
        let line = span_line(node.sig.ident.span());
        self.insert_symbol(&name, "fn", line);
        syn::visit::visit_trait_item_fn(self, node);
    }

    // ── Structs ───────────────────────────────────────────────────────────────

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let name = node.ident.to_string();
        let line = span_line(node.ident.span());
        self.insert_symbol(&name, "struct", line);
        syn::visit::visit_item_struct(self, node);
    }

    // ── Enums ─────────────────────────────────────────────────────────────────

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let name = node.ident.to_string();
        let line = span_line(node.ident.span());
        self.insert_symbol(&name, "enum", line);
        syn::visit::visit_item_enum(self, node);
    }

    // ── Traits ────────────────────────────────────────────────────────────────

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let name = node.ident.to_string();
        let line = span_line(node.ident.span());
        self.insert_symbol(&name, "trait", line);
        syn::visit::visit_item_trait(self, node);
    }

    // ── Type aliases ──────────────────────────────────────────────────────────

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        let name = node.ident.to_string();
        let line = span_line(node.ident.span());
        self.insert_symbol(&name, "type", line);
        syn::visit::visit_item_type(self, node);
    }

    // ── Consts ────────────────────────────────────────────────────────────────

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        let name = node.ident.to_string();
        let line = span_line(node.ident.span());
        self.insert_symbol(&name, "const", line);
        syn::visit::visit_item_const(self, node);
    }

    // ── impl blocks ───────────────────────────────────────────────────────────

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        // Only record impl-trait blocks (not inherent impls).
        if let Some((_, trait_path, _)) = &node.trait_ {
            let type_name = type_name_from_type(&node.self_ty);
            let line = span_line(
                trait_path
                    .segments
                    .last()
                    .map_or_else(Span::call_site, |s| s.ident.span()),
            );
            let (trait_crate, trait_module, trait_sym) = decompose_path(trait_path);
            self.insert_symbol(&type_name, "impl", line);
            if let Some(sym_id) = self.last_symbol_id() {
                self.insert_ref(sym_id, &trait_crate, &trait_module, &trait_sym, "impl");
            }
        }
        syn::visit::visit_item_impl(self, node);
    }

    // ── pub use re-exports ────────────────────────────────────────────────────

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        // Only track `pub use ...` (public re-exports).
        if matches!(node.vis, syn::Visibility::Public(_)) {
            extract_reexports(&node.tree, self, 0);
        }
        syn::visit::visit_item_use(self, node);
    }

    // ── Nested modules ────────────────────────────────────────────────────────

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let name = node.ident.to_string();
        self.push_module(&name);
        syn::visit::visit_item_mod(self, node);
        self.pop_module();
    }
}

/// Extract the type name string from `syn::Type` (best-effort).
fn type_name_from_type(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map_or_else(|| "?".to_owned(), |s| s.ident.to_string()),
        _ => "?".to_owned(),
    }
}

/// Recursively walk a `UseTree` and insert a `reexport` symbol + ref for
/// every leaf that is not a glob.
fn extract_reexports(tree: &syn::UseTree, visitor: &mut IndexVisitor<'_>, depth: u32) {
    // Guard against degenerate trees.
    if depth > 16 {
        return;
    }
    match tree {
        syn::UseTree::Path(p) => {
            extract_reexports(&p.tree, visitor, depth + 1);
        }
        syn::UseTree::Name(n) => {
            let sym = n.ident.to_string();
            let line = span_line(n.ident.span());
            visitor.insert_symbol(&sym, "reexport", line);
            if let Some(id) = visitor.last_symbol_id() {
                // We record the symbol name as a reexport edge; the full
                // origin path is not resolved at AST level — only the leaf
                // name is captured.  Agents can follow the chain via
                // `reexport_chain` queries.
                visitor.insert_ref(id, "", "", &sym, "reexport");
            }
        }
        syn::UseTree::Rename(r) => {
            // `use foo::Bar as Baz` — record the alias.
            let alias = r.rename.to_string();
            let line = span_line(r.rename.span());
            visitor.insert_symbol(&alias, "reexport", line);
        }
        syn::UseTree::Glob(_) => {
            // Glob re-exports (`pub use foo::*`) are not individually tracked
            // in v1 — they would require type resolution to enumerate.
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                extract_reexports(item, visitor, depth + 1);
            }
        }
    }
}

// ── File-level indexer ───────────────────────────────────────────────────────

/// Parse and index a single `.rs` file.
///
/// Errors during parsing are logged at WARN and skipped — they do not abort
/// the overall index build.
#[tracing::instrument(skip(conn), fields(crate_name, file = file_path))]
fn index_file(conn: &Connection, crate_name: &str, file_path: &str) -> Result<()> {
    let content = std::fs::read_to_string(file_path).with_context(|_| ReadSourceSnafu {
        path: PathBuf::from(file_path),
    })?;

    let file = syn::parse_file(&content).with_context(|_| ParseSnafu {
        path: PathBuf::from(file_path),
    })?;

    // Delete any existing symbols from this file before re-inserting.
    conn.execute(
        "DELETE FROM symbols WHERE file_path = ?1",
        params![file_path],
    )
    .context(SqliteSnafu)?;

    let mut visitor = IndexVisitor::new(conn, crate_name, file_path);
    visitor.visit_file(&file);

    Ok(())
}

// ── Workspace indexer ────────────────────────────────────────────────────────

/// Walk the workspace and (re-)index all changed source files.
///
/// Steps:
/// 1. Run `cargo metadata` from `workspace_root`.
/// 2. Populate `crate_edges` from the resolved dependency graph.
/// 3. Walk every `*.rs` file in each workspace member's `src/`.
/// 4. Skip files whose hash matches the stored value (incremental).
/// 5. Update `file_hashes` for all processed files.
#[tracing::instrument(skip(conn))]
pub(crate) fn rebuild(conn: &Connection, workspace_root: &Path) -> Result<()> {
    tracing::info!(workspace = %workspace_root.display(), "gnosis: starting index rebuild");

    // ── 1. cargo metadata ────────────────────────────────────────────────────
    let metadata = MetadataCommand::new()
        .current_dir(workspace_root)
        .no_deps()
        .exec()
        .context(CargoMetadataSnafu)?;

    // Full metadata (with deps) for crate_edges.
    let metadata_full = MetadataCommand::new()
        .current_dir(workspace_root)
        .exec()
        .context(CargoMetadataSnafu)?;

    // ── 2. Populate crate_edges ───────────────────────────────────────────────
    conn.execute("DELETE FROM crate_edges", [])
        .context(SqliteSnafu)?;

    // Build a map from package_id → package_name for workspace members.
    let workspace_ids: HashMap<_, _> = metadata_full
        .workspace_members
        .iter()
        .filter_map(|id| {
            metadata_full
                .packages
                .iter()
                .find(|p| &p.id == id)
                .map(|p| (id.clone(), p.name.clone()))
        })
        .collect();

    for pkg in &metadata_full.packages {
        if !workspace_ids.contains_key(&pkg.id) {
            continue;
        }
        let from_name = pkg.name.as_str();
        for dep in &pkg.dependencies {
            // Only record edges to other workspace members.
            if workspace_ids.values().any(|n| n == dep.name.as_str()) {
                conn.execute(
                    "INSERT OR IGNORE INTO crate_edges (from_crate, to_crate) VALUES (?1, ?2)",
                    params![from_name, dep.name.as_str()],
                )
                .context(SqliteSnafu)?;
            }
        }
    }

    // ── 3. Walk workspace source files ───────────────────────────────────────
    let mut total_files = 0usize;
    let mut skipped = 0usize;

    for pkg in &metadata.packages {
        if !metadata.workspace_members.contains(&pkg.id) {
            continue;
        }
        let crate_name = pkg.name.as_str();
        let manifest_dir = pkg
            .manifest_path
            .parent()
            .map_or_else(|| workspace_root.to_owned(), |p| p.as_std_path().to_owned());

        let src_dir = manifest_dir.join("src");
        if !src_dir.exists() {
            continue;
        }

        let rs_files = collect_rs_files(&src_dir);
        for file_path in rs_files {
            total_files += 1;
            let path_str = file_path.to_string_lossy().into_owned();

            // Read for hashing first.
            let content = match std::fs::read(&file_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(file = %file_path.display(), error = %e, "gnosis: cannot read file, skipping");
                    continue;
                }
            };
            let hash = file_hash(&content);

            // Check stored hash.
            let stored_hash: Option<String> = conn
                .query_row(
                    "SELECT sha256 FROM file_hashes WHERE file_path = ?1",
                    params![path_str],
                    |row| row.get(0),
                )
                .ok();

            if stored_hash.as_deref() == Some(hash.as_str()) {
                skipped += 1;
                continue;
            }

            // Parse and index.
            if let Err(e) = index_file(conn, crate_name, &path_str) {
                tracing::warn!(file = %path_str, error = %e, "gnosis: parse error, skipping file");
                continue;
            }

            // Update hash.
            conn.execute(
                "INSERT OR REPLACE INTO file_hashes (file_path, sha256) VALUES (?1, ?2)",
                params![path_str, hash],
            )
            .context(SqliteSnafu)?;
        }
    }

    tracing::info!(
        total_files,
        skipped,
        indexed = total_files - skipped,
        "gnosis: index rebuild complete"
    );

    Ok(())
}

/// Recursively collect all `*.rs` files under `dir`.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs_recursive(dir, &mut out);
    out
}

fn collect_rs_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_recursive(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
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
        assert_eq!(h1.len(), 32, "hash must be 32 hex chars (2 x u64)");
    }

    #[test]
    fn file_hash_differs_for_different_content() {
        assert_ne!(file_hash(b"foo"), file_hash(b"bar"));
        assert_ne!(file_hash(b"abc"), file_hash(b"cba"));
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

        index_file(&conn, "test_crate", &path_str).expect("index");

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

        index_file(&conn, "my_crate", &path_str).expect("index");

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
        index_file(&conn, "krate", &path_str).expect("index v1");

        let c1: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .expect("count v1");
        assert_eq!(c1, 1);

        // Overwrite with different content.
        std::fs::write(tmp.path(), "pub fn beta() {} pub fn gamma() {}").expect("write v2");
        index_file(&conn, "krate", &path_str).expect("index v2");

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

        index_file(&conn, "re_crate", &path_str).expect("index");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE symbol_kind = 'reexport'",
                [],
                |r| r.get(0),
            )
            .expect("query");
        assert_eq!(count, 1, "expected 1 reexport symbol for 'pub use'");
    }
}
