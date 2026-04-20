// Simple recursive-descent expression evaluator.
//
// Supports: f64 literals, +, -, *, /, parentheses. No variables.
// WHY: a dependency like `meval` would pull in transitive deps for a ~50-line
// problem. Rolling our own keeps the dep count flat and the logic auditable.

use crate::error::VerifyError;

/// Evaluate an arithmetic formula string and return the f64 result.
///
/// # Errors
///
/// Returns `VerifyError::Eval` if the formula contains unknown characters,
/// unmatched parentheses, or a division-by-zero.
pub(crate) fn eval(formula: &str) -> Result<f64, VerifyError> {
    let tokens = tokenize(formula)?;
    let mut pos = 0usize;
    if tokens.is_empty() {
        return Err(VerifyError::Eval {
            formula: formula.to_owned(),
            detail: "empty formula".to_owned(),
        });
    }
    let result = parse_expr(&tokens, &mut pos, formula)?;
    if pos != tokens.len() {
        return Err(VerifyError::Eval {
            formula: formula.to_owned(),
            detail: format!("unexpected token at position {pos}"),
        });
    }
    Ok(result)
}

// ── Token type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

// ── Tokenizer ────────────────────────────────────────────────────────────────

fn tokenize(input: &str) -> Result<Vec<Token>, VerifyError> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {}
            '+' => tokens.push(Token::Plus),
            '-' => {
                // WHY: a '-' that appears at the start or after an operator is
                // a unary negation, not a binary minus. We handle it by
                // consuming the following number literal directly here.
                let is_unary = matches!(
                    tokens.last(),
                    None | Some(
                        Token::Plus | Token::Minus | Token::Star | Token::Slash | Token::LParen
                    )
                );
                if is_unary {
                    let (num, _) = consume_number(input, idx + ch.len_utf8(), &mut chars)?;
                    tokens.push(Token::Num(-num));
                } else {
                    tokens.push(Token::Minus);
                }
            }
            '*' => tokens.push(Token::Star),
            '/' => tokens.push(Token::Slash),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            '0'..='9' | '.' => {
                let (num, _) = consume_number(input, idx, &mut chars)?;
                tokens.push(Token::Num(num));
            }
            _ => {
                return Err(VerifyError::Eval {
                    formula: input.to_owned(),
                    detail: format!("unexpected character '{ch}' at byte {idx}"),
                });
            }
        }
    }

    Ok(tokens)
}

