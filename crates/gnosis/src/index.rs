//! Workspace index builder.
//!
//! Walks a Cargo workspace via `cargo metadata`, then parses each `*.rs`
//! file with `syn`, populating the fjall index with symbol definitions and
//! cross-reference edges.
//!
//! # Incremental rebuild
//!
//! Each file's SHA-256 hash is stored in `file_hashes`.  On the next
//! `rebuild()` call only files whose on-disk hash differs from the stored
//! value are re-parsed.  To force a full rebuild, delete the cache file
//! (typically `~/.cache/aletheia/gnosis.fjall`).
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
//! - Nested functions defined inside another function body are not indexed.
//! - Only direct `pub use` re-exports are tracked; transitive re-export chains
//!   require multiple query hops (via `reexport_chain` query).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use cargo_metadata::MetadataCommand;
use proc_macro2::Span;
use snafu::ResultExt;
use syn::visit::Visit;

use crate::error::{CargoMetadataSnafu, ParseSnafu, Result};
use crate::schema::Store;

// ── SHA-256 helper ───────────────────────────────────────────────────────────

/// Compute a hex SHA-256 digest of `data`.
fn file_hash(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            let hi = HEX.get(usize::from(b >> 4)).copied().unwrap_or(b'0');
            let lo = HEX.get(usize::from(b & 0x0f)).copied().unwrap_or(b'0');
            acc.push(char::from(hi));
            acc.push(char::from(lo));
            acc
        })
}

// ── Visitor ──────────────────────────────────────────────────────────────────

/// Context passed to the `syn::visit` walker.
struct IndexVisitor<'a> {
    store: &'a Store,
    crate_name: &'a str,
    /// Dot-separated module path within the crate, e.g. `"types::message"`.
    module_path: String,
    file_path: &'a str,
    last_symbol_id: Option<u64>,
    /// Number of nested function bodies we are currently inside.
    ///
    /// Only module-level functions (`scope_depth == 0`) are recorded as
    /// symbols; nested helpers defined inside another function body are
    /// skipped.
    scope_depth: usize,
    trait_item_visibility: Vec<bool>,
}

impl<'a> IndexVisitor<'a> {
    fn new(store: &'a Store, crate_name: &'a str, file_path: &'a str, module_path: String) -> Self {
        Self {
            store,
            crate_name,
            module_path,
            file_path,
            last_symbol_id: None,
            scope_depth: 0,
            trait_item_visibility: Vec::new(),
        }
    }

    fn last_symbol_id(&self) -> Option<u64> {
        self.last_symbol_id
    }

    fn record_symbol(&mut self, name: &str, kind: &str, line: u32) {
        match self.store.insert_symbol(
            self.crate_name,
            &self.module_path,
            name,
            kind,
            self.file_path,
            i64::from(line),
        ) {
            Ok(id) => self.last_symbol_id = Some(id),
            Err(e) => {
                self.last_symbol_id = None;
                tracing::warn!(
                    crate_name = self.crate_name,
                    symbol = name,
                    kind,
                    error = %e,
                    "gnosis: failed to insert symbol"
                );
            }
        }
    }

