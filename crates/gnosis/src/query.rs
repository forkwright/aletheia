//! Query implementations against the gnosis `SQLite` index.
//!
//! All queries are synchronous — the index is a local `SQLite` file and queries
//! complete in milliseconds.  The public API returns `Vec<QueryRow>`, a common
//! result type that serialises cleanly to JSON for MCP tool output.
//!
//! # Available queries
//!
//! | Query            | Answers                                                        |
//! |------------------|----------------------------------------------------------------|
//! | `symbol_rdeps`   | Which (crate, module, symbol) entries reference a target symbol? |
//! | `impl_search`    | Which types implement a given trait?                            |
//! | `reexport_chain` | Which crates re-export a given symbol name?                     |
//! | `crate_deps`     | What crates does a given crate directly depend on?              |
//! | `crate_rdeps`    | What crates directly depend on a given crate?                   |
//! | `symbols_in`     | List all symbols in a given crate (optionally filtered by kind). |
//!
//! # Epistemic tier
//!
//! Results are machine-derived from the AST and carry `EpistemicTier::Inferred`
//! confidence.  They reflect the source as of the last `CodeGraph::rebuild()`
//! call, not necessarily the current on-disk state.  The `source` field of
//! every `QueryRow` records the gnosis schema version so callers can detect
//! staleness.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error::{Result, SqliteSnafu};
use crate::schema::SCHEMA_VERSION;

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
    /// Symbol kind (`fn`, `struct`, `enum`, `trait`, `impl`, `reexport`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    /// Absolute path to the source file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// 1-based line number of the definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<i64>,
    /// Reference kind for edge-type results (`call`, `reexport`, `impl`, `type_use`).
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

