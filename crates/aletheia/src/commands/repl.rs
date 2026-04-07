//! `aletheia repl`: interactive Datalog query interface against the knowledge graph.
//!
//! Opens the krites (Datalog) database in read-only mode and accepts queries on
//! stdin. Results are printed as formatted tables. Meta-commands (`.help`,
//! `.tables`, `.quit`) are handled before forwarding to the engine.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct ReplArgs {
    // instance_root is inherited from the top-level -r flag; nothing extra needed.
}

const BANNER: &str = r"aletheia Datalog REPL  (type .help for commands, .quit to exit)";

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
/// WHY: Synchronous stdin loop — async is unnecessary here and would add
/// complexity. The knowledge store holds an exclusive fjall lock, so the
/// server must be stopped before starting the REPL.
pub(crate) fn run(instance_root: Option<&PathBuf>, _args: &ReplArgs) -> Result<()> {
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
    let oikos = super::resolve_oikos(instance_root)?;
    let knowledge_path = oikos.knowledge_db();

    if !knowledge_path.exists() {
        snafu::whatever!(
            "knowledge store not found at {}\n  \
             Has this instance been initialized with recall enabled?",
            knowledge_path.display()
        );
    }

    let config = aletheia_mneme::knowledge_store::KnowledgeConfig::default();
    let store =
        aletheia_mneme::knowledge_store::KnowledgeStore::open_fjall(&knowledge_path, config)
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
                // NOTE: Unknown meta-commands fall through to Datalog processing.
                _ => {}
            }
        }

        // Accumulate lines until the statement ends with a semicolon or is a
        // self-contained expression. The Datalog engine is the authority on
        // whether the statement is complete; we send each line as-is and let
        // the engine emit a parse error if it is incomplete. For ergonomics we
        // buffer lines until the input ends with `;` OR is non-empty and the
        // user presses Enter on a blank line.
        //
        // WHY: Datalog statements don't require a semicolon terminator, but
        // multi-line input is common. We use the heuristic: if the accumulated
        // buffer is non-empty and the user enters a blank line, submit.
        if !trimmed.is_empty() {
            if !pending.is_empty() {
                pending.push(' ');
            }
            pending.push_str(trimmed);
        }

        // Submit on blank line continuation or single-line expression.
        let should_submit = trimmed.is_empty()    // blank line flushes buffer
            || trimmed.ends_with(';')             // explicit terminator
            || (!trimmed.contains('\n') && pending == trimmed); // single line

        if should_submit && !pending.is_empty() {
            let script = std::mem::take(&mut pending);
            // Strip trailing semicolon if present (engine does not require it).
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
    store: &std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    script: &str,
) {
    match store.run_query(script, BTreeMap::new()) {
        Ok(result) => {
            print_table(&result.headers, &result.rows);
        }
        Err(e) => {
            eprintln!("Error: {e}");
        }
    }
}

/// Print rows as a plain ASCII table with column headers.
///
/// WHY: No external pretty-print dependency. The table is functional for
/// debugging; polish is secondary. Each cell is formatted via
/// [`aletheia_mneme::engine::DataValue`]'s `Display` impl.
#[cfg(feature = "recall")]
#[expect(
    clippy::indexing_slicing,
    reason = "bounds are verified: i < widths.len() checked before index, and header enumeration is bounded"
)]
fn print_table(headers: &[String], rows: &[Vec<aletheia_mneme::engine::DataValue>]) {
    if headers.is_empty() && rows.is_empty() {
        println!("(no results)");
        return;
    }

    // Compute column widths: max of header width and each cell's display width.
    let ncols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(String::len).collect();

    let rendered: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(i, v)| {
                    let s = format_value(v);
                    if i < widths.len() && s.len() > widths[i] {
                        widths[i] = s.len();
                    }
                    s
                })
                .collect()
        })
        .collect();

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
    for row in &rendered {
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
    println!("({} row{})", rows.len(), if rows.len() == 1 { "" } else { "s" });
    println!();
}

/// Format a single [`DataValue`] for display.
///
/// WHY: Delegates to the engine's own `Display` impl so new variant additions
/// never cause a compile error here. `Str` values are stripped of engine-style
/// quoting (the `Display` impl wraps strings in `"..."`) to improve readability
/// in table cells.
#[cfg(feature = "recall")]
fn format_value(v: &aletheia_mneme::engine::DataValue) -> String {
    use aletheia_mneme::engine::DataValue;
    match v {
        // WHY: engine Display wraps Str in `"..."` quotes; strip them for
        // cleaner table output.
        DataValue::Str(s) => s.to_string(),
        // WHY: Num has a public get_int(); use it to avoid decimal points on
        // integer values, which the engine Display also does.
        DataValue::Num(n) => {
            if let Some(i) = n.get_int() {
                format!("{i}")
            } else {
                // WHY: fall back to engine Display for floats — it handles
                // NaN/Inf correctly.
                format!("{v}")
            }
        }
        // All other variants: use the engine's Display impl as source of truth.
        _ => format!("{v}"),
    }
}
