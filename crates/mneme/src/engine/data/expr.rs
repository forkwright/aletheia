//! Expression evaluation and representation.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::cmp::{max, min};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display, Formatter};
use std::mem;

use super::error::*;
use crate::engine::error::{InternalError, InternalResult as Result};
use compact_str::CompactString;
use itertools::Itertools;
use serde::de::{Error, Visitor};
use serde::{Deserializer, Serializer};

use crate::engine::data::functions::*;
use crate::engine::data::relation::NullableColType;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::{DataValue, LARGEST_UTF_CHAR};
use crate::engine::parse::SourceSpan;
use crate::engine::parse::expr::expr2bytecode;

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Debug)]
pub enum Bytecode {
    /// push 1
    Binding {
        var: Symbol,
        tuple_pos: Option<usize>,
    },
    /// push 1
    Const {
        val: DataValue,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// pop n, push 1
    Apply {
        op: &'static Op,
        arity: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// pop 1
    JumpIfFalse {
        jump_to: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// unchanged
    Goto {
        jump_to: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
}

fn unbound_variable_err(name: &str) -> DataError {
    UnboundVariableSnafu {
        message: format!("The variable '{name}' is unbound"),
    }
    .build()
}

fn tuple_too_short_err(name: &str, index: usize, length: usize) -> DataError {
    InvalidValueSnafu {
        message: format!(
            "The tuple bound by variable '{name}' is too short: index is {index}, length is {length}"
        ),
    }
    .build()
}

pub fn eval_bytecode_pred(
    bytecodes: &[Bytecode],
    bindings: impl AsRef<[DataValue]>,
    stack: &mut Vec<DataValue>,
    _span: SourceSpan,
) -> Result<bool> {
    match eval_bytecode(bytecodes, bindings, stack)? {
        DataValue::Bool(b) => Ok(b),
        v => Err(TypeMismatchSnafu {
            op: "predicate evaluation".to_string(),
            expected: format!("a boolean value, got {:?}", v),
        }
        .build()
        .into()),
    }
}

pub fn eval_bytecode(
    bytecodes: &[Bytecode],
    bindings: impl AsRef<[DataValue]>,
    stack: &mut Vec<DataValue>,
) -> Result<DataValue> {
    stack.clear();
    let mut pointer = 0;
    loop {
        if pointer == bytecodes.len() {
            break;
        }
        let current_instruction = &bytecodes[pointer];
        // println!("{current_instruction:?}");
        match current_instruction {
            Bytecode::Binding { var, tuple_pos, .. } => match tuple_pos {
                None => {
                    return Err(unbound_variable_err(&var.name).into());
                }
                Some(i) => {
                    let val = bindings
                        .as_ref()
                        .get(*i)
                        .ok_or_else(|| {
                            InternalError::from(tuple_too_short_err(
                                &var.name,
                                *i,
                                bindings.as_ref().len(),
                            ))
                        })?
                        .clone();
                    stack.push(val);
                    pointer += 1;
                }
            },
            Bytecode::Const { val, .. } => {
                stack.push(val.clone());
                pointer += 1;
            }
            Bytecode::Apply { op, arity, span: _ } => {
                let frame_start = stack.len() - *arity;
                let args_frame = &stack[frame_start..];
                let result = (op.inner)(args_frame)?;
                stack.truncate(frame_start);
                stack.push(result);
                pointer += 1;
            }
            Bytecode::JumpIfFalse { jump_to, span: _ } => {
                let val = stack
                    .pop()
                    .expect("JumpIfFalse bytecode guarantees a value on the stack");
                let cond = val.get_bool().ok_or_else(|| {
                    InternalError::from(
                        TypeMismatchSnafu {
                            op: "predicate evaluation".to_string(),
                            expected: format!("a boolean value, got {:?}", val),
                        }
                        .build(),
                    )
                })?;
                if cond {
                    pointer += 1;
                } else {
                    pointer = *jump_to;
                }
            }
            Bytecode::Goto { jump_to, .. } => {
                pointer = *jump_to;
            }
        }
    }
    Ok(stack
        .pop()
        .expect("bytecode execution guarantees exactly one result on the stack"))
}

/// Expression can be evaluated to yield a DataValue
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
            // nested not's can accumulate during conversion to normal form
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValueRange {
    pub(crate) lower: DataValue,
    pub(crate) upper: DataValue,
}

impl ValueRange {
    fn merge(self, other: Self) -> Self {
        let lower = max(self.lower, other.lower);
        let upper = min(self.upper, other.upper);
        if lower > upper {
            Self::null()
        } else {
            Self { lower, upper }
        }
    }
    fn null() -> Self {
        Self {
            lower: DataValue::Bot,
            upper: DataValue::Bot,
        }
    }
    fn new(lower: DataValue, upper: DataValue) -> Self {
        Self { lower, upper }
    }
    fn lower_bound(val: DataValue) -> Self {
        Self {
            lower: val,
            upper: DataValue::Bot,
        }
    }
    fn upper_bound(val: DataValue) -> Self {
        Self {
            lower: DataValue::Null,
            upper: val,
        }
    }
}

impl Default for ValueRange {
    fn default() -> Self {
        Self {
            lower: DataValue::Null,
            upper: DataValue::Bot,
        }
    }
}

#[derive(Clone)]
pub struct Op {
    pub(crate) name: &'static str,
    pub(crate) min_arity: usize,
    pub(crate) vararg: bool,
    pub(crate) inner: fn(&[DataValue]) -> DataResult<DataValue>,
}

/// Used as `Arc<dyn CustomOp>`
#[expect(
    dead_code,
    reason = "public extension point for custom operations, no built-in implementors yet"
)]
pub trait CustomOp {
    fn name(&self) -> &'static str;
    fn min_arity(&self) -> usize;
    fn vararg(&self) -> bool;
    fn return_type(&self) -> NullableColType;
    fn call(&self, args: &[DataValue]) -> Result<DataValue>;
}

impl serde::Serialize for &'_ Op {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.name)
    }
}

