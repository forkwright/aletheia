//! Full-text search query parsing.
//!
//! Parses FTS query strings into an [`FtsExpr`] AST supporting boolean
//! operators (AND, OR, NOT), proximity (NEAR), phrase matching, prefix
//! wildcards, and boost weights.
#![expect(
    clippy::as_conversions,
    clippy::pedantic,
    clippy::result_large_err,
    reason = "FTS query parser — numeric cast for boost weight, InternalError is the crate-wide Result type"
)]

use std::sync::LazyLock;

use compact_str::CompactString;
use itertools::Itertools;
use pest::Parser;
use pest::pratt_parser::{Op, PrattParser};

use crate::error::InternalResult as Result;
use crate::fts::ast::{FtsExpr, FtsLiteral, FtsNear};
use crate::parse::error::{InvalidQuerySnafu, SyntaxSnafu};
use crate::parse::expr::parse_string;
use crate::parse::{DatalogParser, Pair, Rule, input_location_to_span};

/// Parse a full-text search query string into an [`FtsExpr`] tree.
///
/// Multiple top-level terms are implicitly ANDed together.
///
/// # Errors
///
/// Returns an error if the query string contains invalid FTS syntax.
pub(crate) fn parse_fts_query(q: &str) -> Result<FtsExpr> {
    let mut pairs = DatalogParser::parse(Rule::fts_doc, q).map_err(|err| {
        let message = err.to_string();
        let span = input_location_to_span(err.location);
        SyntaxSnafu {
            span: span.to_string(),
            message,
        }
        .build()
    })?;
    let pairs = pairs
        .next()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "FTS parser produced no tokens".to_string(),
            }
            .build()
        })?
        .into_inner();
    let pairs: Vec<_> = pairs
        .filter(|r| r.as_rule() != Rule::EOI)
        .map(parse_fts_expr)
        .try_collect()?;
    Ok(if pairs.len() == 1 {
        pairs.into_iter().next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "FTS parser produced empty expression".to_string(),
            }
            .build()
        })?
    } else {
        FtsExpr::And(pairs)
    })
}

/// Parse a single FTS expression using Pratt precedence for AND/OR/NOT.
fn parse_fts_expr(pair: Pair<'_>) -> Result<FtsExpr> {
    debug_assert!(pair.as_rule() == Rule::fts_expr);
    let pairs = pair.into_inner();
    PRATT_PARSER
        .map_primary(build_term)
        .map_infix(build_infix)
        .parse(pairs)
}

/// Handle FTS binary operators (AND, OR, NOT).
fn build_infix(lhs: Result<FtsExpr>, op: Pair<'_>, rhs: Result<FtsExpr>) -> Result<FtsExpr> {
    let lhs = lhs?;
    let rhs = rhs?;
    Ok(match op.as_rule() {
        Rule::fts_and => FtsExpr::And(vec![lhs, rhs]),
        Rule::fts_or => FtsExpr::Or(vec![lhs, rhs]),
        Rule::fts_not => FtsExpr::Not(Box::new(lhs), Box::new(rhs)),
        r => {
            return Err(InvalidQuerySnafu {
                message: format!("unexpected rule {:?} in FTS parser - grammar mismatch", r),
            }
            .build()
            .into());
        }
    })
}

/// Build a primary FTS term: grouped expression, NEAR proximity, or phrase literal.
fn build_term(pair: Pair<'_>) -> Result<FtsExpr> {
    Ok(match pair.as_rule() {
        Rule::fts_grouped => {
            let collected: Vec<_> = pair.into_inner().map(parse_fts_expr).try_collect()?;
            if collected.len() == 1 {
                collected.into_iter().next().ok_or_else(|| {
                    InvalidQuerySnafu {
                        message: "FTS grouped expression is empty".to_string(),
                    }
                    .build()
                })?
            } else {
                FtsExpr::And(collected)
            }
        }
        Rule::fts_near => {
            let mut literals = vec![];
            let mut distance = 10;
            for pair in pair.into_inner() {
                match pair.as_rule() {
                    Rule::pos_int => {
                        let i = pair.as_str().replace('_', "").parse::<i64>().map_err(|e| {
                            InvalidQuerySnafu {
                                message: e.to_string(),
                            }
                            .build()
                        })?;
                        distance = u32::try_from(i).map_err(|_e| {
                            InvalidQuerySnafu {
                                message: format!("FTS near distance must fit in u32, got {i}"),
                            }
                            .build()
                        })?;
                    }
                    _ => literals.push(build_phrase(pair)?),
                }
            }
            FtsExpr::Near(FtsNear { literals, distance })
        }
        Rule::fts_phrase => FtsExpr::Literal(build_phrase(pair)?),
        r => {
            return SyntaxSnafu {
                span: String::new(),
                message: format!("unexpected FTS rule: {r:?}"),
            }
            .fail()
            .map_err(|e| e.into());
        }
    })
}

