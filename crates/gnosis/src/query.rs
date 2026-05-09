//! Query implementations against the gnosis fjall index.
//!
//! All queries are synchronous and scan small local keyspaces. The public API
//! returns `Vec<QueryRow>`, a common result type that serialises cleanly to
//! JSON for MCP tool output.
//!
//! # Available queries
//!
//! | Query            | Answers                                                        |
//! |------------------|----------------------------------------------------------------|
//! | `symbol_rdeps`   | Which symbols implement or re-export a target symbol?          |
//! | `impl_search`    | Which types implement a given trait?                           |
//! | `reexport_chain` | Which crates re-export a given symbol name?                    |
//! | `crate_deps`     | What crates does a given crate directly depend on?             |
//! | `crate_rdeps`    | What crates directly depend on a given crate?                  |
//! | `symbols_in`     | List all symbols in a given crate, optionally filtered by kind. |
//!
//! # Epistemic tier
//!
//! Results are machine-derived from the AST and carry `EpistemicTier::Inferred`
//! confidence. They reflect the source as of the last `CodeGraph::rebuild()`
//! call, not necessarily the current on-disk state. The `source` field of
//! every `QueryRow` records the gnosis schema version so callers can detect
//! staleness.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::schema::{SCHEMA_VERSION, Store, SymbolRecord};

/// A single row in a query result.
///
/// All fields are `Option<String>` so the same struct covers every query type.
/// Fields not relevant to a given query are `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QueryRow {
    /// The crate that contains this result entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    /// Module path within the crate (e.g. `"types::message"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_path: Option<String>,
    /// Symbol name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    /// Symbol kind (`fn`, `struct`, `enum`, `trait`, `impl`, `reexport`, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    /// Absolute path to the source file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// 1-based line number of the definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<i64>,
    /// Reference kind for edge-type results (`impl`, `reexport`).
    /// In v1 only `impl` and `reexport` edges are indexed; call-site and
    /// type-use edges are deferred to v2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_kind: Option<String>,
    /// Provenance: `"gnosis@<schema_version>"`.
    pub source: String,
}

impl QueryRow {
    fn new(schema_ver: u32) -> Self {
        Self {
            crate_name: None,
            module_path: None,
            symbol_name: None,
            symbol_kind: None,
            file_path: None,
            line_start: None,
            ref_kind: None,
            source: format!("gnosis@{schema_ver}"),
        }
    }
}

// ── Query: symbol_rdeps ───────────────────────────────────────────────────────

/// Return symbols that implement or re-export `symbol_name` (optionally filtered
/// to a specific target `crate_name`).
///
/// Corresponds to the `symbol_rdeps` MCP operation.
///
/// # Arguments
///
/// - `symbol_name` — the symbol to look up (e.g. `"Message"`).
/// - `target_crate` — if `Some`, only return refs where `to_crate` matches.
#[tracing::instrument(skip(store))]
pub(crate) fn symbol_rdeps(
    store: &Store,
    symbol_name: &str,
    target_crate: Option<&str>,
) -> Result<Vec<QueryRow>> {
    let mut rows = Vec::new();
    for reference in store.refs()? {
        if reference.to_symbol != symbol_name {
            continue;
        }
        if target_crate.is_some_and(|crate_name| reference.to_crate != crate_name) {
            continue;
        }
        if reference.ref_kind != "impl" && reference.ref_kind != "reexport" {
            continue;
        }
        if let Some(symbol) = store.symbol(reference.from_symbol)? {
            let mut row = row_from_symbol(&symbol);
            row.ref_kind = Some(reference.ref_kind);
            rows.push(row);
        }
    }
    rows.sort_by(|left, right| {
        (
            left.crate_name.as_deref(),
            left.module_path.as_deref(),
            left.symbol_name.as_deref(),
        )
            .cmp(&(
                right.crate_name.as_deref(),
                right.module_path.as_deref(),
                right.symbol_name.as_deref(),
            ))
    });
    Ok(rows)
}

// ── Query: impl_search ────────────────────────────────────────────────────────

