//! Condition evaluation for output-gated prompt DAG nodes.
//!
//! Conditions use a small expression language over recorded structured outputs:
//! `$1.output.severity == 'high'`, `$2.output.score > 0.8`,
//! `$3.output.tags contains 'security'`, and
//! `$4.output.kind in ['feature', 'refactor']`.

use std::collections::HashMap;

use serde_json::Value;

/// Structured outputs available to condition expressions.
#[derive(Debug, Clone, Default)]
pub struct ConditionContext {
    outputs: HashMap<String, Value>,
}

impl ConditionContext {
    /// Create an empty condition context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert structured output for a node identifier.
    pub fn insert_output(&mut self, node: impl Into<String>, output: Value) {
        self.outputs.insert(node.into(), output);
    }

    /// Build a condition context from a numeric prompt-output map.
    #[must_use]
    pub fn from_prompt_outputs(outputs: &HashMap<u32, Value>) -> Self {
        let outputs = outputs
            .iter()
            .map(|(node, output)| (node.to_string(), output.clone()))
            .collect();
        Self { outputs }
    }

    fn resolve_path<'a>(&'a self, path: &str) -> Option<&'a Value> {
        let path = path.strip_prefix('$')?;
        let mut segments = path.split('.');
        let node = segments.next()?;
        if segments.next()? != "output" {
            return None;
        }

        let mut current = self.outputs.get(node)?;
        for segment in segments {
            current = current.get(segment)?;
        }
        Some(current)
    }
}

/// Errors raised while parsing or evaluating a condition.
#[derive(Debug, Clone, PartialEq, Eq, snafu::Snafu)]
#[non_exhaustive]
pub enum ConditionError {
    /// The expression did not match the supported condition grammar.
    #[snafu(display("invalid condition expression: {expression}"))]
    InvalidExpression {
        /// The expression that failed to parse.
        expression: String,
    },

    /// The expression referenced a path that is not present in the context.
    #[snafu(display("condition path not found: {path}"))]
    MissingPath {
        /// Dotted path that could not be resolved.
        path: String,
    },

    /// The expression used an unsupported comparison for the operand types.
    #[snafu(display("unsupported condition comparison: {detail}"))]
    UnsupportedComparison {
        /// Human-readable comparison diagnostic.
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Eq,
    Ne,
    Lt,
    Gt,
    In,
    Contains,
}

/// Evaluate a branch condition against structured node outputs.
///
/// # Errors
///
/// Returns [`ConditionError`] if the expression cannot be parsed, references
/// a missing output path, or uses an unsupported type/operator combination.
pub fn evaluate_condition(
    expression: &str,
    context: &ConditionContext,
) -> Result<bool, ConditionError> {
    let (left, operator, right) = parse_expression(expression)?;
    let Some(left_value) = context.resolve_path(left) else {
        return Err(ConditionError::MissingPath {
            path: left.to_owned(),
        });
    };
    let right_value = parse_literal(right).ok_or_else(|| ConditionError::InvalidExpression {
        expression: expression.to_owned(),
    })?;

    compare(left_value, operator, &right_value)
}

fn parse_expression(expression: &str) -> Result<(&str, Operator, &str), ConditionError> {
    for (needle, operator) in [
        (" contains ", Operator::Contains),
        (" in ", Operator::In),
        (" == ", Operator::Eq),
        (" != ", Operator::Ne),
        (" < ", Operator::Lt),
        (" > ", Operator::Gt),
    ] {
        if let Some((left, right)) = expression.split_once(needle) {
            let left = left.trim();
            let right = right.trim();
            if left.starts_with('$') && !right.is_empty() {
                return Ok((left, operator, right));
            }
        }
    }

    Err(ConditionError::InvalidExpression {
        expression: expression.to_owned(),
    })
}

fn parse_literal(raw: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str(raw) {
        return Some(value);
    }

    if raw.starts_with('[') && raw.ends_with(']') {
        let jsonish = raw.replace('\'', "\"");
        if let Ok(value) = serde_json::from_str(&jsonish) {
            return Some(value);
        }
    }

    if let Some(value) = raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Some(Value::String(value.to_owned()));
    }

    None
}

fn compare(left: &Value, operator: Operator, right: &Value) -> Result<bool, ConditionError> {
    match operator {
        Operator::Eq => Ok(left == right),
        Operator::Ne => Ok(left != right),
        Operator::Lt => compare_numbers(left, right, |l, r| l < r),
        Operator::Gt => compare_numbers(left, right, |l, r| l > r),
        Operator::In => match right {
            Value::Array(values) => Ok(values.iter().any(|value| value == left)),
            Value::Object(values) => left
                .as_str()
                .map(|key| values.contains_key(key))
                .ok_or_else(|| ConditionError::UnsupportedComparison {
                    detail: "`in` with an object requires a string left operand".to_owned(),
                }),
            _ => Err(ConditionError::UnsupportedComparison {
                detail: "`in` requires an array or object right operand".to_owned(),
            }),
        },
        Operator::Contains => match (left, right) {
            (Value::Array(values), _) => Ok(values.iter().any(|value| value == right)),
            (Value::String(haystack), Value::String(needle)) => Ok(haystack.contains(needle)),
            _ => Err(ConditionError::UnsupportedComparison {
                detail: "`contains` requires an array left operand or two strings".to_owned(),
            }),
        },
    }
}

fn compare_numbers(
    left: &Value,
    right: &Value,
    predicate: impl FnOnce(f64, f64) -> bool,
) -> Result<bool, ConditionError> {
    let Some(left) = left.as_f64() else {
        return Err(ConditionError::UnsupportedComparison {
            detail: "numeric comparison requires numeric operands".to_owned(),
        });
    };
    let Some(right) = right.as_f64() else {
        return Err(ConditionError::UnsupportedComparison {
            detail: "numeric comparison requires numeric operands".to_owned(),
        });
    };
    Ok(predicate(left, right))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use serde_json::json;

    use super::*;

    fn context() -> ConditionContext {
        let mut context = ConditionContext::new();
        context.insert_output(
            "1",
            json!({
                "severity": "high",
                "approved": true,
                "score": 0.91,
                "tags": ["security", "api"],
                "summary": "security review"
            }),
        );
        context
    }

    #[test]
    fn evaluates_equality_on_dotted_output_path() {
        assert!(evaluate_condition("$1.output.severity == 'high'", &context()).unwrap());
        assert!(!evaluate_condition("$1.output.severity == 'low'", &context()).unwrap());
    }

    #[test]
    fn evaluates_boolean_and_numeric_comparisons() {
        assert!(evaluate_condition("$1.output.approved == true", &context()).unwrap());
        assert!(evaluate_condition("$1.output.score > 0.9", &context()).unwrap());
        assert!(!evaluate_condition("$1.output.score < 0.9", &context()).unwrap());
    }

    #[test]
    fn evaluates_membership_operators() {
        assert!(evaluate_condition("$1.output.tags contains 'security'", &context()).unwrap());
        assert!(
            evaluate_condition("$1.output.severity in ['high', 'medium']", &context()).unwrap()
        );
        assert!(evaluate_condition("$1.output.summary contains 'review'", &context()).unwrap());
    }
}