/// Consume a decimal number starting at `start` in `input`.
///
/// `chars` must be positioned just AFTER `start` (i.e. the first character of
/// the number has already been consumed by the caller). Returns `(value,
/// bytes_consumed)`.
fn consume_number(
    input: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<(f64, usize), VerifyError> {
    // Find the end of the number by peeking ahead.
    let end = loop {
        match chars.peek() {
            Some((_, c)) if c.is_ascii_digit() || *c == '.' || *c == 'e' || *c == 'E' => {
                chars.next();
            }
            Some((idx, _)) => break *idx,
            None => break input.len(),
        }
    };

    let slice = input.get(start..end).ok_or_else(|| VerifyError::Eval {
        formula: input.to_owned(),
        detail: format!("byte slice {start}..{end} out of bounds"),
    })?;

    slice
        .parse::<f64>()
        .map(|v| (v, end - start))
        .map_err(|e| VerifyError::Eval {
            formula: input.to_owned(),
            detail: format!("cannot parse '{slice}' as a number: {e}"),
        })
}

// ── Recursive-descent parser (precedence climbing) ───────────────────────────
//
// Grammar:
//   expr   = term (('+' | '-') term)*
//   term   = factor (('*' | '/') factor)*
//   factor = NUMBER | '(' expr ')'

fn parse_expr(tokens: &[Token], pos: &mut usize, formula: &str) -> Result<f64, VerifyError> {
    let mut lhs = parse_term(tokens, pos, formula)?;

    while let Some(tok) = tokens.get(*pos) {
        match tok {
            Token::Plus => {
                *pos += 1;
                lhs += parse_term(tokens, pos, formula)?;
            }
            Token::Minus => {
                *pos += 1;
                lhs -= parse_term(tokens, pos, formula)?;
            }
            _ => break,
        }
    }

    Ok(lhs)
}

fn parse_term(tokens: &[Token], pos: &mut usize, formula: &str) -> Result<f64, VerifyError> {
    let mut lhs = parse_factor(tokens, pos, formula)?;

    while let Some(tok) = tokens.get(*pos) {
        match tok {
            Token::Star => {
                *pos += 1;
                lhs *= parse_factor(tokens, pos, formula)?;
            }
            Token::Slash => {
                *pos += 1;
                let rhs = parse_factor(tokens, pos, formula)?;
                if rhs.abs() < f64::EPSILON {
                    return Err(VerifyError::Eval {
                        formula: formula.to_owned(),
                        detail: "division by zero".to_owned(),
                    });
                }
                lhs /= rhs;
            }
            _ => break,
        }
    }

    Ok(lhs)
}

fn parse_factor(tokens: &[Token], pos: &mut usize, formula: &str) -> Result<f64, VerifyError> {
    match tokens.get(*pos) {
        Some(Token::Num(v)) => {
            let v = *v;
            *pos += 1;
            Ok(v)
        }
        Some(Token::LParen) => {
            *pos += 1;
            let inner = parse_expr(tokens, pos, formula)?;
            match tokens.get(*pos) {
                Some(Token::RParen) => {
                    *pos += 1;
                    Ok(inner)
                }
                _ => Err(VerifyError::Eval {
                    formula: formula.to_owned(),
                    detail: "expected closing ')'".to_owned(),
                }),
            }
        }
        Some(other) => Err(VerifyError::Eval {
            formula: formula.to_owned(),
            detail: format!("unexpected token {other:?}"),
        }),
        None => Err(VerifyError::Eval {
            formula: formula.to_owned(),
            detail: "unexpected end of expression".to_owned(),
        }),
    }
}

// ── Arithmetic formula check ─────────────────────────────────────────────────

/// Evaluate `formula` and compare to `expected` within `tolerance`.
///
/// # Errors
///
/// Returns `VerifyError::Eval` if the formula cannot be parsed or evaluated.
pub(crate) fn check(
    formula: &str,
    expected: f64,
    tolerance: f64,
) -> Result<ArithmeticResult, VerifyError> {
    let actual = eval(formula)?;
    let diff = (actual - expected).abs();
    Ok(ArithmeticResult {
        actual,
        diff,
        pass: diff <= tolerance,
    })
}

/// Result of an arithmetic check.
#[derive(Debug, Clone)]
pub(crate) struct ArithmeticResult {
    pub(crate) actual: f64,
    pub(crate) diff: f64,
    pub(crate) pass: bool,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::unreadable_literal,
    reason = "test fixture values match upstream report data verbatim"
)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn eval_integer_literal() {
        let v = eval("42").expect("must eval");
        assert!(approx(v, 42.0), "literal 42 must evaluate to 42.0");
    }

    #[test]
    fn eval_float_literal() {
        // WHY: 2.5 avoids the clippy::approx_constant lint triggered by values near π.
        let v = eval("2.5").expect("must eval");
        assert!(approx(v, 2.5), "literal 2.5 must evaluate to 2.5");
    }

    #[test]
    fn eval_addition() {
        let v = eval("1 + 2").expect("must eval");
        assert!(approx(v, 3.0), "1 + 2 must be 3");
    }

    #[test]
    fn eval_subtraction() {
        let v = eval("10 - 4").expect("must eval");
        assert!(approx(v, 6.0), "10 - 4 must be 6");
    }

    #[test]
    fn eval_multiplication() {
        let v = eval("3 * 7").expect("must eval");
        assert!(approx(v, 21.0), "3 * 7 must be 21");
    }

    #[test]
    fn eval_division() {
        let v = eval("10 / 4").expect("must eval");
        assert!(approx(v, 2.5), "10 / 4 must be 2.5");
    }

    #[test]
    fn eval_operator_precedence() {
        let v = eval("2 + 3 * 4").expect("must eval");
        assert!(approx(v, 14.0), "2 + 3 * 4 must respect precedence -> 14");
    }

    #[test]
    fn eval_parentheses_override_precedence() {
        let v = eval("(2 + 3) * 4").expect("must eval");
        assert!(
            approx(v, 20.0),
            "(2 + 3) * 4 must evaluate inner first -> 20"
        );
    }

    #[test]
    fn eval_negative_unary() {
        let v = eval("-5 + 3").expect("must eval");
        assert!(approx(v, -2.0), "-5 + 3 must be -2");
    }

    #[test]
    fn eval_savings_sum() {
        let v = eval("78187 + 26558 + 1620 + 1165 + 127 + 127 + 0").expect("must eval");
        assert!(approx(v, 107784.0), "savings components must sum to 107784");
    }

    #[test]
    fn eval_percentage_formula() {
        let v = eval("106365 / 107784 * 100").expect("must eval");
        assert!(
            (v - 98.683_f64).abs() < 0.01,
            "percentage formula must evaluate near 98.68"
        );
    }

    #[test]
    fn eval_nested_parentheses() {
        let v = eval("((2 + 3) * (4 - 1)) / 5").expect("must eval");
        assert!(approx(v, 3.0), "nested parens must evaluate correctly");
    }

    #[test]
    fn eval_division_by_zero_fails() {
        let result = eval("10 / 0");
        assert!(result.is_err(), "division by zero must return an error");
    }

    #[test]
    fn eval_unknown_character_fails() {
        let result = eval("1 + x");
        assert!(result.is_err(), "unknown character must return an error");
    }

    #[test]
    fn eval_unmatched_paren_fails() {
        let result = eval("(1 + 2");
        assert!(result.is_err(), "unmatched paren must return an error");
    }

    #[test]
    fn eval_empty_string_fails() {
        let result = eval("");
        assert!(result.is_err(), "empty formula must return an error");
    }

    #[test]
    fn check_pass_within_tolerance() {
        let r = check("78187 + 26558 + 1620 + 1165 + 127 + 127 + 0", 107784.0, 1.0)
            .expect("check must succeed");
        assert!(r.pass, "must PASS within tolerance");
        assert!(r.diff.abs() < 1e-9, "diff must be zero for exact match");
    }

    #[test]
    fn check_fail_outside_tolerance() {
        let r = check("100", 102.0, 1.0).expect("check must succeed");
        assert!(!r.pass, "must FAIL when diff exceeds tolerance");
        assert!(approx(r.diff, 2.0), "diff must be 2.0");
    }

    #[test]
    fn check_exact_tolerance_boundary_passes() {
        let r = check("100", 101.0, 1.0).expect("check must succeed");
        assert!(r.pass, "diff == tolerance must still PASS");
    }
}