/// Find all `impl <trait> for <type>` blocks in the workspace.
///
/// Returns the implementing type, its crate, file, and line.
///
/// # Arguments
///
/// - `trait_name` — the trait name to search for (e.g. `"Stamped"`).
///   Matched against `to_symbol` in `symbol_refs` where `ref_kind = 'impl'`.
#[tracing::instrument(skip(store))]
pub(crate) fn impl_search(store: &Store, trait_name: &str) -> Result<Vec<QueryRow>> {
    let mut rows = Vec::new();
    for reference in store.refs()? {
        if reference.ref_kind == "impl"
            && reference.to_symbol == trait_name
            && let Some(symbol) = store.symbol(reference.from_symbol)?
        {
            let mut row = row_from_symbol(&symbol);
            row.symbol_kind = Some("impl".to_owned());
            row.ref_kind = Some("impl".to_owned());
            rows.push(row);
        }
    }
    rows.sort_by(|left, right| {
        (left.crate_name.as_deref(), left.symbol_name.as_deref())
            .cmp(&(right.crate_name.as_deref(), right.symbol_name.as_deref()))
    });
    Ok(rows)
}

// ── Query: reexport_chain ─────────────────────────────────────────────────────

/// Find all crates that re-export `symbol_name` via `pub use`.
///
/// Returns the crate, module, file, and line of each `pub use` site.
///
/// # Arguments
///
/// - `symbol_name` — the symbol name to look up (e.g. `"Message"`).
#[tracing::instrument(skip(store))]
pub(crate) fn reexport_chain(store: &Store, symbol_name: &str) -> Result<Vec<QueryRow>> {
    let mut rows: Vec<QueryRow> = store
        .symbols()?
        .into_iter()
        .filter(|symbol| symbol.symbol_name == symbol_name && symbol.symbol_kind == "reexport")
        .map(|symbol| {
            let mut row = row_from_symbol(&symbol);
            row.ref_kind = Some("reexport".to_owned());
            row
        })
        .collect();
    rows.sort_by(|left, right| {
        (left.crate_name.as_deref(), left.module_path.as_deref())
            .cmp(&(right.crate_name.as_deref(), right.module_path.as_deref()))
    });
    Ok(rows)
}

// ── Query: crate_deps ─────────────────────────────────────────────────────────

/// Return the direct workspace dependencies of `crate_name`.
#[tracing::instrument(skip(store))]
pub(crate) fn crate_deps(store: &Store, crate_name: &str) -> Result<Vec<QueryRow>> {
    let mut rows: Vec<QueryRow> = store
        .crate_edges()?
        .into_iter()
        .filter(|(from_crate, _to_crate)| from_crate == crate_name)
        .map(|(_from_crate, to_crate)| {
            let mut row = QueryRow::new(SCHEMA_VERSION);
            row.crate_name = Some(to_crate);
            row
        })
        .collect();
    rows.sort_by(|left, right| left.crate_name.cmp(&right.crate_name));
    Ok(rows)
}

// ── Query: crate_rdeps ────────────────────────────────────────────────────────

/// Return the workspace crates that directly depend on `crate_name`.
#[tracing::instrument(skip(store))]
pub(crate) fn crate_rdeps(store: &Store, crate_name: &str) -> Result<Vec<QueryRow>> {
    let mut rows: Vec<QueryRow> = store
        .crate_edges()?
        .into_iter()
        .filter(|(_from_crate, to_crate)| to_crate == crate_name)
        .map(|(from_crate, _to_crate)| {
            let mut row = QueryRow::new(SCHEMA_VERSION);
            row.crate_name = Some(from_crate);
            row
        })
        .collect();
    rows.sort_by(|left, right| left.crate_name.cmp(&right.crate_name));
    Ok(rows)
}

// ── Query: symbols_in ─────────────────────────────────────────────────────────

/// List all symbols in `crate_name`, optionally filtered to `kind`.
#[tracing::instrument(skip(store))]
pub(crate) fn symbols_in(
    store: &Store,
    crate_name: &str,
    kind: Option<&str>,
) -> Result<Vec<QueryRow>> {
    let mut rows: Vec<QueryRow> = store
        .symbols()?
        .into_iter()
        .filter(|symbol| symbol.crate_name == crate_name)
        .filter(|symbol| kind.is_none_or(|wanted| symbol.symbol_kind == wanted))
        .map(|symbol| row_from_symbol(&symbol))
        .collect();
    rows.sort_by(|left, right| {
        (left.module_path.as_deref(), left.symbol_name.as_deref())
            .cmp(&(right.module_path.as_deref(), right.symbol_name.as_deref()))
    });
    Ok(rows)
}