/// Return every symbol that references `symbol_name` (optionally filtered to
/// a specific target `crate_name`).
///
/// Corresponds to the `symbol_rdeps` MCP operation.
///
/// # Arguments
///
/// - `symbol_name` — the symbol to look up (e.g. `"Message"`).
/// - `target_crate` — if `Some`, only return refs where `to_crate` matches.
#[tracing::instrument(skip(conn))]
pub(crate) fn symbol_rdeps(
    conn: &Connection,
    symbol_name: &str,
    target_crate: Option<&str>,
) -> Result<Vec<QueryRow>> {
    let sql = if target_crate.is_some() {
        "SELECT s.crate_name, s.module_path, s.symbol_name, s.symbol_kind,
                s.file_path, s.line_start, r.ref_kind
         FROM symbol_refs r
         JOIN symbols s ON s.id = r.from_symbol
         WHERE r.to_symbol = ?1 AND r.to_crate = ?2
         ORDER BY s.crate_name, s.module_path, s.symbol_name"
    } else {
        "SELECT s.crate_name, s.module_path, s.symbol_name, s.symbol_kind,
                s.file_path, s.line_start, r.ref_kind
         FROM symbol_refs r
         JOIN symbols s ON s.id = r.from_symbol
         WHERE r.to_symbol = ?1
         ORDER BY s.crate_name, s.module_path, s.symbol_name"
    };

    let mut stmt = conn.prepare(sql).context(SqliteSnafu)?;

    let rows = if let Some(tc) = target_crate {
        stmt.query_map(params![symbol_name, tc], map_symbol_ref_row)
            .context(SqliteSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(SqliteSnafu)?
    } else {
        stmt.query_map(params![symbol_name], map_symbol_ref_row)
            .context(SqliteSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(SqliteSnafu)?
    };

    Ok(rows)
}

fn map_symbol_ref_row(row: &rusqlite::Row<'_>) -> std::result::Result<QueryRow, rusqlite::Error> {
    let mut r = QueryRow::new(SCHEMA_VERSION);
    r.crate_name = Some(row.get(0)?);
    r.module_path = Some(row.get(1)?);
    r.symbol_name = Some(row.get(2)?);
    r.symbol_kind = Some(row.get(3)?);
    r.file_path = Some(row.get(4)?);
    r.line_start = Some(row.get(5)?);
    r.ref_kind = Some(row.get(6)?);
    Ok(r)
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
#[tracing::instrument(skip(conn))]
pub(crate) fn impl_search(conn: &Connection, trait_name: &str) -> Result<Vec<QueryRow>> {
    let sql = "SELECT s.crate_name, s.module_path, s.symbol_name, s.file_path, s.line_start
               FROM symbol_refs r
               JOIN symbols s ON s.id = r.from_symbol
               WHERE r.ref_kind = 'impl' AND r.to_symbol = ?1
               ORDER BY s.crate_name, s.symbol_name";

    let mut stmt = conn.prepare(sql).context(SqliteSnafu)?;
    let rows = stmt
        .query_map(params![trait_name], |row| {
            let mut r = QueryRow::new(SCHEMA_VERSION);
            r.crate_name = Some(row.get(0)?);
            r.module_path = Some(row.get(1)?);
            r.symbol_name = Some(row.get(2)?);
            r.symbol_kind = Some("impl".to_owned());
            r.file_path = Some(row.get(3)?);
            r.line_start = Some(row.get(4)?);
            r.ref_kind = Some("impl".to_owned());
            Ok(r)
        })
        .context(SqliteSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(SqliteSnafu)?;

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
#[tracing::instrument(skip(conn))]
pub(crate) fn reexport_chain(conn: &Connection, symbol_name: &str) -> Result<Vec<QueryRow>> {
    let sql = "SELECT s.crate_name, s.module_path, s.file_path, s.line_start
               FROM symbols s
               WHERE s.symbol_name = ?1 AND s.symbol_kind = 'reexport'
               ORDER BY s.crate_name, s.module_path";

    let mut stmt = conn.prepare(sql).context(SqliteSnafu)?;
    let rows = stmt
        .query_map(params![symbol_name], |row| {
            let mut r = QueryRow::new(SCHEMA_VERSION);
            r.crate_name = Some(row.get(0)?);
            r.module_path = Some(row.get(1)?);
            r.symbol_name = Some(symbol_name.to_owned());
            r.symbol_kind = Some("reexport".to_owned());
            r.file_path = Some(row.get(2)?);
            r.line_start = Some(row.get(3)?);
            r.ref_kind = Some("reexport".to_owned());
            Ok(r)
        })
        .context(SqliteSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(SqliteSnafu)?;

    Ok(rows)
}

// ── Query: crate_deps ─────────────────────────────────────────────────────────

/// Return the direct workspace dependencies of `crate_name`.
#[tracing::instrument(skip(conn))]
pub(crate) fn crate_deps(conn: &Connection, crate_name: &str) -> Result<Vec<QueryRow>> {
    let sql = "SELECT to_crate FROM crate_edges WHERE from_crate = ?1 ORDER BY to_crate";
    let mut stmt = conn.prepare(sql).context(SqliteSnafu)?;
    let rows = stmt
        .query_map(params![crate_name], |row| {
            let mut r = QueryRow::new(SCHEMA_VERSION);
            r.crate_name = row.get(0)?;
            Ok(r)
        })
        .context(SqliteSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(SqliteSnafu)?;
    Ok(rows)
}

// ── Query: crate_rdeps ────────────────────────────────────────────────────────

/// Return the workspace crates that directly depend on `crate_name`.
#[tracing::instrument(skip(conn))]
pub(crate) fn crate_rdeps(conn: &Connection, crate_name: &str) -> Result<Vec<QueryRow>> {
    let sql = "SELECT from_crate FROM crate_edges WHERE to_crate = ?1 ORDER BY from_crate";
    let mut stmt = conn.prepare(sql).context(SqliteSnafu)?;
    let rows = stmt
        .query_map(params![crate_name], |row| {
            let mut r = QueryRow::new(SCHEMA_VERSION);
            r.crate_name = row.get(0)?;
            Ok(r)
        })
        .context(SqliteSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(SqliteSnafu)?;
    Ok(rows)
}

// ── Query: symbols_in ─────────────────────────────────────────────────────────

/// List all symbols in `crate_name`, optionally filtered to `kind`.
#[tracing::instrument(skip(conn))]
pub(crate) fn symbols_in(
    conn: &Connection,
    crate_name: &str,
    kind: Option<&str>,
) -> Result<Vec<QueryRow>> {
    let sql = if kind.is_some() {
        "SELECT crate_name, module_path, symbol_name, symbol_kind, file_path, line_start
         FROM symbols
         WHERE crate_name = ?1 AND symbol_kind = ?2
         ORDER BY module_path, symbol_name"
    } else {
        "SELECT crate_name, module_path, symbol_name, symbol_kind, file_path, line_start
         FROM symbols
         WHERE crate_name = ?1
         ORDER BY module_path, symbol_name"
    };

    let mut stmt = conn.prepare(sql).context(SqliteSnafu)?;

    let rows = if let Some(k) = kind {
        stmt.query_map(params![crate_name, k], map_symbol_row)
            .context(SqliteSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(SqliteSnafu)?
    } else {
        stmt.query_map(params![crate_name], map_symbol_row)
            .context(SqliteSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(SqliteSnafu)?
    };

    Ok(rows)
}

fn map_symbol_row(row: &rusqlite::Row<'_>) -> std::result::Result<QueryRow, rusqlite::Error> {
    let mut r = QueryRow::new(SCHEMA_VERSION);
    r.crate_name = Some(row.get(0)?);
    r.module_path = Some(row.get(1)?);
    r.symbol_name = Some(row.get(2)?);
    r.symbol_kind = Some(row.get(3)?);
    r.file_path = Some(row.get(4)?);
    r.line_start = Some(row.get(5)?);
    Ok(r)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::schema;
    use rusqlite::Connection;

    fn open_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        schema::init(&conn).expect("schema init");
        conn
    }

    fn insert_symbol(
        conn: &Connection,
        crate_name: &str,
        module: &str,
        name: &str,
        kind: &str,
        file: &str,
        line: i64,
    ) -> i64 {
        conn.execute(
            "INSERT INTO symbols (crate_name, module_path, symbol_name, symbol_kind, file_path, line_start)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![crate_name, module, name, kind, file, line],
        )
        .expect("insert symbol");
        conn.last_insert_rowid()
    }

    fn insert_ref(
        conn: &Connection,
        from_id: i64,
        to_crate: &str,
        to_module: &str,
        to_sym: &str,
        kind: &str,
    ) {
        conn.execute(
            "INSERT INTO symbol_refs (from_symbol, to_crate, to_module, to_symbol, ref_kind)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![from_id, to_crate, to_module, to_sym, kind],
        )
        .expect("insert ref");
    }

    fn insert_edge(conn: &Connection, from: &str, to: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO crate_edges (from_crate, to_crate) VALUES (?1, ?2)",
            params![from, to],
        )
        .expect("insert edge");
    }

    // ── symbol_rdeps ─────────────────────────────────────────────────────────

    #[test]
    fn symbol_rdeps_empty_when_no_refs() {
        let conn = open_db();
        let rows = symbol_rdeps(&conn, "Message", None).expect("query");
        assert!(rows.is_empty());
    }

    #[test]
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    fn symbol_rdeps_finds_single_caller() {
        let conn = open_db();
        let sym_id = insert_symbol(
            &conn,
            "nous",
            "execute",
            "dispatch",
            "fn",
            "nous/src/execute.rs",
            10,
        );
        insert_ref(&conn, sym_id, "hermeneus", "types", "Message", "type_use");

        let rows = symbol_rdeps(&conn, "Message", None).expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol_name.as_deref(), Some("dispatch"));
        assert_eq!(rows[0].crate_name.as_deref(), Some("nous"));
    }

    #[test]
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    fn symbol_rdeps_filters_by_target_crate() {
        let conn = open_db();
        let id1 = insert_symbol(&conn, "nous", "", "fn_a", "fn", "a.rs", 1);
        insert_ref(&conn, id1, "hermeneus", "types", "Message", "type_use");
        let id2 = insert_symbol(&conn, "melete", "", "fn_b", "fn", "b.rs", 2);
        insert_ref(&conn, id2, "other", "", "Message", "type_use");

        let all = symbol_rdeps(&conn, "Message", None).expect("query all");
        assert_eq!(all.len(), 2);

        let filtered = symbol_rdeps(&conn, "Message", Some("hermeneus")).expect("query filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].crate_name.as_deref(), Some("nous"));
    }

    // ── impl_search ──────────────────────────────────────────────────────────

    #[test]
    fn impl_search_finds_implementing_types() {
        let conn = open_db();
        let id1 = insert_symbol(
            &conn,
            "eidos",
            "types",
            "Response",
            "impl",
            "eidos/src/types.rs",
            42,
        );
        insert_ref(&conn, id1, "", "", "Stamped", "impl");
        let id2 = insert_symbol(
            &conn,
            "nous",
            "agent",
            "AgentCtx",
            "impl",
            "nous/src/agent.rs",
            7,
        );
        insert_ref(&conn, id2, "", "", "Stamped", "impl");

        let rows = impl_search(&conn, "Stamped").expect("query");
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
        let conn = open_db();
        let rows = impl_search(&conn, "NoSuchTrait").expect("query");
        assert!(rows.is_empty());
    }

    // ── reexport_chain ───────────────────────────────────────────────────────

    #[test]
    fn reexport_chain_finds_pub_use_sites() {
        let conn = open_db();
        insert_symbol(
            &conn,
            "organon",
            "prelude",
            "Message",
            "reexport",
            "organon/src/prelude.rs",
            5,
        );
        insert_symbol(
            &conn,
            "nous",
            "prelude",
            "Message",
            "reexport",
            "nous/src/prelude.rs",
            3,
        );

        let rows = reexport_chain(&conn, "Message").expect("query");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn reexport_chain_ignores_non_reexport_symbols() {
        let conn = open_db();
        // A struct named "Message" should NOT appear in reexport results.
        insert_symbol(
            &conn,
            "eidos",
            "types",
            "Message",
            "struct",
            "eidos/src/types.rs",
            1,
        );

        let rows = reexport_chain(&conn, "Message").expect("query");
        assert!(
            rows.is_empty(),
            "struct should not appear in reexport_chain"
        );
    }

    // ── crate_deps / crate_rdeps ─────────────────────────────────────────────

    #[test]
    fn crate_deps_returns_direct_deps() {
        let conn = open_db();
        insert_edge(&conn, "nous", "eidos");
        insert_edge(&conn, "nous", "hermeneus");
        insert_edge(&conn, "melete", "nous");

        let deps = crate_deps(&conn, "nous").expect("deps");
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
        let conn = open_db();
        insert_edge(&conn, "nous", "eidos");
        insert_edge(&conn, "hermeneus", "eidos");

        let rdeps = crate_rdeps(&conn, "eidos").expect("rdeps");
        let names: Vec<_> = rdeps
            .iter()
            .filter_map(|r| r.crate_name.as_deref())
            .collect();
        assert!(names.contains(&"nous"));
        assert!(names.contains(&"hermeneus"));
    }

    // ── symbols_in ───────────────────────────────────────────────────────────

    #[test]
    fn symbols_in_returns_all_for_crate() {
        let conn = open_db();
        insert_symbol(&conn, "eidos", "types", "Foo", "struct", "f.rs", 1);
        insert_symbol(&conn, "eidos", "types", "bar", "fn", "f.rs", 5);
        insert_symbol(&conn, "nous", "", "baz", "fn", "g.rs", 1);

        let rows = symbols_in(&conn, "eidos", None).expect("query");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    fn symbols_in_filters_by_kind() {
        let conn = open_db();
        insert_symbol(&conn, "eidos", "types", "Foo", "struct", "f.rs", 1);
        insert_symbol(&conn, "eidos", "types", "bar", "fn", "f.rs", 5);

        let fns = symbols_in(&conn, "eidos", Some("fn")).expect("query fns");
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].symbol_name.as_deref(), Some("bar"));
    }

    // ── QueryRow source field ─────────────────────────────────────────────────

    #[test]
    fn query_row_source_carries_schema_version() {
        let r = QueryRow::new(SCHEMA_VERSION);
        assert_eq!(r.source, format!("gnosis@{SCHEMA_VERSION}"));
    }
}
