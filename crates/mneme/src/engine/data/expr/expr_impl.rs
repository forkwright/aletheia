//! Expr type and all its implementations.
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display, Formatter};
use std::mem;

use compact_str::CompactString;
use itertools::Itertools;

use crate::engine::data::functions::*;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::{DataValue, LARGEST_UTF_CHAR};
use crate::engine::error::{InternalError, InternalResult as Result};
use crate::engine::parse::SourceSpan;
use crate::engine::parse::expr::expr2bytecode;

use super::super::error::*;
use super::{Bytecode, Op, ValueRange, tuple_too_short_err, unbound_variable_err};

/// Expression can be evaluated to yield a DataValue
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum Expr {
    /// Binding to variables
    Binding {
        /// The variable name to bind
        var: Symbol,
        /// When executing in the context of a tuple, the position of the binding within the tuple
        tuple_pos: Option<usize>,
    },
    /// Constant expression containing a value
    Const {
        /// The value
        val: DataValue,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
    /// Function application
    Apply {
        /// Op representing the function to apply
        op: &'static Op,
        /// Arguments to the application
        args: Box<[Expr]>,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
    /// Unbound function application
    UnboundApply {
        /// Op representing the function to apply
        op: CompactString,
        /// Arguments to the application
        args: Box<[Expr]>,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
    /// Conditional expressions
    Cond {
        /// Conditional clauses, the first expression in each tuple should evaluate to a boolean
        clauses: Vec<(Expr, Expr)>,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
}

impl Debug for Expr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl Display for Expr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::Binding { var, .. } => {
                write!(f, "{}", var.name)
            }
            Expr::Const { val, .. } => {
                write!(f, "{val}")
            }
            Expr::Apply { op, args, .. } => {
                let mut writer = f.debug_tuple(
                    op.name
                        .strip_prefix("OP_")
                        .expect("all operator names are prefixed with OP_")
                        .to_lowercase()
                        .as_str(),
                );
                for arg in args.iter() {
                    writer.field(arg);
                }
                writer.finish()
            }
            Expr::UnboundApply { op, args, .. } => {
                let mut writer = f.debug_tuple(op);
                for arg in args.iter() {
                    writer.field(arg);
                }
                writer.finish()
            }
            Expr::Cond { clauses, .. } => {
                let mut writer = f.debug_tuple("cond");
                for (cond, expr) in clauses {
                    writer.field(cond);
                    writer.field(expr);
                }
                writer.finish()
            }
        }
    }
}

pub(crate) fn no_implementation_err(op: &str) -> DataError {
    NotImplementedSnafu {
        message: format!("No implementation found for op `{op}`"),
    }
    .build()
}

impl Expr {
    pub(crate) fn compile(&self) -> Result<Vec<Bytecode>> {
        let mut collector = vec![];
        expr2bytecode(self, &mut collector)?;
        Ok(collector)
    }
    pub(crate) fn span(&self) -> SourceSpan {
        match self {
            Expr::Binding { var, .. } => var.span,
            Expr::Const { span, .. } | Expr::Apply { span, .. } | Expr::Cond { span, .. } => *span,
            Expr::UnboundApply { span, .. } => *span,
        }
    }
    pub(crate) fn get_binding(&self) -> Option<&Symbol> {
        if let Expr::Binding { var, .. } = self {
            Some(var)
        } else {
            None
        }
    }
    pub(crate) fn get_const(&self) -> Option<&DataValue> {
        if let Expr::Const { val, .. } = self {
            Some(val)
        } else {
            None
        }
    }
    pub(crate) fn build_equate(exprs: Vec<Expr>, span: SourceSpan) -> Self {
        Expr::Apply {
            op: &OP_EQ,
            args: exprs.into(),
            span,
        }
    }
    pub(crate) fn build_and(exprs: Vec<Expr>, span: SourceSpan) -> Self {
        Expr::Apply {
            op: &OP_AND,
            args: exprs.into(),
            span,
        }
    }
    pub(crate) fn build_is_in(exprs: Vec<Expr>, span: SourceSpan) -> Self {
        Expr::Apply {
            op: &OP_IS_IN,
            args: exprs.into(),
            span,
        }
    }
    pub(crate) fn negate(self, span: SourceSpan) -> Self {
        Expr::Apply {
            op: &OP_NEGATE,
            args: Box::new([self]),
            span,
        }
    }
    pub(crate) fn to_conjunction(&self) -> Vec<Self> {
        match self {
            Expr::Apply { op, args, .. } if **op == OP_AND => args.to_vec(),
            v => vec![v.clone()],
        }
    }
    pub(crate) fn fill_binding_indices(
        &mut self,
        binding_map: &BTreeMap<Symbol, usize>,
    ) -> Result<()> {
        match self {
            Expr::Binding { var, tuple_pos, .. } => {
                let found_idx = *binding_map.get(var).ok_or_else(|| {
                    InternalError::from(
                        UnboundVariableSnafu {
                            message: format!("Cannot find binding {}", var),
                        }
                        .build(),
                    )
                })?;
                *tuple_pos = Some(found_idx)
            }
            // NOTE: constants have no variable bindings to process
            Expr::Const { .. } => {}
            Expr::Apply { args, .. } => {
                for arg in args.iter_mut() {
                    arg.fill_binding_indices(binding_map)?;
                }
            }
            Expr::Cond { clauses, .. } => {
                for (cond, val) in clauses {
                    cond.fill_binding_indices(binding_map)?;
                    val.fill_binding_indices(binding_map)?;
                }
            }
            Expr::UnboundApply { op, .. } => {
                return Err(no_implementation_err(op).into());
            }
        }
        Ok(())
    }
    pub(crate) fn binding_indices(&self) -> Result<BTreeSet<usize>> {
        let mut ret = BTreeSet::default();
        self.do_binding_indices(&mut ret)?;
        Ok(ret)
    }
    fn do_binding_indices(&self, coll: &mut BTreeSet<usize>) -> Result<()> {
        match self {
            Expr::Binding { tuple_pos, .. } => {
                if let Some(idx) = tuple_pos {
                    coll.insert(*idx);
                }
            }
            // NOTE: constants have no variable bindings to process
            Expr::Const { .. } => {}
            Expr::Apply { args, .. } => {
                for arg in args.iter() {
                    arg.do_binding_indices(coll)?;
                }
            }
            Expr::Cond { clauses, .. } => {
                for (cond, val) in clauses {
                    cond.do_binding_indices(coll)?;
                    val.do_binding_indices(coll)?;
                }
            }
            Expr::UnboundApply { op, .. } => {
                return Err(no_implementation_err(op).into());
            }
        }
        Ok(())
    }
    /// Evaluate the expression to a constant value if possible
    pub fn eval_to_const(mut self) -> Result<DataValue> {
        self.partial_eval()?;
        match self {
            Expr::Const { val, .. } => Ok(val),
            _ => Err(InvalidValueSnafu {
                message: "Expression contains unevaluated constant".to_string(),
            }
            .build()
            .into()),
        }
    }
    pub(crate) fn partial_eval(&mut self) -> Result<()> {
        if let Expr::Apply { args, span, .. } = self {
            let span = *span;
            let mut all_evaluated = true;
            for arg in args.iter_mut() {
                arg.partial_eval()?;
                all_evaluated = all_evaluated && matches!(arg, Expr::Const { .. });
            }
            if all_evaluated {
                let result = self.eval(vec![])?;
                *self = Expr::Const { val: result, span };
            }
            if let Expr::Apply {
                op: op1,
                args: arg1,
                ..
            } = self
                && op1.name == OP_NEGATE.name
                && let Some(Expr::Apply {
                    op: op2,
                    args: arg2,
                    ..
                }) = arg1.first()
                && op2.name == OP_NEGATE.name
            {
                let mut new_self = arg2[0].clone();
                mem::swap(self, &mut new_self);
            }
        }
        Ok(())
    }
    pub(crate) fn bindings(&self) -> Result<BTreeSet<Symbol>> {
        let mut ret = BTreeSet::new();
        self.collect_bindings(&mut ret)?;
        Ok(ret)
    }
    pub(crate) fn collect_bindings(&self, coll: &mut BTreeSet<Symbol>) -> Result<()> {
        match self {
            Expr::Binding { var, .. } => {
                coll.insert(var.clone());
            }
            // NOTE: constants have no variable bindings to process
            Expr::Const { .. } => {}
            Expr::Apply { args, .. } => {
                for arg in args.iter() {
                    arg.collect_bindings(coll)?;
                }
            }
            Expr::Cond { clauses, .. } => {
                for (cond, val) in clauses {
                    cond.collect_bindings(coll)?;
                    val.collect_bindings(coll)?;
                }
            }
            Expr::UnboundApply { op, .. } => {
                return Err(no_implementation_err(op).into());
            }
        }
        Ok(())
    }
    pub(crate) fn eval(&self, bindings: impl AsRef<[DataValue]>) -> Result<DataValue> {
        match self {
            Expr::Binding { var, tuple_pos, .. } => match tuple_pos {
                None => Err(unbound_variable_err(&var.name).into()),
                Some(i) => Ok(bindings
                    .as_ref()
                    .get(*i)
                    .ok_or_else(|| {
                        InternalError::from(tuple_too_short_err(
                            &var.name,
                            *i,
                            bindings.as_ref().len(),
                        ))
                    })?
                    .clone()),
            },
            Expr::Const { val, .. } => Ok(val.clone()),
            Expr::Apply { op, args, .. } => {
                let args: Box<[DataValue]> = args
                    .iter()
                    .map(|v| v.eval(bindings.as_ref()))
                    .try_collect()?;
                Ok((op.inner)(&args)?)
            }
            Expr::Cond { clauses, .. } => {
                for (cond, val) in clauses {
                    let cond_val = cond.eval(bindings.as_ref())?;
                    let cond_val = cond_val.get_bool().ok_or_else(|| {
                        InternalError::from(
                            TypeMismatchSnafu {
                                op: "predicate evaluation".to_string(),
                                expected: format!("a boolean value, got {:?}", cond_val),
                            }
                            .build(),
                        )
                    })?;

                    if cond_val {
                        return val.eval(bindings.as_ref());
                    }
                }
                Ok(DataValue::Null)
            }
            Expr::UnboundApply { op, .. } => Err(no_implementation_err(op).into()),
        }
    }
    pub(crate) fn extract_bound(&self, target: &Symbol) -> Result<ValueRange> {
        Ok(match self {
            Expr::Binding { .. } | Expr::Const { .. } | Expr::Cond { .. } => ValueRange::default(),
            Expr::Apply { op, args, .. } => match op.name {
                n if n == OP_GE.name || n == OP_GT.name => {
                    if let Some(symb) = args[0].get_binding()
                        && let Some(val) = args[1].get_const()
                        && target == symb
                    {
                        let tar_val = match val.get_int() {
                            Some(i) => DataValue::from(i),
                            None => val.clone(),
                        };
                        return Ok(ValueRange::lower_bound(tar_val));
                    }
                    if let Some(symb) = args[1].get_binding()
                        && let Some(val) = args[0].get_const()
                        && target == symb
                    {
                        let tar_val = match val.get_float() {
                            Some(i) => DataValue::from(i),
                            None => val.clone(),
                        };
                        return Ok(ValueRange::upper_bound(tar_val));
                    }
                    ValueRange::default()
                }
                n if n == OP_LE.name || n == OP_LT.name => {
                    if let Some(symb) = args[0].get_binding()
                        && let Some(val) = args[1].get_const()
                        && target == symb
                    {
                        let tar_val = match val.get_float() {
                            Some(i) => DataValue::from(i),
                            None => val.clone(),
                        };

                        return Ok(ValueRange::upper_bound(tar_val));
                    }
                    if let Some(symb) = args[1].get_binding()
                        && let Some(val) = args[0].get_const()
                        && target == symb
                    {
                        let tar_val = match val.get_int() {
                            Some(i) => DataValue::from(i),
                            None => val.clone(),
                        };

                        return Ok(ValueRange::lower_bound(tar_val));
                    }
                    ValueRange::default()
                }
                n if n == OP_STARTS_WITH.name => {
                    if let Some(symb) = args[0].get_binding()
                        && let Some(val) = args[1].get_const()
                        && target == symb
                    {
                        let s = val.get_str().ok_or_else(|| {
                            InternalError::from(
                                TypeMismatchSnafu {
                                    op: "prefix scan".to_string(),
                                    expected: format!("a string value, got {:?}", val),
                                }
                                .build(),
                            )
                        })?;
                        let lower = DataValue::from(s);
                        let mut upper = CompactString::from(s);
                        upper.push(LARGEST_UTF_CHAR);
                        let upper = DataValue::Str(upper);
                        return Ok(ValueRange::new(lower, upper));
                    }
                    ValueRange::default()
                }
                _ => ValueRange::default(),
            },
            Expr::UnboundApply { op, .. } => {
                return Err(no_implementation_err(op).into());
            }
        })
    }
    #[expect(
        dead_code,
        reason = "utility method for variable introspection, retained for future analysis"
    )]
    pub(crate) fn get_variables(&self) -> Result<BTreeSet<String>> {
        let mut ret = BTreeSet::new();
        self.do_get_variables(&mut ret)?;
        Ok(ret)
    }
    fn do_get_variables(&self, coll: &mut BTreeSet<String>) -> Result<()> {
        match self {
            Expr::Binding { var, .. } => {
                coll.insert(var.to_string());
            }
            // NOTE: constants have no variable bindings to process
            Expr::Const { .. } => {}
            Expr::Apply { args, .. } => {
                for arg in args.iter() {
                    arg.do_get_variables(coll)?;
                }
            }
            Expr::Cond { clauses, .. } => {
                for (cond, act) in clauses.iter() {
                    cond.do_get_variables(coll)?;
                    act.do_get_variables(coll)?;
                }
            }
            Expr::UnboundApply { op, .. } => {
                return Err(no_implementation_err(op).into());
            }
        }
        Ok(())
    }
    pub(crate) fn to_var_list(&self) -> Result<Vec<CompactString>> {
        match self {
            Expr::Apply { op, args, .. } => {
                if op.name != "OP_LIST" {
                    Err(InvalidValueSnafu {
                        message: format!("Invalid fields op: {} for {}", op.name, self),
                    }
                    .build()
                    .into())
                } else {
                    let mut collected = vec![];
                    for field in args.iter() {
                        match field {
                            Expr::Binding { var, .. } => collected.push(var.name.clone()),
                            _ => {
                                return Err(InvalidValueSnafu {
                                    message: format!("Invalid field element: {}", field),
                                }
                                .build()
                                .into());
                            }
                        }
                    }
                    Ok(collected)
                }
            }
            Expr::Binding { var, .. } => Ok(vec![var.name.clone()]),
            _ => Err(InvalidValueSnafu {
                message: format!("Invalid fields: {}", self),
            }
            .build()
            .into()),
        }
    }
}

pub(crate) fn compute_bounds(
    filters: &[Expr],
    symbols: &[Symbol],
) -> Result<(Vec<DataValue>, Vec<DataValue>)> {
    let mut lowers = vec![];
    let mut uppers = vec![];
    for current in symbols {
        let mut cur_bound = ValueRange::default();
        for filter in filters {
            let nxt = filter.extract_bound(current)?;
            cur_bound = cur_bound.merge(nxt);
        }
        lowers.push(cur_bound.lower);
        uppers.push(cur_bound.upper);
    }

    Ok((lowers, uppers))
}