    fn insert_ref(&self, from_id: u64, to_crate: &str, to_module: &str, to_sym: &str, kind: &str) {
        if let Err(e) = self
            .store
            .insert_ref(from_id, to_crate, to_module, to_sym, kind)
        {
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

    fn trait_items_are_publicish(&self) -> bool {
        self.trait_item_visibility.last().copied().unwrap_or(false)
    }
}

fn is_publicish_visibility(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(restricted) => restricted.path.is_ident("crate"),
        syn::Visibility::Inherited => false,
    }
}

/// Resolve a slice of path segments to (crate, module, symbol) strings.
fn decompose_segments(segs: &[String]) -> (String, String, String) {
    match segs.split_last() {
        None => (String::new(), String::new(), String::new()),
        Some((sym, [])) => (String::new(), String::new(), sym.clone()),
        Some((sym, [crate_name])) => (crate_name.clone(), String::new(), sym.clone()),
        Some((sym, [crate_name, module @ ..])) => {
            (crate_name.clone(), module.join("::"), sym.clone())
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
    decompose_segments(&segs)
}

/// Extract the line number from a `proc_macro2::Span`.
///
/// Line numbers are `usize` in `proc_macro2` but stored as `u32` in fjall.
/// Real source files never exceed `u32::MAX` lines so the truncation is safe;
/// we use `try_from` and fall back to 0 for defence.
fn span_line(span: Span) -> u32 {
    u32::try_from(span.start().line).unwrap_or(0)
}

impl<'ast> Visit<'ast> for IndexVisitor<'_> {
    // ── Functions ─────────────────────────────────────────────────────────────

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if self.scope_depth == 0 && is_publicish_visibility(&node.vis) {
            let name = node.sig.ident.to_string();
            let line = span_line(node.sig.ident.span());
            self.record_symbol(&name, "fn", line);
        }
        self.scope_depth += 1;
        syn::visit::visit_item_fn(self, node);
        self.scope_depth -= 1;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if self.scope_depth == 0 && is_publicish_visibility(&node.vis) {
            let name = node.sig.ident.to_string();
            let line = span_line(node.sig.ident.span());
            self.record_symbol(&name, "fn", line);
        }
        self.scope_depth += 1;
        syn::visit::visit_impl_item_fn(self, node);
        self.scope_depth -= 1;
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if self.scope_depth == 0 && self.trait_items_are_publicish() {
            let name = node.sig.ident.to_string();
            let line = span_line(node.sig.ident.span());
            self.record_symbol(&name, "fn", line);
        }
        self.scope_depth += 1;
        syn::visit::visit_trait_item_fn(self, node);
        self.scope_depth -= 1;
    }

    // ── Structs ───────────────────────────────────────────────────────────────

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        if is_publicish_visibility(&node.vis) {
            let name = node.ident.to_string();
            let line = span_line(node.ident.span());
            self.record_symbol(&name, "struct", line);
        }
        syn::visit::visit_item_struct(self, node);
    }

