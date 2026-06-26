//! Z3 SMT solver tool: deterministic reasoning handoff for SMT-LIB2 formulas.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::workspace::{extract_opt_u64, extract_str};

/// Maximum timeout in milliseconds.
const MAX_TIMEOUT_MS: u64 = 60_000;
/// Default timeout in milliseconds.
const DEFAULT_TIMEOUT_MS: u64 = 5_000;

struct Z3SolverExecutor;

impl ToolExecutor for Z3SolverExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let formula = extract_str(&input.arguments, "formula", &input.name)?;
            let timeout_ms = extract_opt_u64(&input.arguments, "timeout_ms")
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .min(MAX_TIMEOUT_MS);

            let formula = formula.to_owned();
            let name = input.name.clone();

            let result = tokio::task::spawn_blocking(move || solve_smt(&formula, timeout_ms))
                .await
                .map_err(|e| {
                    crate::error::ExecutionFailedSnafu {
                        name: name.clone(),
                        message: format!("z3 solver task panicked: {e}"),
                    }
                    .build()
                })?
                .map_err(|message| crate::error::ExecutionFailedSnafu { name, message }.build())?;

            Ok(ToolResult::text(result))
        })
    }
}

/// Validate basic SMT-LIB2 syntax: no null bytes and balanced parentheses.
fn validate_smtlib2(formula: &str) -> std::result::Result<(), String> {
    if formula.contains('\0') {
        return Err("formula contains null bytes".to_owned());
    }
    let mut depth = 0i32;
    for ch in formula.chars() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth < 0 {
                return Err("unbalanced parentheses in formula".to_owned());
            }
        }
    }
    if depth != 0 {
        return Err("unbalanced parentheses in formula".to_owned());
    }
    Ok(())
}

fn solve_smt(formula: &str, timeout_ms: u64) -> std::result::Result<String, String> {
    if let Err(reason) = validate_smtlib2(formula) {
        return Err(format!("malformed SMT-LIB2 formula: {reason}"));
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut cfg = z3::Config::new();
        cfg.set_timeout_msec(timeout_ms);
        z3::with_z3_config(&cfg, || {
            let solver = z3::Solver::new();
            solver.from_string(formula.to_owned());
            match solver.check() {
                z3::SatResult::Sat => {
                    let model = solver.get_model();
                    let model_str = model.map(|m| m.to_string()).unwrap_or_default();
                    serde_json::json!({
                        "status": "sat",
                        "model": model_str,
                    })
                }
                z3::SatResult::Unsat => {
                    serde_json::json!({
                        "status": "unsat",
                        "model": "",
                    })
                }
                z3::SatResult::Unknown => {
                    serde_json::json!({
                        "status": "unknown",
                        "model": "",
                    })
                }
            }
        })
    }));

    match result {
        Ok(output) => {
            serde_json::to_string(&output).map_err(|e| format!("failed to serialize result: {e}"))
        }
        Err(_) => Err("malformed SMT-LIB2 formula".to_owned()),
    }
}

fn z3_solver_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("z3_solver"),
        description: "Solve an SMT-LIB2 formula using the Z3 solver and return sat/unsat/unknown plus a model when sat."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "formula".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "SMT-LIB2 source string to evaluate".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "timeout_ms".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description:
                            "Solver timeout in milliseconds (default: 5000, max: 60000)"
                                .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(DEFAULT_TIMEOUT_MS)),
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["formula".to_owned()],
        },
        category: ToolCategory::Research,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify],
    }
}

/// Register the `z3_solver` tool into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(z3_solver_def(), Box::new(Z3SolverExecutor))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolExecutor;
    use crate::types::{ToolContext, ToolInput};

    use super::*;

    fn mock_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn make_input(formula: &str, timeout_ms: Option<u64>) -> ToolInput {
        let mut args = serde_json::Map::new();
        args.insert(
            "formula".to_owned(),
            serde_json::Value::String(formula.to_owned()),
        );
        if let Some(t) = timeout_ms {
            args.insert("timeout_ms".to_owned(), serde_json::json!(t));
        }
        ToolInput {
            name: ToolName::from_static("z3_solver"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::Value::Object(args),
        }
    }

    #[tokio::test]
    async fn test_sat_trivial() {
        let ctx = mock_ctx();
        let executor = Z3SolverExecutor;
        let input = make_input("(assert (= 1 1))", None);

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected success");
        let text = result.content.text_summary();
        assert!(
            text.contains("\"status\":\"sat\""),
            "expected sat status: {text}"
        );
    }

    #[tokio::test]
    async fn test_unsat_contradiction() {
        let ctx = mock_ctx();
        let executor = Z3SolverExecutor;
        let input = make_input(
            "(declare-const x Int) (assert (= x 1)) (assert (= x 2)) (check-sat)",
            None,
        );

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected success");
        let text = result.content.text_summary();
        assert!(
            text.contains("\"status\":\"unsat\""),
            "expected unsat status: {text}"
        );
    }

    #[tokio::test]
    async fn test_timeout_respected() {
        let ctx = mock_ctx();
        let executor = Z3SolverExecutor;
        // A deliberately hard formula: 256-bit bit-vector factorization.
        let hard_formula = r"
(declare-const x (_ BitVec 256))
(declare-const y (_ BitVec 256))
(assert (= (bvmul x y) (_ bv340282366920938463463374607431768211457 256)))
(assert (not (= x (_ bv1 256))))
(check-sat)
";
        let input = make_input(hard_formula, Some(100));

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected success");
        let text = result.content.text_summary();
        assert!(
            text.contains("\"status\":\"unknown\""),
            "expected unknown status due to timeout: {text}"
        );
    }

    #[tokio::test]
    async fn test_malformed_formula_returns_err() {
        let ctx = mock_ctx();
        let executor = Z3SolverExecutor;
        let input = make_input("(assert (= 1", None);

        let result = executor.execute(&input, &ctx).await;
        assert!(result.is_err(), "expected Err for malformed formula");
        let Err(err) = result else {
            panic!("expected Err for malformed formula");
        };
        let err_msg = format!("{err:?}");
        assert!(
            err_msg.contains("malformed") || err_msg.contains("unbalanced"),
            "expected diagnostic message about malformed formula: {err_msg}"
        );
    }
}
