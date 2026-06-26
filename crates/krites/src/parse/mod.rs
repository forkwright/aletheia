//! Datalog query language parser.
//!
//! Transforms Datalog source text into an AST of [`DatalogScript`] variants.
//! The parser uses a pest PEG grammar (`src/datalog.pest`) for lexing and
//! structural analysis, then converts pest pairs into typed AST nodes.
//!
//! # Architecture
//!
//! - [`parse_script`] is the public entry point.
//! - [`expr`] handles expression parsing via Pratt precedence.
//! - [`query`] handles rule, atom, and program parsing.
//! - [`imperative`] handles `%if`/`%loop`/`%return` blocks.
//! - [`sys`] handles system commands (`:compact`, `:explain`, index ops, etc.).
//! - [`fts`] handles full-text search query parsing.
//! - [`schema`] handles relation schema definitions.
//! - [`error`] defines the structured [`ParseError`](error::ParseError) type.
#![expect(
    clippy::needless_return,
    clippy::pedantic,
    clippy::result_large_err,
    reason = "Datalog parser top-level — InternalError is the crate-wide Result type, pedantic style deferred"
)]

use std::cmp::{max, min};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use compact_str::CompactString;
use either::{Either, Left};
use pest::Parser;
use pest::error::InputLocation;
use snafu::Snafu;

use crate::FixedRule;
use crate::data::program::InputProgram;
use crate::data::value::{DataValue, ValidityTs};
use crate::error::InternalResult as Result;
use crate::parse::imperative::parse_imperative_block;
use crate::parse::query::parse_query;
use crate::parse::sys::{SysOp, parse_sys};

pub(crate) mod error;
pub(crate) mod expr;
pub(crate) mod fts;
pub(crate) mod imperative;
pub(crate) mod query;
pub(crate) mod schema;
pub(crate) mod sys;

#[derive(pest_derive::Parser)]
#[grammar = "src/datalog.pest"]
pub(crate) struct DatalogParser;

pub(crate) type Pair<'a> = pest::iterators::Pair<'a, Rule>;
pub(crate) type Pairs<'a> = pest::iterators::Pairs<'a, Rule>;

/// A parsed datalog script, as returned by `parse_script`.
#[derive(Debug)]
#[non_exhaustive]
pub enum DatalogScript {
    /// A single query program.
    Single(InputProgram),
    /// An imperative script with control flow.
    Imperative(ImperativeProgram),
    /// A system command (`:compact`, `:explain`, etc.).
    Sys(SysOp),
}

/// A query program with an optional storage destination.
#[derive(Debug)]
pub struct ImperativeStmtClause {
    /// The parsed query program.
    pub prog: InputProgram,
    /// Optional name to store results into a temporary relation.
    pub store_as: Option<CompactString>,
}

/// A system operation with an optional storage destination.
#[derive(Debug)]
pub struct ImperativeSysop {
    /// The parsed system operation.
    pub sysop: SysOp,
    /// Optional name to store results into a temporary relation.
    pub store_as: Option<CompactString>,
}

/// An imperative statement within a `%`-prefixed control block.
#[derive(Debug)]
#[non_exhaustive]
pub enum ImperativeStmt {
    /// Exit the nearest (or named) enclosing loop.
    Break {
        target: Option<CompactString>,
        span: SourceSpan,
    },
    /// Skip to the next iteration of the nearest (or named) enclosing loop.
    Continue {
        target: Option<CompactString>,
        span: SourceSpan,
    },
    /// Return results from an imperative block.
    Return {
        returns: Vec<Either<ImperativeStmtClause, CompactString>>,
    },
    /// Execute a query program.
    Program { prog: ImperativeStmtClause },
    /// Execute a system operation.
    SysOp { sysop: ImperativeSysop },
    /// Execute a query, suppressing any errors.
    IgnoreErrorProgram { prog: ImperativeStmtClause },
    /// Conditional branch.
    If {
        condition: ImperativeCondition,
        then_branch: ImperativeProgram,
        else_branch: ImperativeProgram,
        negated: bool,
    },
    /// Infinite loop with optional label.
    Loop {
        label: Option<CompactString>,
        body: ImperativeProgram,
    },
    /// Swap two temporary relations.
    TempSwap {
        left: CompactString,
        right: CompactString,
    },
    /// Debug-print a temporary relation.
    TempDebug { temp: CompactString },
}

pub(crate) type ImperativeCondition = Either<CompactString, ImperativeStmtClause>;

/// A series of `{}` queries possibly with imperative directives like `%if` and `%loop`.
pub type ImperativeProgram = Vec<ImperativeStmt>;