    // ── Enums ─────────────────────────────────────────────────────────────────

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        if is_publicish_visibility(&node.vis) {
            let name = node.ident.to_string();
            let line = span_line(node.ident.span());
            self.record_symbol(&name, "enum", line);
        }
        syn::visit::visit_item_enum(self, node);
    }

    // ── Traits ────────────────────────────────────────────────────────────────

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let is_publicish = is_publicish_visibility(&node.vis);
        if is_publicish {
            let name = node.ident.to_string();
            let line = span_line(node.ident.span());
            self.record_symbol(&name, "trait", line);
        }
        self.trait_item_visibility.push(is_publicish);
        syn::visit::visit_item_trait(self, node);
        self.trait_item_visibility.pop();
    }

    // ── Type aliases ──────────────────────────────────────────────────────────

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        if is_publicish_visibility(&node.vis) {
            let name = node.ident.to_string();
            let line = span_line(node.ident.span());
            self.record_symbol(&name, "type", line);
        }
        syn::visit::visit_item_type(self, node);
    }

    // ── Consts ────────────────────────────────────────────────────────────────

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        if is_publicish_visibility(&node.vis) {
            let name = node.ident.to_string();
            let line = span_line(node.ident.span());
            self.record_symbol(&name, "const", line);
        }
        syn::visit::visit_item_const(self, node);
    }

    // ── impl blocks ───────────────────────────────────────────────────────────

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        // NOTE: only impl-trait blocks are recorded (not inherent impls).
        if let Some((_, trait_path, _)) = &node.trait_ {
            let type_name = type_name_from_type(&node.self_ty);
            let line = span_line(
                trait_path
                    .segments
                    .last()
                    .map_or_else(Span::call_site, |s| s.ident.span()),
            );
            let (trait_crate, trait_module, trait_sym) = decompose_path(trait_path);
            self.record_symbol(&type_name, "impl", line);
            if let Some(sym_id) = self.last_symbol_id() {
                self.insert_ref(sym_id, &trait_crate, &trait_module, &trait_sym, "impl");
            }
        }
        syn::visit::visit_item_impl(self, node);
    }

    // ── pub use re-exports ────────────────────────────────────────────────────

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if is_publicish_visibility(&node.vis) {
            extract_reexports(&node.tree, self, &mut Vec::new());
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
///
/// `prefix` accumulates the path segments from the root of the use tree.
fn extract_reexports(
    tree: &syn::UseTree,
    visitor: &mut IndexVisitor<'_>,
    prefix: &mut Vec<String>,
) {
    if prefix.len() > 16 {
        return;
    }
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            extract_reexports(&p.tree, visitor, prefix);
            prefix.pop();
        }
        syn::UseTree::Name(n) => {
            let sym = n.ident.to_string();
            let line = span_line(n.ident.span());
            visitor.record_symbol(&sym, "reexport", line);
            if let Some(id) = visitor.last_symbol_id() {
                prefix.push(sym.clone());
                let (to_crate, to_module, to_sym) = decompose_segments(prefix);
                visitor.insert_ref(id, &to_crate, &to_module, &to_sym, "reexport");
                prefix.pop();
            }
        }
        syn::UseTree::Rename(r) => {
            let alias = r.rename.to_string();
            let line = span_line(r.rename.span());
            visitor.record_symbol(&alias, "reexport", line);
            if let Some(id) = visitor.last_symbol_id() {
                let original = r.ident.to_string();
                prefix.push(original);
                let (to_crate, to_module, to_sym) = decompose_segments(prefix);
                visitor.insert_ref(id, &to_crate, &to_module, &to_sym, "reexport");
                prefix.pop();
            }
        }
        syn::UseTree::Glob(_) => {
            // NOTE: glob re-exports (`pub use foo::*`) are not individually
            // tracked in v1 — they would require type resolution to enumerate.
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                extract_reexports(item, visitor, prefix);
            }
        }
    }
}

/// Derive the dot-separated module path from a file path relative to `src_dir`.
///
/// | File path              | Module path |
/// |------------------------|-------------|
/// | `src/lib.rs`           | `""`        |
/// | `src/main.rs`          | `""`        |
/// | `src/foo.rs`           | `"foo"`     |
/// | `src/foo/mod.rs`       | `"foo"`     |
/// | `src/foo/bar.rs`       | `"foo::bar"`|
/// | `src/foo/bar/mod.rs`   | `"foo::bar"`|
fn module_path_from_file_path(src_dir: &Path, file_path: &Path) -> String {
    let rel = file_path.strip_prefix(src_dir).unwrap_or(file_path);
    let mut components: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    if let Some(last) = components.pop() {
        if last == "lib.rs" || last == "main.rs" {
            String::new()
        } else if last == "mod.rs" {
            components.join("::")
        } else if let Some(stem) = last.strip_suffix(".rs") {
            components.push(stem);
            components.join("::")
        } else {
            String::new()
        }
    } else {
        String::new()
    }
}

// ── File-level indexer ───────────────────────────────────────────────────────

/// Parse and index a single `.rs` file from pre-read content.
///
/// `content` is the exact source bytes (as UTF-8) used for both hashing and
/// parsing, eliminating the TOCTOU window between the two operations.
/// Errors during parsing are logged at WARN and skipped — they do not abort
/// the overall index build.
#[tracing::instrument(skip(store, content), fields(crate_name, file = file_path))]
fn index_file(
    store: &Store,
    crate_name: &str,
    file_path: &str,
    module_path: &str,
    content: &str,
) -> Result<()> {
    let file = syn::parse_file(content).with_context(|_| ParseSnafu {
        path: PathBuf::from(file_path),
    })?;

    store.delete_symbols_for_file(file_path)?;

    let mut visitor = IndexVisitor::new(store, crate_name, file_path, module_path.to_owned());
    visitor.visit_file(&file);

    Ok(())
}

