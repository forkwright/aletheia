//! `aletheia repl`: interactive Datalog query interface against the knowledge graph.
//!
//! Opens the krites (Datalog) database in read-only mode and accepts queries on
//! stdin. Results are printed as formatted tables. Meta-commands (`.help`,
//! `.tables`, `.quit`) are handled before forwarding to the engine.

use std::path::PathBuf;

use clap::Args;

use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct ReplArgs {
    /// Server URL for lock detection
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,
}

#[cfg(feature = "recall")]
const BANNER: &str = r"aletheia Datalog REPL  (type .help for commands, .quit to exit)";

#[cfg(feature = "recall")]
const HELP_TEXT: &str = r"Meta-commands:
  .help      Show this help message
  .tables    List all stored relations
  .quit      Exit the REPL

Datalog examples:
  ?[x] := x = 1
  ?[id, content] := *facts{id, content}
  ::relations
";

/// Run the interactive Datalog REPL.
///
/// WHY: the guard check is async (HTTP health probe), but the REPL loop itself
/// is synchronous stdin I/O. We check for a running server first, then drop
/// into the blocking loop.
pub(crate) async fn run(instance_root: Option<&PathBuf>, args: &ReplArgs) -> Result<()> {
    super::agent_io::guard_knowledge_lock(&args.url).await?;

    #[cfg(feature = "recall")]
    {
        run_repl(instance_root)
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = instance_root;
        snafu::whatever!(
            "repl requires the 'recall' feature.\n  \
             Build with: cargo build --features recall"
        );
    }
}

#[cfg(feature = "recall")]
fn run_repl(instance_root: Option<&PathBuf>) -> Result<()> {
    use std::io::{self, BufRead, Write};

    use snafu::prelude::*;

    let oikos = super::resolve_oikos(instance_root)?;
    let knowledge_path = oikos.knowledge_cohort_db("shared");

    if !knowledge_path.exists() && !oikos.knowledge_db().exists() {
        snafu::whatever!(
            "knowledge store not initialized at {}\n  \
             The store is created lazily by the running server. Either:\n    \
               1. Start the server once to bootstrap it:  aletheia\n    \
               2. Or route this command through a running server with --url",
            knowledge_path.display()
        );
    }

    let config = taxis::loader::load_config(&oikos).ok().map_or_else(
        mneme::knowledge_store::KnowledgeConfig::default,
        |config| {
            let embedding = config.embedding.to_embedding_config();
            mneme::knowledge_store::KnowledgeConfig {
                dim: config.embedding.dimension,
                embedding_model: embedding.effective_model_name(),
                ..Default::default()
            }
        },
    );
    let store = mneme::knowledge_store::KnowledgeStore::open_fjall(&knowledge_path, config)
        .whatever_context("failed to open knowledge store")?;

    let stdin = io::stdin();
    let stdout = io::stdout();

    println!("{BANNER}");
    println!("Database: {}", knowledge_path.display());
    println!();

    let mut line_buf = String::new();
    let mut pending = String::new();

    loop {
        // Print prompt: `> ` for a fresh statement, `  ` for continuation.
        let prompt = if pending.is_empty() { "> " } else { "  " };
        {
            let mut out = stdout.lock();
            out.write_all(prompt.as_bytes())
                .whatever_context("failed to write prompt")?;
            out.flush().whatever_context("failed to flush stdout")?;
        }

        line_buf.clear();
        let n = stdin
            .lock()
            .read_line(&mut line_buf)
            .whatever_context("failed to read stdin")?;

        // EOF (Ctrl-D).
        if n == 0 {
            println!();
            break;
        }

        let trimmed = line_buf.trim();

        // Skip blank lines when not in a multi-line statement.
        if trimmed.is_empty() && pending.is_empty() {
            continue;
        }

        // --- Meta-commands (only recognised at statement start) ---
        if pending.is_empty() {
            match trimmed {
                ".quit" | ".exit" => break,
                ".help" => {
                    print!("{HELP_TEXT}");
                    continue;
                }
                ".tables" => {
                    run_query_and_print(&store, "::relations");
                    continue;
                }
                _ => {} // Not a REPL command; fall through to treat as query.
            }
        }

        // WHY: the Datalog engine is the authority on statement completeness —
        // it requires no semicolon terminator, but multi-line input is common.
        // Heuristic: buffer lines until the input ends with `;`, or submit
        // when the buffer is non-empty and the user enters a blank line.
        if !trimmed.is_empty() {
            if !pending.is_empty() {
                pending.push(' ');
            }
            pending.push_str(trimmed);
        }

        let should_submit = trimmed.is_empty()    // blank line flushes buffer
            || trimmed.ends_with(';')             // explicit terminator
            || (!trimmed.contains('\n') && pending == trimmed); // single line

        if should_submit && !pending.is_empty() {
            let script = std::mem::take(&mut pending);
            let script = script.trim_end_matches(';').trim().to_owned();
            if !script.is_empty() {
                run_query_and_print(&store, &script);
            }
        }
    }

    println!("Bye.");
    Ok(())
}

/// Execute a single Datalog script and print the result as a formatted table.
///
/// Errors are printed to stderr and swallowed: a bad query should not abort
/// the REPL session.
#[cfg(feature = "recall")]
fn run_query_and_print(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    script: &str,
) {
    use std::collections::BTreeMap;
    match store.run_query(script, BTreeMap::new()) {
        Ok(result) => {
            let rendered = result.rows_as_strings();
            print_table(&result.headers, &rendered);
        }
        Err(e) => {
            eprintln!("Error: {e}");
        }
    }
}

/// Print rows as a plain ASCII table with column headers.
///
/// WHY: No external pretty-print dependency. The table is functional for
/// debugging; polish is secondary. Rows are pre-formatted as strings by
/// [`QueryResult::rows_as_strings`].
#[cfg(feature = "recall")]
#[expect(
    clippy::indexing_slicing,
    reason = "bounds are verified: i < widths.len() checked before index, and header enumeration is bounded"
)]
fn print_table(headers: &[String], rows: &[Vec<String>]) {
    if headers.is_empty() && rows.is_empty() {
        println!("(no results)");
        return;
    }

    // Compute column widths: max of header width and each cell's display width.
    let ncols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(String::len).collect();

    // WHY: compute widths by side-effect during iteration; this is intentionally
    // not inspect() because we need mutable access to widths via the closure.
    for row in rows {
        for (i, s) in row.iter().enumerate() {
            if i < widths.len() && s.len() > widths[i] {
                widths[i] = s.len();
            }
        }
    }

    // Ensure widths covers all columns that may appear in rows but not headers.
    let max_row_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    while widths.len() < max_row_cols {
        widths.push(1);
    }

    // Build separator line.
    let sep: String = widths
        .iter()
        .map(|w| "-".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("+");
    let sep = format!("+{sep}+");

    // Header row.
    println!("{sep}");
    if ncols > 0 {
        let header_row: String = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!(" {h:width$} ", width = widths[i]))
            .collect::<Vec<_>>()
            .join("|");
        println!("|{header_row}|");
        println!("{sep}");
    }

    // Data rows.
    for row in rows {
        let cells: String = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let w = widths.get(i).copied().unwrap_or(cell.len());
                format!(" {cell:w$} ")
            })
            .collect::<Vec<_>>()
            .join("|");
        println!("|{cells}|");
    }

    println!("{sep}");
    println!(
        "({} row{})",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" }
    );
    println!();
}