impl<'de> serde::Deserialize<'de> for &'static Op {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(OpVisitor)
    }
}

struct OpVisitor;

impl<'de> Visitor<'de> for OpVisitor {
    type Value = &'static Op;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("name of the op")
    }

    fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
    where
        E: Error,
    {
        let name = v
            .strip_prefix("OP_")
            .ok_or_else(|| E::custom(format!("op name must start with OP_, got: {v}")))?
            .to_ascii_lowercase();
        get_op(&name).ok_or_else(|| E::custom(format!("op not found in serialized data: {v}")))
    }
}

impl PartialEq for Op {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Op {}

impl Debug for Op {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub(crate) fn get_op(name: &str) -> Option<&'static Op> {
    Some(match name {
        "coalesce" => &OP_COALESCE,
        "list" => &OP_LIST,
        "json" => &OP_JSON,
        "set_json_path" => &OP_SET_JSON_PATH,
        "remove_json_path" => &OP_REMOVE_JSON_PATH,
        "parse_json" => &OP_PARSE_JSON,
        "dump_json" => &OP_DUMP_JSON,
        "json_object" => &OP_JSON_OBJECT,
        "is_json" => &OP_IS_JSON,
        "json_to_scalar" => &OP_JSON_TO_SCALAR,
        "add" => &OP_ADD,
        "sub" => &OP_SUB,
        "mul" => &OP_MUL,
        "div" => &OP_DIV,
        "minus" => &OP_MINUS,
        "abs" => &OP_ABS,
        "signum" => &OP_SIGNUM,
        "floor" => &OP_FLOOR,
        "ceil" => &OP_CEIL,
        "round" => &OP_ROUND,
        "mod" => &OP_MOD,
        "max" => &OP_MAX,
        "min" => &OP_MIN,
        "pow" => &OP_POW,
        "sqrt" => &OP_SQRT,
        "exp" => &OP_EXP,
        "exp2" => &OP_EXP2,
        "ln" => &OP_LN,
        "log2" => &OP_LOG2,
        "log10" => &OP_LOG10,
        "sin" => &OP_SIN,
        "cos" => &OP_COS,
        "tan" => &OP_TAN,
        "asin" => &OP_ASIN,
        "acos" => &OP_ACOS,
        "atan" => &OP_ATAN,
        "atan2" => &OP_ATAN2,
        "sinh" => &OP_SINH,
        "cosh" => &OP_COSH,
        "tanh" => &OP_TANH,
        "asinh" => &OP_ASINH,
        "acosh" => &OP_ACOSH,
        "atanh" => &OP_ATANH,
        "eq" => &OP_EQ,
        "neq" => &OP_NEQ,
        "gt" => &OP_GT,
        "ge" => &OP_GE,
        "lt" => &OP_LT,
        "le" => &OP_LE,
        "or" => &OP_OR,
        "and" => &OP_AND,
        "negate" => &OP_NEGATE,
        "bit_and" => &OP_BIT_AND,
        "bit_or" => &OP_BIT_OR,
        "bit_not" => &OP_BIT_NOT,
        "bit_xor" => &OP_BIT_XOR,
        "pack_bits" => &OP_PACK_BITS,
        "unpack_bits" => &OP_UNPACK_BITS,
        "concat" => &OP_CONCAT,
        "str_includes" => &OP_STR_INCLUDES,
        "lowercase" => &OP_LOWERCASE,
        "uppercase" => &OP_UPPERCASE,
        "trim" => &OP_TRIM,
        "trim_start" => &OP_TRIM_START,
        "trim_end" => &OP_TRIM_END,
        "starts_with" => &OP_STARTS_WITH,
        "ends_with" => &OP_ENDS_WITH,
        "is_null" => &OP_IS_NULL,
        "is_int" => &OP_IS_INT,
        "is_float" => &OP_IS_FLOAT,
        "is_num" => &OP_IS_NUM,
        "is_string" => &OP_IS_STRING,
        "is_list" => &OP_IS_LIST,
        "is_bytes" => &OP_IS_BYTES,
        "is_in" => &OP_IS_IN,
        "is_finite" => &OP_IS_FINITE,
        "is_infinite" => &OP_IS_INFINITE,
        "is_nan" => &OP_IS_NAN,
        "is_uuid" => &OP_IS_UUID,
        "is_vec" => &OP_IS_VEC,
        "length" => &OP_LENGTH,
        "sorted" => &OP_SORTED,
        "reverse" => &OP_REVERSE,
        "append" => &OP_APPEND,
        "prepend" => &OP_PREPEND,
        "unicode_normalize" => &OP_UNICODE_NORMALIZE,
        "haversine" => &OP_HAVERSINE,
        "haversine_deg_input" => &OP_HAVERSINE_DEG_INPUT,
        "deg_to_rad" => &OP_DEG_TO_RAD,
        "rad_to_deg" => &OP_RAD_TO_DEG,
        "get" => &OP_GET,
        "maybe_get" => &OP_MAYBE_GET,
        "chars" => &OP_CHARS,
        "slice_string" => &OP_SLICE_STRING,
        "from_substrings" => &OP_FROM_SUBSTRINGS,
        "slice" => &OP_SLICE,
        "regex_matches" => &OP_REGEX_MATCHES,
        "regex_replace" => &OP_REGEX_REPLACE,
        "regex_replace_all" => &OP_REGEX_REPLACE_ALL,
        "regex_extract" => &OP_REGEX_EXTRACT,
        "regex_extract_first" => &OP_REGEX_EXTRACT_FIRST,
        "t2s" => &OP_T2S,
        "encode_base64" => &OP_ENCODE_BASE64,
        "decode_base64" => &OP_DECODE_BASE64,
        "first" => &OP_FIRST,
        "last" => &OP_LAST,
        "chunks" => &OP_CHUNKS,
        "chunks_exact" => &OP_CHUNKS_EXACT,
        "windows" => &OP_WINDOWS,
        "to_int" => &OP_TO_INT,
        "to_float" => &OP_TO_FLOAT,
        "to_string" => &OP_TO_STRING,
        "l2_dist" => &OP_L2_DIST,
        "l2_normalize" => &OP_L2_NORMALIZE,
        "ip_dist" => &OP_IP_DIST,
        "cos_dist" => &OP_COS_DIST,
        "int_range" => &OP_INT_RANGE,
        "rand_float" => &OP_RAND_FLOAT,
        "rand_bernoulli" => &OP_RAND_BERNOULLI,
        "rand_int" => &OP_RAND_INT,
        "rand_choose" => &OP_RAND_CHOOSE,
        "assert" => &OP_ASSERT,
        "union" => &OP_UNION,
        "intersection" => &OP_INTERSECTION,
        "difference" => &OP_DIFFERENCE,
        "to_uuid" => &OP_TO_UUID,
        "to_bool" => &OP_TO_BOOL,
        "to_unity" => &OP_TO_UNITY,
        "rand_uuid_v1" => &OP_RAND_UUID_V1,
        "rand_uuid_v4" => &OP_RAND_UUID_V4,
        "uuid_timestamp" => &OP_UUID_TIMESTAMP,
        "validity" => &OP_VALIDITY,
        "now" => &OP_NOW,
        "format_timestamp" => &OP_FORMAT_TIMESTAMP,
        "parse_timestamp" => &OP_PARSE_TIMESTAMP,
        "vec" => &OP_VEC,
        "rand_vec" => &OP_RAND_VEC,
        _ => return None,
    })
}

impl Op {
    pub(crate) fn post_process_args(&self, args: &mut [Expr]) {
        if self.name.starts_with("OP_REGEX_") {
            args[1] = Expr::Apply {
                op: &OP_REGEX,
                args: [args[1].clone()].into(),
                span: args[1].span(),
            }
        }
    }
}
