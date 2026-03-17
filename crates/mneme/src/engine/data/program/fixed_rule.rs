use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

use compact_str::CompactString;

use crate::engine::data::error::*;
use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::ValidityTs;
use crate::engine::error::{InternalError, InternalResult as Result};
use crate::engine::fixed_rule::{FixedRule, FixedRuleHandle};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::temp_store::EpochStore;
use crate::engine::runtime::transact::SessionTx;

use super::magic::MagicSymbol;

#[derive(Clone)]
pub(crate) struct FixedRuleApply {
    pub(crate) fixed_handle: FixedRuleHandle,
    pub(crate) rule_args: Vec<FixedRuleArg>,
    pub(crate) options: Arc<BTreeMap<CompactString, Expr>>,
    pub(crate) head: Vec<Symbol>,
    pub(crate) arity: usize,
    pub(crate) span: SourceSpan,
    pub(crate) fixed_impl: Arc<Box<dyn FixedRule>>,
}

impl FixedRuleApply {
    pub(crate) fn arity(&self) -> Result<usize> {
        self.fixed_impl
            .as_ref()
            .arity(&self.options, &self.head, self.span)
    }
}

impl Debug for FixedRuleApply {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FixedRuleApply")
            .field("name", &self.fixed_handle.name)
            .field("rules", &self.rule_args)
            .field("options", &self.options)
            .finish()
    }
}

pub(crate) struct MagicFixedRuleApply {
    pub(crate) fixed_handle: FixedRuleHandle,
    pub(crate) rule_args: Vec<MagicFixedRuleRuleArg>,
    pub(crate) options: Arc<BTreeMap<CompactString, Expr>>,
    pub(crate) span: SourceSpan,
    pub(crate) arity: usize,
    pub(crate) fixed_impl: Arc<Box<dyn FixedRule>>,
}

#[derive(Debug)]
pub(crate) struct FixedRuleOptionNotFoundError {
    pub(crate) name: String,
    #[expect(
        dead_code,
        reason = "structural field preserved for diagnostic context"
    )]
    pub(crate) span: SourceSpan,
    pub(crate) rule_name: String,
}

impl std::fmt::Display for FixedRuleOptionNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Cannot find a required named option '{}' for '{}'",
            self.name, self.rule_name
        )
    }
}

impl std::error::Error for FixedRuleOptionNotFoundError {}

#[derive(Debug)]
pub(crate) struct WrongFixedRuleOptionError {
    pub(crate) name: String,
    #[expect(
        dead_code,
        reason = "structural field preserved for diagnostic context"
    )]
    pub(crate) span: SourceSpan,
    pub(crate) rule_name: String,
    pub(crate) help: String,
}

impl std::fmt::Display for WrongFixedRuleOptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Wrong value for option '{}' of '{}'",
            self.name, self.rule_name
        )
    }
}

impl std::error::Error for WrongFixedRuleOptionError {}

impl MagicFixedRuleApply {
    #[expect(
        dead_code,
        reason = "validation helper retained for fixed rule implementors"
    )]
    pub(crate) fn relation_with_min_len(
        &self,
        idx: usize,
        len: usize,
        tx: &SessionTx<'_>,
        stores: &BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<&MagicFixedRuleRuleArg> {
        let rel = self.relation(idx)?;
        let arity = rel.arity(tx, stores)?;
        if arity < len {
            return Err(ProgramConstraintSnafu {
                message: "Input relation to fixed rule has insufficient arity".to_string(),
            }
            .build()
            .into());
        }
        Ok(rel)
    }
    pub(crate) fn relations_count(&self) -> usize {
        self.rule_args.len()
    }
    pub(crate) fn relation(&self, idx: usize) -> Result<&MagicFixedRuleRuleArg> {
        self.rule_args.get(idx).ok_or_else(|| {
            InternalError::from(
                ProgramConstraintSnafu {
                    message: format!(
                        "Cannot find a required positional argument at index {} for '{}'",
                        idx, self.fixed_handle.name
                    ),
                }
                .build(),
            )
        })
    }
}
impl Debug for MagicFixedRuleApply {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FixedRuleApply")
            .field("name", &self.fixed_handle.name)
            .field("rules", &self.rule_args)
            .field("options", &self.options)
            .finish()
    }
}
#[derive(Clone)]
pub(crate) enum FixedRuleArg {
    InMem {
        name: Symbol,
        bindings: Vec<Symbol>,
        span: SourceSpan,
    },
    Stored {
        name: Symbol,
        bindings: Vec<Symbol>,
        valid_at: Option<ValidityTs>,
        span: SourceSpan,
    },
    NamedStored {
        name: Symbol,
        bindings: BTreeMap<CompactString, Symbol>,
        valid_at: Option<ValidityTs>,
        span: SourceSpan,
    },
}
impl Debug for FixedRuleArg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}
impl Display for FixedRuleArg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FixedRuleArg::InMem { name, bindings, .. } => {
                write!(f, "{name}")?;
                f.debug_list().entries(bindings).finish()?;
            }
            FixedRuleArg::Stored { name, bindings, .. } => {
                write!(f, ":{name}")?;
                f.debug_list().entries(bindings).finish()?;
            }
            FixedRuleArg::NamedStored { name, bindings, .. } => {
                write!(f, "*")?;
                let mut sf = f.debug_struct(name);
                for (k, v) in bindings {
                    sf.field(k, v);
                }
                sf.finish()?;
            }
        }
        Ok(())
    }
}
#[derive(Debug)]
pub(crate) enum MagicFixedRuleRuleArg {
    InMem {
        name: MagicSymbol,
        bindings: Vec<Symbol>,
        span: SourceSpan,
    },
    Stored {
        name: Symbol,
        bindings: Vec<Symbol>,
        valid_at: Option<ValidityTs>,
        span: SourceSpan,
    },
}
impl MagicFixedRuleRuleArg {
    pub(crate) fn bindings(&self) -> &[Symbol] {
        match self {
            MagicFixedRuleRuleArg::InMem { bindings, .. }
            | MagicFixedRuleRuleArg::Stored { bindings, .. } => bindings,
        }
    }
    pub(crate) fn span(&self) -> SourceSpan {
        match self {
            MagicFixedRuleRuleArg::InMem { span, .. }
            | MagicFixedRuleRuleArg::Stored { span, .. } => *span,
        }
    }
    pub(crate) fn get_binding_map(&self, starting: usize) -> BTreeMap<Symbol, usize> {
        let bindings = match self {
            MagicFixedRuleRuleArg::InMem { bindings, .. }
            | MagicFixedRuleRuleArg::Stored { bindings, .. } => bindings,
        };
        bindings
            .iter()
            .enumerate()
            .map(|(idx, symb)| (symb.clone(), idx + starting))
            .collect()
    }
}