impl ImperativeStmt {
    /// Collect relation names that require write locks for this statement.
    pub(crate) fn needs_write_locks(&self, collector: &mut BTreeSet<CompactString>) {
        match self {
            ImperativeStmt::Program { prog, .. }
            | ImperativeStmt::IgnoreErrorProgram { prog, .. } => {
                if let Some(name) = prog.prog.needs_write_lock() {
                    collector.insert(name);
                }
            }
            ImperativeStmt::Return { returns, .. } => {
                for ret in returns {
                    if let Left(prog) = ret
                        && let Some(name) = prog.prog.needs_write_lock()
                    {
                        collector.insert(name);
                    }
                }
            }
            ImperativeStmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if let ImperativeCondition::Right(prog) = condition
                    && let Some(name) = prog.prog.needs_write_lock()
                {
                    collector.insert(name);
                }
                for prog in then_branch.iter().chain(else_branch.iter()) {
                    prog.needs_write_locks(collector);
                }
            }
            ImperativeStmt::Loop { body, .. } => {
                for prog in body {
                    prog.needs_write_locks(collector);
                }
            }
            // NOTE: these statements don't acquire relation locks
            ImperativeStmt::TempDebug { .. }
            | ImperativeStmt::Break { .. }
            | ImperativeStmt::Continue { .. }
            | ImperativeStmt::TempSwap { .. } => {}
            ImperativeStmt::SysOp { sysop } => collect_sysop_write_locks(&sysop.sysop, collector),
        }
    }
}

/// Collect write locks needed by a system operation into `collector`.
fn collect_sysop_write_locks(sysop: &SysOp, collector: &mut BTreeSet<CompactString>) {
    /// Insert both the base relation and the composite `relation:index` key.
    fn insert_index_lock(
        collector: &mut BTreeSet<CompactString>,
        base: &CompactString,
        index: &CompactString,
    ) {
        collector.insert(base.clone());
        collector.insert(CompactString::from(format!("{base}:{index}")));
    }

    match sysop {
        SysOp::RemoveRelation(rels) => {
            for rel in rels {
                collector.insert(rel.name.clone());
            }
        }
        SysOp::RenameRelation(renames) => {
            for (old, new) in renames {
                collector.insert(old.name.clone());
                collector.insert(new.name.clone());
            }
        }
        SysOp::CreateIndex(symb, subs, _) => {
            insert_index_lock(collector, &symb.name, &subs.name);
        }
        SysOp::CreateVectorIndex(m) => {
            insert_index_lock(collector, &m.base_relation, &m.index_name);
        }
        SysOp::CreateFtsIndex(m) => {
            insert_index_lock(collector, &m.base_relation, &m.index_name);
        }
        SysOp::CreateMinHashLshIndex(m) => {
            insert_index_lock(collector, &m.base_relation, &m.index_name);
        }
        SysOp::RemoveIndex(rel, idx) => {
            collector.insert(CompactString::from(format!("{}:{}", rel.name, idx.name)));
        }
        _ => {
            // NOTE: other system operations don't require relation locks
        }
    }
}

impl DatalogScript {
    /// Extract the single program from a script, or error if it contains
    /// an imperative block or system command.
    pub(crate) fn get_single_program(self) -> Result<InputProgram> {
        match self {
            DatalogScript::Single(s) => Ok(s),
            DatalogScript::Imperative(_) | DatalogScript::Sys(_) => {
                return Err(crate::parse::error::InvalidQuerySnafu {
                    message: "expect script to contain only a single program".to_string(),
                }
                .build()
                .into());
            }
        }
    }
}

/// Byte-offset span within the source script.
///
/// `SourceSpan(start, length)` where `start` is the byte offset from the
/// beginning of the source and `length` is the number of bytes covered.
#[derive(Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, Copy, Clone, Default)]
pub struct SourceSpan(pub usize, pub usize);

impl Display for SourceSpan {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.0, self.0 + self.1)
    }
}

impl SourceSpan {
    /// Merge two spans into the smallest span that covers both.
    #[must_use]
    pub(crate) fn merge(self, other: Self) -> Self {
        let s1 = self.0;
        let e1 = self.0 + self.1;
        let s2 = other.0;
        let e2 = other.0 + other.1;
        let s = min(s1, s2);
        let e = max(e1, e2);
        Self(s, e - s)
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("The query parser has encountered unexpected input / end of input at {span}"))]
pub(crate) struct ParseError {
    pub(crate) span: SourceSpan,
}

/// Convert a pest `InputLocation` to a [`SourceSpan`].
#[must_use]
pub(crate) fn input_location_to_span(loc: InputLocation) -> SourceSpan {
    match loc {
        InputLocation::Pos(p) => SourceSpan(p, 0),
        InputLocation::Span((start, end)) => SourceSpan(start, end - start),
    }
}

