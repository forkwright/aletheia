//! AST for the datalog query language.

use std::cmp::{max, min};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use crate::engine::error::InternalResult as Result;
use compact_str::CompactString;
use either::{Either, Left};
use pest::Parser;
use pest::error::InputLocation;
use snafu::Snafu;

use crate::engine::FixedRule;
use crate::engine::data::program::InputProgram;
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::parse::imperative::parse_imperative_block;
use crate::engine::parse::query::parse_query;
use crate::engine::parse::sys::{SysOp, parse_sys};

pub(crate) mod error;
pub(crate) mod expr;
pub(crate) mod fts;
pub(crate) mod imperative;
pub(crate) mod query;
pub(crate) mod schema;
pub(crate) mod sys;

#[derive(pest_derive::Parser)]
#[grammar = "engine/datalog.pest"]
pub(crate) struct DatalogParser;

pub(crate) type Pair<'a> = pest::iterators::Pair<'a, Rule>;
pub(crate) type Pairs<'a> = pest::iterators::Pairs<'a, Rule>;

/// A parsed datalog script, as returned by `parse_script`.
#[derive(Debug)]
pub enum DatalogScript {
    Single(InputProgram),
    Imperative(ImperativeProgram),
    Sys(SysOp),
}

#[derive(Debug)]
pub struct ImperativeStmtClause {
    pub prog: InputProgram,
    pub store_as: Option<CompactString>,
}

#[derive(Debug)]
pub struct ImperativeSysop {
    pub sysop: SysOp,
    pub store_as: Option<CompactString>,
}

#[derive(Debug)]
pub enum ImperativeStmt {
    Break {
        target: Option<CompactString>,
        span: SourceSpan,
    },
    Continue {
        target: Option<CompactString>,
        span: SourceSpan,
    },
    Return {
        returns: Vec<Either<ImperativeStmtClause, CompactString>>,
    },
    Program {
        prog: ImperativeStmtClause,
    },
    SysOp {
        sysop: ImperativeSysop,
    },
    IgnoreErrorProgram {
        prog: ImperativeStmtClause,
    },
    If {
        condition: ImperativeCondition,
        then_branch: ImperativeProgram,
        else_branch: ImperativeProgram,
        negated: bool,
    },
    Loop {
        label: Option<CompactString>,
        body: ImperativeProgram,
    },
    TempSwap {
        left: CompactString,
        right: CompactString,
        // span: SourceSpan,
    },
    TempDebug {
        temp: CompactString,
    },
}

pub(crate) type ImperativeCondition = Either<CompactString, ImperativeStmtClause>;

/// A series of `{}` queries possibly with imperative directives like `%if` and `%loop`.
pub type ImperativeProgram = Vec<ImperativeStmt>;

impl ImperativeStmt {
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
                    if let Left(prog) = ret {
                        if let Some(name) = prog.prog.needs_write_lock() {
                            collector.insert(name);
                        }
                    }
                }
            }
            ImperativeStmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if let ImperativeCondition::Right(prog) = condition {
                    if let Some(name) = prog.prog.needs_write_lock() {
                        collector.insert(name);
                    }
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
            ImperativeStmt::TempDebug { .. }
            | ImperativeStmt::Break { .. }
            | ImperativeStmt::Continue { .. }
            | ImperativeStmt::TempSwap { .. } => {}
            ImperativeStmt::SysOp { sysop } => match &sysop.sysop {
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
                    collector.insert(symb.name.clone());
                    collector.insert(CompactString::from(format!("{}:{}", symb.name, subs.name)));
                }
                SysOp::CreateVectorIndex(m) => {
                    collector.insert(m.base_relation.clone());
                    collector.insert(CompactString::from(format!(
                        "{}:{}",
                        m.base_relation, m.index_name
                    )));
                }
                SysOp::CreateFtsIndex(m) => {
                    collector.insert(m.base_relation.clone());
                    collector.insert(CompactString::from(format!(
                        "{}:{}",
                        m.base_relation, m.index_name
                    )));
                }
                SysOp::CreateMinHashLshIndex(m) => {
                    collector.insert(m.base_relation.clone());
                    collector.insert(CompactString::from(format!(
                        "{}:{}",
                        m.base_relation, m.index_name
                    )));
                }
                SysOp::RemoveIndex(rel, idx) => {
                    collector.insert(CompactString::from(format!("{}:{}", rel.name, idx.name)));
                }
                _ => {}
            },
        }
    }
}

impl DatalogScript {
    pub(crate) fn get_single_program(self) -> Result<InputProgram> {
        match self {
            DatalogScript::Single(s) => Ok(s),
            DatalogScript::Imperative(_) | DatalogScript::Sys(_) => {
                return Err(crate::engine::parse::error::InvalidQuerySnafu {
                    message: "expect script to contain only a single program".to_string(),
                }
                .build()
                .into());
            }
        }
    }
}

/// Span of the element in the source script, with starting and ending positions.
#[derive(Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, Copy, Clone, Default)]
pub struct SourceSpan(pub usize, pub usize);

impl Display for SourceSpan {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.0, self.0 + self.1)
    }
}

impl SourceSpan {
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

/// Parse a text script into the datalog AST.
///
/// * `src` - the script to parse
///
/// * `param_pool` - the list of parameters to execute the script with. These are substituted into the syntax tree during parsing.
///
/// * `fixed_rules` - a mapping of fixed rule names to their implementations. These are substituted into the syntax tree during parsing.
///
/// * `cur_vld` - the current timestamp, substituted into expressions where validity is relevant.
pub fn parse_script(
    src: &str,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<DatalogScript> {
    let parsed = DatalogParser::parse(Rule::script, src)
        .map_err(|err| {
            let span = match err.location {
                InputLocation::Pos(p) => SourceSpan(p, 0),
                InputLocation::Span((start, end)) => SourceSpan(start, end - start),
            };
            error::SyntaxSnafu {
                span: span.to_string(),
                message: err.to_string(),
            }
            .build()
        })?
        .next()
        .expect("pest guarantees a script token after successful parse");
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
        _ => unreachable!(),
    })
}

trait ExtractSpan {
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
