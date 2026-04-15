//! Expression-to-bytecode compilation.
//!
//! Walks an [`Expr`] AST and emits a flat [`Bytecode`] instruction sequence
//! suitable for the VM. Conditional expressions use jump/goto with post-hoc
//! target fixup.
#![expect(
    clippy::indexing_slicing,
    clippy::pedantic,
    clippy::result_large_err,
    reason = "Bytecode compiler — indices are computed from collector.len() and always valid, InternalError is the crate-wide Result type"
)]

use crate::data::expr::{Bytecode, Expr, no_implementation_err};
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;

/// Compile an expression tree into a flat bytecode sequence.
///
/// Walks the AST depth-first, emitting [`Bytecode`] instructions into
/// `collector`. Conditional expressions emit jump/goto with fixup.
///
/// # Errors
///
/// Returns an error if the expression references an unbound function name.
pub(crate) fn expr2bytecode(expr: &Expr, collector: &mut Vec<Bytecode>) -> Result<()> {
    match expr {
        Expr::Binding { var, tuple_pos } => collector.push(Bytecode::Binding {
            var: var.clone(),
            tuple_pos: *tuple_pos,
        }),
        Expr::Const { val, span } => collector.push(Bytecode::Const {
            val: val.clone(),
            span: *span,
        }),
        Expr::Apply { op, args, span } => {
            let arity = args.len();
            for arg in args.iter() {
                expr2bytecode(arg, collector)?;
            }
            collector.push(Bytecode::Apply {
                op,
                arity,
                span: *span,
            })
        }
        Expr::Cond { clauses, span } => {
            compile_cond_bytecode(clauses, *span, collector)?;
        }
        Expr::UnboundApply { op, .. } => {
            return Err(no_implementation_err(op).into());
        }
    }
    Ok(())
}

/// Emit bytecode for a conditional (`cond`/`if`) expression.
///
/// Each clause becomes: evaluate condition, `JumpIfFalse` past the body,
/// evaluate body, `Goto` past remaining clauses. Jump targets are fixed up
/// after all clauses are emitted.
fn compile_cond_bytecode(
    clauses: &[(Expr, Expr)],
    span: SourceSpan,
    collector: &mut Vec<Bytecode>,
) -> Result<()> {
    let mut return_jump_pos = vec![];
    for (cond, val) in clauses {
        expr2bytecode(cond, collector)?;
        collector.push(Bytecode::JumpIfFalse {
            jump_to: 0,
            span,
        });
        let false_jump_amend_pos = collector.len() - 1;
        expr2bytecode(val, collector)?;
        collector.push(Bytecode::Goto {
            jump_to: 0,
            span,
        });
        return_jump_pos.push(collector.len() - 1);
        // SAFETY: `false_jump_amend_pos` is `collector.len() - 1` from above,
        // so it is always a valid index.
        collector[false_jump_amend_pos] = Bytecode::JumpIfFalse {
            jump_to: collector.len(),
            span,
        };
    }
    let total_len = collector.len();
    for pos in return_jump_pos {
        // SAFETY: `pos` values are `collector.len() - 1` values pushed above,
        // so they are always valid indices.
        collector[pos] = Bytecode::Goto {
            jump_to: total_len,
            span,
        }
    }
    Ok(())
}
