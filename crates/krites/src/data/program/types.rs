use std::fmt::{Debug, Display, Formatter};

use crate::data::relation::StoredRelationMetadata;
use crate::data::symb::Symbol;
use crate::parse::SourceSpan;
use crate::runtime::relation::InputRelationHandle;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum QueryAssertion {
    AssertNone(SourceSpan),
    AssertSome(SourceSpan),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum ReturnMutation {
    NotReturning,
    Returning,
}

#[derive(Clone, PartialEq, Default)]
pub(crate) struct QueryOutOptions {
    pub(crate) limit: Option<usize>,
    pub(crate) offset: Option<usize>,
    pub(crate) timeout: Option<f64>,
    pub(crate) sleep: Option<f64>,
    pub(crate) sorters: Vec<(Symbol, SortDir)>,
    pub(crate) store_relation: Option<(InputRelationHandle, RelationOp, ReturnMutation)>,
    pub(crate) assertion: Option<QueryAssertion>,
}

impl Debug for QueryOutOptions {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl Display for QueryOutOptions {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(l) = self.limit {
            writeln!(f, ":limit {l};")?;
        }
        if let Some(l) = self.offset {
            writeln!(f, ":offset {l};")?;
        }
        if let Some(l) = self.timeout {
            writeln!(f, ":timeout {l};")?;
        }
        for (symb, dir) in &self.sorters {
            write!(f, ":order ")?;
            if *dir == SortDir::Dsc {
                write!(f, "-")?;
            }
            writeln!(f, "{symb};")?;
        }
        if let Some((
            InputRelationHandle {
                name,
                metadata: StoredRelationMetadata { keys, non_keys },
                key_bindings,
                dep_bindings,
                ..
            },
            op,
            return_mutation,
        )) = &self.store_relation
        {
            if *return_mutation == ReturnMutation::Returning {
                writeln!(f, ":returning")?;
            }
            match op {
                RelationOp::Create => {
                    write!(f, ":create ")?;
                }
                RelationOp::Replace => {
                    write!(f, ":replace ")?;
                }
                RelationOp::Insert => {
                    write!(f, ":insert ")?;
                }
                RelationOp::Put => {
                    write!(f, ":put ")?;
                }
                RelationOp::Update => {
                    write!(f, ":update ")?;
                }
                RelationOp::Rm => {
                    write!(f, ":rm ")?;
                }
                RelationOp::Delete => {
                    write!(f, ":delete ")?;
                }
                RelationOp::Ensure => {
                    write!(f, ":ensure ")?;
                }
                RelationOp::EnsureNot => {
                    write!(f, ":ensure_not ")?;
                }
            }
            write!(f, "{name} {{")?;
            let mut is_first = true;
            for (col, bind) in keys.iter().zip(key_bindings) {
                if is_first {
                    is_first = false
                } else {
                    write!(f, ", ")?;
                }
                write!(f, "{}: {}", col.name, col.typing)?;
                if let Some(default_gen) = &col.default_gen {
                    write!(f, " default {default_gen}")?;
                } else {
                    write!(f, " = {bind}")?;
                }
            }
            write!(f, " => ")?;
            let mut is_first = true;
            for (col, bind) in non_keys.iter().zip(dep_bindings) {
                if is_first {
                    is_first = false
                } else {
                    write!(f, ", ")?;
                }
                write!(f, "{}: {}", col.name, col.typing)?;
                if let Some(default_gen) = &col.default_gen {
                    write!(f, " default {default_gen}")?;
                } else {
                    write!(f, " = {bind}")?;
                }
            }
            writeln!(f, "}};")?;
        }

        if let Some(a) = &self.assertion {
            match a {
                QueryAssertion::AssertNone(_) => {
                    writeln!(f, ":assert none;")?;
                }
                QueryAssertion::AssertSome(_) => {
                    writeln!(f, ":assert some;")?;
                }
            }
        }

        Ok(())
    }
}

impl QueryOutOptions {
    pub(crate) fn num_to_take(&self) -> Option<usize> {
        match (self.limit, self.offset) {
            (None, _) => None,
            (Some(i), None) => Some(i),
            (Some(i), Some(j)) => Some(i + j),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum SortDir {
    Asc,
    Dsc,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum RelationOp {
    Create,
    Replace,
    Put,
    Insert,
    Update,
    Rm,
    Delete,
    Ensure,
    EnsureNot,
}

#[derive(Default)]
pub(crate) struct TempSymbGen {
    last_id: u32,
}

impl TempSymbGen {
    pub(crate) fn next(&mut self, span: SourceSpan) -> Symbol {
        self.last_id += 1;
        Symbol::new(&format!("*{}", self.last_id) as &str, span)
    }
    pub(crate) fn next_ignored(&mut self, span: SourceSpan) -> Symbol {
        self.last_id += 1;
        Symbol::new(&format!("~{}", self.last_id) as &str, span)
    }
}

#[derive(Debug)]
pub(crate) struct NoEntryError;

impl std::fmt::Display for NoEntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Program has no entry")
    }
}

impl std::error::Error for NoEntryError {}