/// Build an FTS phrase literal with optional prefix marker and boost weight.
fn build_phrase(pair: Pair<'_>) -> Result<FtsLiteral> {
    let mut inner = pair.into_inner();
    let kernel = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "FTS phrase has no content".to_string(),
        }
        .build()
    })?;
    let core_text = match kernel.as_rule() {
        Rule::fts_phrase_group => CompactString::from(kernel.as_str().trim()),
        Rule::quoted_string | Rule::s_quoted_string | Rule::raw_string => parse_string(kernel)?,
        r => {
            return Err(InvalidQuerySnafu {
                message: format!("unexpected rule {:?} in FTS phrase - grammar mismatch", r),
            }
            .build()
            .into());
        }
    };
    let mut is_quoted = false;
    let mut booster = 1.0;
    for pair in inner {
        match pair.as_rule() {
            Rule::fts_prefix_marker => is_quoted = true,
            Rule::fts_booster => {
                let boosted = pair.into_inner().next().ok_or_else(|| {
                    InvalidQuerySnafu {
                        message: "FTS booster has no value".to_string(),
                    }
                    .build()
                })?;
                match boosted.as_rule() {
                    Rule::dot_float => {
                        let f = boosted
                            .as_str()
                            .replace('_', "")
                            .parse::<f64>()
                            .map_err(|e| {
                                InvalidQuerySnafu {
                                    message: e.to_string(),
                                }
                                .build()
                            })?;
                        booster = f;
                    }
                    Rule::int | Rule::pos_int => {
                        let i = boosted
                            .as_str()
                            .replace('_', "")
                            .parse::<i64>()
                            .map_err(|e| {
                                InvalidQuerySnafu {
                                    message: e.to_string(),
                                }
                                .build()
                            })?;
                        // INVARIANT: FTS booster is a small relevance multiplier;
                        // precision loss above 2^53 is irrelevant in practice.
                        #[expect(
                            clippy::cast_precision_loss,
                            reason = "FTS booster is a small relevance multiplier"
                        )]
                        let f = i as f64;
                        booster = f;
                    }
                    r => {
                        return Err(InvalidQuerySnafu {
                            message: format!(
                                "unexpected rule {:?} in FTS booster - grammar mismatch",
                                r
                            ),
                        }
                        .build()
                        .into());
                    }
                }
            }
            r => {
                return Err(InvalidQuerySnafu {
                    message: format!(
                        "unexpected rule {:?} in FTS phrase modifier - grammar mismatch",
                        r
                    ),
                }
                .build()
                .into());
            }
        }
    }
    Ok(FtsLiteral {
        value: core_text,
        is_prefix: is_quoted,
        booster: booster.into(),
    })
}

static PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| {
    use pest::pratt_parser::Assoc::*;

    PrattParser::new()
        .op(Op::infix(Rule::fts_not, Left))
        .op(Op::infix(Rule::fts_and, Left))
        .op(Op::infix(Rule::fts_or, Left))
});

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use crate::fts::ast::{FtsExpr, FtsNear};
    use crate::parse::fts::parse_fts_query;

    #[test]
    fn fts_parser_recognizes_or_and_not_and_near_operators() {
        let src = " hello world OR bye bye world";
        let res = parse_fts_query(src).unwrap().flatten();
        assert!(matches!(res, FtsExpr::Or(_)));
        let src = " hello world AND bye bye world";
        let res = parse_fts_query(src).unwrap().flatten();
        assert!(matches!(res, FtsExpr::And(_)));
        let src = " hello world NOT bye bye NOT 'ok, mates'";
        let res = parse_fts_query(src).unwrap().flatten();
        assert!(matches!(res, FtsExpr::Not(_, _)));
        let src = " NEAR(abc def \"ghi\"^22.8) ";
        let res = parse_fts_query(src).unwrap().flatten();
        assert!(
            matches!(&res, FtsExpr::Near(FtsNear { distance: 10, .. })),
            "NEAR query should parse to a Near node with default distance 10"
        );
        // WHY(#4515): fts_phrase_group = {fts_phrase_simple+} — adjacent unquoted words
        // inside NEAR() merge into one phrase_group.  "abc def" is ONE literal; "ghi"^22.8
        // is a second literal with its booster.  Three separate literals only arise when
        // each word is individually quoted or separated by explicit NEAR nesting.
        if let FtsExpr::Near(FtsNear { literals, distance }) = res {
            assert_eq!(distance, 10);
            assert_eq!(literals.len(), 2);
            let lit0 = literals.first().unwrap();
            let lit1 = literals.get(1).unwrap();
            assert_eq!(lit0.value, "abc def");
            assert!(!lit0.is_prefix);
            assert_eq!(lit0.booster.0, 1.0);
            assert_eq!(lit1.value, "ghi");
            assert_eq!(lit1.booster.0, 22.8);
            assert!(!lit1.is_prefix);
        } else {
            unreachable!("INVARIANT: prior match asserted NEAR shape");
        }
    }

    mod proptests {
        use proptest::prelude::*;

        use crate::fts::ast::{FtsExpr, FtsLiteral};
        use crate::parse::fts::parse_fts_query;

        /// Check whether an FTS word collides with a keyword prefix.
        fn conflicts_with_kw(w: &str) -> bool {
            let u = w.to_uppercase();
            matches!(u.as_str(), "AND" | "OR" | "NOT" | "NEAR")
                || u.starts_with("AND")
                || u.starts_with("OR")
                || u.starts_with("NOT")
                || u.starts_with("NEAR")
        }

        proptest! {
            /// The FTS parser must never panic on arbitrary input: it should return a
            /// parse error instead. Panics indicate logic bugs in the parser itself.
            #[test]
            fn fts_parser_never_panics(input in "\\PC{0,500}") {
                let _ = parse_fts_query(&input);
            }

            /// Valid single-word queries must parse to a single literal with the
            /// original text preserved.
            #[test]
            fn fts_single_word_parses(word in "[a-zA-Z]{1,30}") {
                prop_assume!(!conflicts_with_kw(&word));
                let parsed = parse_fts_query(&word)
                    .unwrap_or_else(|_| unreachable!("INVARIANT: non-keyword alphanumeric word always parses"));
                prop_assert!(
                    matches!(
                        &parsed,
                        FtsExpr::Literal(FtsLiteral { value, is_prefix: false, booster })
                        if value.as_str() == word && booster.0 == 1.0
                    ),
                    "single word should parse to a single literal: got {:?}",
                    parsed
                );
            }

            /// AND/OR combinations of alphanumeric words must parse to the expected
            /// binary operator with two literal children.
            #[test]
            fn fts_and_or_parses(
                lhs in "[a-zA-Z]{1,20}",
                op in prop_oneof![Just("AND"), Just("OR")],
                rhs in "[a-zA-Z]{1,20}",
            ) {
                prop_assume!(!conflicts_with_kw(&lhs) && !conflicts_with_kw(&rhs));
                let query = format!("{lhs} {op} {rhs}");
                let parsed = parse_fts_query(&query)
                    .unwrap_or_else(|_| unreachable!("INVARIANT: non-keyword AND/OR query always parses"));
                let children = match &parsed {
                    FtsExpr::And(v) if op == "AND" => v,
                    FtsExpr::Or(v) if op == "OR" => v,
                    other => panic!("expected {op} expression, got {:?}", other),
                };
                prop_assert_eq!(children.len(), 2, "{} should combine exactly two children", op);
                let child0 = children.first().unwrap();
                let child1 = children.get(1).unwrap();
                prop_assert!(
                    matches!(
                        child0,
                        FtsExpr::Literal(FtsLiteral { value, is_prefix: false, booster })
                        if value.as_str() == lhs && booster.0 == 1.0
                    ),
                    "left operand should be the literal {:?}: got {:?}",
                    lhs,
                    child0
                );
                prop_assert!(
                    matches!(
                        child1,
                        FtsExpr::Literal(FtsLiteral { value, is_prefix: false, booster })
                        if value.as_str() == rhs && booster.0 == 1.0
                    ),
                    "right operand should be the literal {:?}: got {:?}",
                    rhs,
                    child1
                );
            }
        }
    }
}