/// Build an `UnexpectedRule` error for a grammar rule that should not appear
/// in the given parser context.
pub(crate) fn unexpected_rule_error(
    grammar_rule: &Rule,
    span: SourceSpan,
    context: &'static str,
) -> crate::error::InternalError {
    error::UnexpectedRuleSnafu {
        rule: format!("{grammar_rule:?}"),
        span,
        context,
    }
    .build()
    .into()
}

/// Parse a text script into the datalog AST.
///
/// * `src` - the script to parse
/// * `param_pool` - the list of parameters to execute the script with. These are substituted into the syntax tree during parsing.
/// * `fixed_rules` - a mapping of fixed rule names to their implementations. These are substituted into the syntax tree during parsing.
/// * `cur_vld` - the current timestamp, substituted into expressions where validity is relevant.
///
/// # Errors
///
/// Returns an error if the source contains syntax errors or if parsing fails.
pub fn parse_script(
    src: &str,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<DatalogScript> {
    let parsed = DatalogParser::parse(Rule::script, src)
        .map_err(|err| {
            let message = err.to_string();
            let span = input_location_to_span(err.location);
            error::SyntaxSnafu {
                span: span.to_string(),
                message,
            }
            .build()
        })?
        .next()
        .ok_or_else(|| {
            error::InvalidQuerySnafu {
                message: "expected script content".to_string(),
            }
            .build()
        })?;
    let span = parsed.extract_span();
    Ok(match parsed.as_rule() {
        Rule::query_script => {
            let q = parse_query(parsed.into_inner(), param_pool, fixed_rules, cur_vld)?;
            DatalogScript::Single(q)
        }
        Rule::imperative_script => {
            let p = parse_imperative_block(parsed, param_pool, fixed_rules, cur_vld)?;
            DatalogScript::Imperative(p)
        }
        Rule::sys_script => DatalogScript::Sys(parse_sys(
            parsed.into_inner(),
            param_pool,
            fixed_rules,
            cur_vld,
        )?),
        r => {
            return Err(unexpected_rule_error(&r, span, "parse_script"));
        }
    })
}

/// Extract a [`SourceSpan`] from a pest parse element.
trait ExtractSpan {
    /// Return the byte-offset span of this element within the source.
    fn extract_span(&self) -> SourceSpan;
}

impl ExtractSpan for Pair<'_> {
    fn extract_span(&self) -> SourceSpan {
        let span = self.as_span();
        let start = span.start();
        let end = span.end();
        SourceSpan(start, end - start)
    }
}

#[cfg(test)]
mod proptests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use proptest::prelude::*;

    use crate::data::value::ValidityTs;
    use crate::parse::DatalogScript;
    use crate::parse::parse_script;

    fn empty_fixed_rules() -> BTreeMap<String, Arc<Box<dyn crate::FixedRule>>> {
        BTreeMap::new()
    }

    proptest! {
        /// The Datalog parser must never panic on arbitrary input: it should return a
        /// parse error for invalid input, never crash. Panics indicate parser logic bugs.
        #[test]
        fn datalog_parser_never_panics(input in "\\PC{0,500}") {
            let _ = parse_script(
                &input,
                &BTreeMap::new(),
                &empty_fixed_rules(),
                ValidityTs(std::cmp::Reverse(0)),
            );
        }

        /// Valid minimal Datalog programs must parse to a single program with the
        /// expected entry arity and the generated relation present in the program.
        #[test]
        fn datalog_valid_program_parses_with_shape(
            rel in "[a-z][a-z0-9_]{0,15}",
            vals in proptest::collection::vec(0i64..100, 1..10),
        ) {
            let val_list = vals
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let src = format!("{rel}[a] := a in [{val_list}]\n?[a] := {rel}[a]");
            let parsed = parse_script(
                &src,
                &BTreeMap::new(),
                &empty_fixed_rules(),
                ValidityTs(std::cmp::Reverse(0)),
            )
            .unwrap_or_else(|e| panic!("valid generated Datalog should parse: {e}"));
            let program = match parsed {
                DatalogScript::Single(p) => p,
                other => panic!("expected a single program, got {:?}", other),
            };
            prop_assert_eq!(
                program.get_entry_arity().unwrap_or_else(|e| panic!("entry should exist: {e}")),
                1,
                "entry query has one column"
            );
            prop_assert!(
                program.prog.contains_key(&crate::data::symb::Symbol::new(
                    rel.clone(),
                    crate::parse::SourceSpan(0, 0)
                )),
                "generated relation {rel} should be present in program"
            );
        }
    }
}