/// Populate `crate_edges` from `cargo metadata` dependency graph.
fn populate_crate_edges(store: &Store, metadata: &cargo_metadata::Metadata) -> Result<()> {
    store.clear_crate_edges()?;

    let workspace_ids: HashMap<_, _> = metadata
        .workspace_members
        .iter()
        .filter_map(|id| {
            metadata
                .packages
                .iter()
                .find(|p| &p.id == id)
                .map(|p| (id.clone(), p.name.clone()))
        })
        .collect();

    for pkg in &metadata.packages {
        if !workspace_ids.contains_key(&pkg.id) {
            continue;
        }
        let from_name = pkg.name.as_str();
        for dep in &pkg.dependencies {
            if workspace_ids.values().any(|n| n == dep.name.as_str()) {
                store.insert_crate_edge(from_name, dep.name.as_str())?;
            }
        }
    }

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
///
/// Time: O(P + F + B), where P is workspace packages, F is source files, and B
/// is bytes read from changed files. Space: O(F) for the present-file set.
#[tracing::instrument(skip(store))]
pub(crate) fn rebuild(store: &Store, workspace_root: &Path) -> Result<()> {
    tracing::info!(workspace = %workspace_root.display(), "gnosis: starting index rebuild");

    // ── 1. cargo metadata ────────────────────────────────────────────────────
    let metadata = MetadataCommand::new()
        .current_dir(workspace_root)
        .no_deps()
        .exec()
        .context(CargoMetadataSnafu)?;

    // ── 2. Populate crate_edges ───────────────────────────────────────────────
    populate_crate_edges(store, &metadata)?;

    // ── 3. Walk workspace source files ───────────────────────────────────────
    let mut total_files = 0usize;
    let mut skipped = 0usize;
    let mut all_present_paths: Vec<String> = Vec::new();

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
            let present_path = file_path.to_string_lossy().into_owned();
            let path_str = present_path.as_str();

            let content = match std::fs::read(&file_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(file = %file_path.display(), error = %e, "gnosis: cannot read file, skipping");
                    all_present_paths.push(present_path);
                    continue;
                }
            };
            let hash = file_hash(&content);

            let stored_hash = store.file_hash(path_str)?;

            if stored_hash.as_deref() == Some(hash.as_str()) {
                skipped += 1;
                all_present_paths.push(present_path);
                continue;
            }

            let module_path = module_path_from_file_path(&src_dir, &file_path);
            let content_str = match std::str::from_utf8(&content) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(file = %path_str, error = %e, "gnosis: source file is not valid UTF-8, skipping");
                    all_present_paths.push(present_path);
                    continue;
                }
            };
            if let Err(e) = index_file(store, crate_name, path_str, &module_path, content_str) {
                tracing::warn!(file = %path_str, error = %e, "gnosis: parse error, skipping file");
                all_present_paths.push(present_path);
                continue;
            }

            store.set_file_hash(path_str, &hash)?;
            all_present_paths.push(present_path);
        }
    }

    // ── 4. Prune deleted files ────────────────────────────────────────────────
    prune_deleted_files(store, &all_present_paths)?;
    store.persist()?;

    tracing::info!(
        total_files,
        skipped,
        indexed = total_files - skipped,
        "gnosis: index rebuild complete"
    );

    Ok(())
}

/// Remove symbols and `file_hashes` for source files no longer on disk.
fn prune_deleted_files(store: &Store, present: &[String]) -> Result<()> {
    let present_set: std::collections::BTreeSet<&str> =
        present.iter().map(String::as_str).collect();
    let file_paths: Vec<String> = store
        .symbols()?
        .into_iter()
        .map(|symbol| symbol.file_path)
        .collect();
    for file_path in file_paths {
        if !present_set.contains(file_path.as_str()) {
            store.delete_symbols_for_file(&file_path)?;
        }
    }
    store.prune_file_hashes_not_in(present)?;
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
#[path = "index_tests.rs"]
mod tests;