fn row_from_symbol(symbol: &SymbolRecord) -> QueryRow {
    let mut row = QueryRow::new(SCHEMA_VERSION);
    row.crate_name = Some(symbol.crate_name.clone());
    row.module_path = Some(symbol.module_path.clone());
    row.symbol_name = Some(symbol.symbol_name.clone());
    row.symbol_kind = Some(symbol.symbol_kind.clone());
    row.file_path = Some(symbol.file_path.clone());
    row.line_start = Some(symbol.line_start);
    row
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn open_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("open fjall store");
        (store, dir)
    }

    fn insert_symbol(
        store: &Store,
        crate_name: &str,
        module: &str,
        name: &str,
        kind: &str,
        file: &str,
        line: i64,
    ) -> u64 {
        store
            .insert_symbol(crate_name, module, name, kind, file, line)
            .expect("insert symbol")
    }

    fn insert_ref(
        store: &Store,
        from_id: u64,
        to_crate: &str,
        to_module: &str,
        to_sym: &str,
        kind: &str,
    ) {
        store
            .insert_ref(from_id, to_crate, to_module, to_sym, kind)
            .expect("insert ref");
    }

    fn insert_edge(store: &Store, from: &str, to: &str) {
        store
            .insert_crate_edge(from, to)
            .expect("insert crate edge");
    }

    #[test]
    fn symbol_rdeps_empty_when_no_refs() {
        let (store, _tmp) = open_store();
        let rows = symbol_rdeps(&store, "Message", None).expect("query");
        assert!(rows.is_empty());
    }

    #[test]
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    fn symbol_rdeps_finds_single_caller() {
        let (store, _tmp) = open_store();
        let sym_id = insert_symbol(
            &store,
            "nous",
            "execute",
            "dispatch",
            "fn",
            "nous/src/execute.rs",
            10,
        );
        insert_ref(&store, sym_id, "hermeneus", "types", "Message", "impl");

        let rows = symbol_rdeps(&store, "Message", None).expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol_name.as_deref(), Some("dispatch"));
        assert_eq!(rows[0].crate_name.as_deref(), Some("nous"));
    }

    #[test]
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    fn symbol_rdeps_filters_by_target_crate() {
        let (store, _tmp) = open_store();
        let id1 = insert_symbol(&store, "nous", "", "fn_a", "fn", "a.rs", 1);
        insert_ref(&store, id1, "hermeneus", "types", "Message", "reexport");
        let id2 = insert_symbol(&store, "melete", "", "fn_b", "fn", "b.rs", 2);
        insert_ref(&store, id2, "other", "", "Message", "impl");

        let all = symbol_rdeps(&store, "Message", None).expect("query all");
        assert_eq!(all.len(), 2);

        let filtered = symbol_rdeps(&store, "Message", Some("hermeneus")).expect("query filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].crate_name.as_deref(), Some("nous"));
    }

    #[test]
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    fn symbol_rdeps_includes_reexports_with_target_crate_filter() {
        let (store, _tmp) = open_store();
        let id1 = insert_symbol(
            &store, "organon", "prelude", "Message", "reexport", "a.rs", 1,
        );
        insert_ref(&store, id1, "hermeneus", "types", "Message", "reexport");

        let all = symbol_rdeps(&store, "Message", None).expect("query all");
        assert_eq!(all.len(), 1);

        let filtered = symbol_rdeps(&store, "Message", Some("hermeneus")).expect("query filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].symbol_name.as_deref(), Some("Message"));
        assert_eq!(filtered[0].ref_kind.as_deref(), Some("reexport"));

        let none = symbol_rdeps(&store, "Message", Some("other")).expect("query other");
        assert!(none.is_empty());
    }

    #[test]
    fn symbol_rdeps_excludes_unindexed_edge_kinds() {
        let (store, _tmp) = open_store();
        let id1 = insert_symbol(&store, "nous", "", "fn_a", "fn", "a.rs", 1);
        insert_ref(&store, id1, "hermeneus", "types", "Message", "type_use");
        let id2 = insert_symbol(&store, "melete", "", "fn_b", "fn", "b.rs", 2);
        insert_ref(&store, id2, "hermeneus", "types", "Message", "call");

        let rows = symbol_rdeps(&store, "Message", None).expect("query all");
        assert!(
            rows.is_empty(),
            "symbol_rdeps v1 must expose only indexed impl and reexport edges"
        );
    }

    #[test]
    fn impl_search_finds_implementing_types() {
        let (store, _tmp) = open_store();
        let id1 = insert_symbol(
            &store,
            "eidos",
            "types",
            "Response",
            "impl",
            "eidos/src/types.rs",
            42,
        );
        insert_ref(&store, id1, "", "", "Stamped", "impl");
        let id2 = insert_symbol(
            &store,
            "nous",
            "agent",
            "AgentCtx",
            "impl",
            "nous/src/agent.rs",
            7,
        );
        insert_ref(&store, id2, "", "", "Stamped", "impl");

        let rows = impl_search(&store, "Stamped").expect("query");
        assert_eq!(rows.len(), 2);
        let names: Vec<_> = rows
            .iter()
            .filter_map(|r| r.symbol_name.as_deref())
            .collect();
        assert!(names.contains(&"Response"), "expected Response");
        assert!(names.contains(&"AgentCtx"), "expected AgentCtx");
    }

    #[test]
    fn impl_search_empty_for_unknown_trait() {
        let (store, _tmp) = open_store();
        let rows = impl_search(&store, "NoSuchTrait").expect("query");
        assert!(rows.is_empty());
    }

    #[test]
    fn reexport_chain_finds_pub_use_sites() {
        let (store, _tmp) = open_store();
        insert_symbol(
            &store,
            "organon",
            "prelude",
            "Message",
            "reexport",
            "organon/src/prelude.rs",
            5,
        );
        insert_symbol(
            &store,
            "nous",
            "prelude",
            "Message",
            "reexport",
            "nous/src/prelude.rs",
            3,
        );

        let rows = reexport_chain(&store, "Message").expect("query");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn reexport_chain_ignores_non_reexport_symbols() {
        let (store, _tmp) = open_store();
        insert_symbol(
            &store,
            "eidos",
            "types",
            "Message",
            "struct",
            "eidos/src/types.rs",
            1,
        );

        let rows = reexport_chain(&store, "Message").expect("query");
        assert!(
            rows.is_empty(),
            "struct should not appear in reexport_chain"
        );
    }

    #[test]
    fn crate_deps_returns_direct_deps() {
        let (store, _tmp) = open_store();
        insert_edge(&store, "nous", "eidos");
        insert_edge(&store, "nous", "hermeneus");
        insert_edge(&store, "melete", "nous");

        let deps = crate_deps(&store, "nous").expect("deps");
        let names: Vec<_> = deps
            .iter()
            .filter_map(|r| r.crate_name.as_deref())
            .collect();
        assert!(names.contains(&"eidos"));
        assert!(names.contains(&"hermeneus"));
        assert!(
            !names.contains(&"melete"),
            "melete depends on nous, not the other way"
        );
    }

    #[test]
    fn crate_rdeps_returns_dependents() {
        let (store, _tmp) = open_store();
        insert_edge(&store, "nous", "eidos");
        insert_edge(&store, "hermeneus", "eidos");

        let rdeps = crate_rdeps(&store, "eidos").expect("rdeps");
        let names: Vec<_> = rdeps
            .iter()
            .filter_map(|r| r.crate_name.as_deref())
            .collect();
        assert!(names.contains(&"nous"));
        assert!(names.contains(&"hermeneus"));
    }

    #[test]
    fn symbols_in_returns_all_for_crate() {
        let (store, _tmp) = open_store();
        insert_symbol(&store, "eidos", "types", "Foo", "struct", "f.rs", 1);
        insert_symbol(&store, "eidos", "types", "bar", "fn", "f.rs", 5);
        insert_symbol(&store, "nous", "", "baz", "fn", "g.rs", 1);

        let rows = symbols_in(&store, "eidos", None).expect("query");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    fn symbols_in_filters_by_kind() {
        let (store, _tmp) = open_store();
        insert_symbol(&store, "eidos", "types", "Foo", "struct", "f.rs", 1);
        insert_symbol(&store, "eidos", "types", "bar", "fn", "f.rs", 5);

        let fns = symbols_in(&store, "eidos", Some("fn")).expect("query fns");
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].symbol_name.as_deref(), Some("bar"));
    }

    #[test]
    fn query_row_source_carries_schema_version() {
        let r = QueryRow::new(SCHEMA_VERSION);
        assert_eq!(r.source, format!("gnosis@{SCHEMA_VERSION}"));
    }
}
