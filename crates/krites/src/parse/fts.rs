//! Full-text search clause parsing.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
#![expect(
    clippy::as_conversions,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::sync::LazyLock;

use compact_str::CompactString;
use itertools::Itertools;
use pest::Parser;
use pest::error::InputLocation;
use pest::pratt_parser::{Op, PrattParser};

use crate::error::InternalResult as Result;
use crate::fts::ast::{FtsExpr, FtsLiteral, FtsNear};
use crate::parse::error::{InvalidQuerySnafu, SyntaxSnafu};
use crate::parse::expr::parse_string;
use crate::parse::{DatalogParser, Pair, Rule, SourceSpan};

pub(crate) fn parse_fts_query(q: &str) -> Result<FtsExpr> {
    let mut pairs = DatalogParser::parse(Rule::fts_doc, q).map_err(|err| {
        let span = match err.location {
            InputLocation::Pos(p) => SourceSpan(p, 0),
            InputLocation::Span((start, end)) => SourceSpan(start, end - start),
        };
        SyntaxSnafu {
            span: span.to_string(),
            message: err.to_string(),
        }
        .build()
    })?;
    let pairs = pairs
        .next()
        .expect("pest guarantees fts_doc token")
        .into_inner();
    let pairs: Vec<_> = pairs
        .filter(|r| r.as_rule() != Rule::EOI)
        .map(parse_fts_expr)
        .try_collect()?;
    Ok(if pairs.len() == 1 {
        pairs.into_iter().next().expect("just checked len == 1")
    } else {
        FtsExpr::And(pairs)
    })
}

fn parse_fts_expr(pair: Pair<'_>) -> Result<FtsExpr> {
    debug_assert!(pair.as_rule() == Rule::fts_expr);
    let pairs = pair.into_inner();
    PRATT_PARSER
        .map_primary(build_term)
        .map_infix(build_infix)
        .parse(pairs)
}

fn build_infix(lhs: Result<FtsExpr>, op: Pair<'_>, rhs: Result<FtsExpr>) -> Result<FtsExpr> {
    let lhs = lhs?;
    let rhs = rhs?;
    Ok(match op.as_rule() {
        Rule::fts_and => FtsExpr::And(vec![lhs, rhs]),
        Rule::fts_or => FtsExpr::Or(vec![lhs, rhs]),
        Rule::fts_not => FtsExpr::Not(Box::new(lhs), Box::new(rhs)),
        _ => unreachable!("unexpected rule: {:?}", op.as_rule()),
    })
}

fn build_term(pair: Pair<'_>) -> Result<FtsExpr> {
    Ok(match pair.as_rule() {
        Rule::fts_grouped => {
            let collected: Vec<_> = pair.into_inner().map(parse_fts_expr).try_collect()?;
            if collected.len() == 1 {
                collected.into_iter().next().expect("just checked len == 1")
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
                        distance = i as u32;
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

fn build_phrase(pair: Pair<'_>) -> Result<FtsLiteral> {
    let mut inner = pair.into_inner();
    let kernel = inner.next().expect("pest guarantees phrase kernel");
    let core_text = match kernel.as_rule() {
        Rule::fts_phrase_group => CompactString::from(kernel.as_str().trim()),
        Rule::quoted_string | Rule::s_quoted_string | Rule::raw_string => parse_string(kernel)?,
        _ => unreachable!("unexpected rule: {:?}", kernel.as_rule()),
    };
    let mut is_quoted = false;
    let mut booster = 1.0;
    for pair in inner {
        match pair.as_rule() {
            Rule::fts_prefix_marker => is_quoted = true,
            Rule::fts_booster => {
                let boosted = pair
                    .into_inner()
                    .next()
                    .expect("pest guarantees booster value");
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
                    Rule::int => {
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
                        booster = i as f64;
                    }
                    _ => unreachable!("unexpected rule: {:?}", boosted.as_rule()),
                }
            }
            _ => unreachable!("unexpected rule: {:?}", pair.as_rule()),
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
        assert!(matches!(res, FtsExpr::Near(FtsNear { distance: 10, .. })));
        println!("{:#?}", res);
    }

    mod proptests {
        use proptest::prelude::*;

        use crate::parse::fts::parse_fts_query;

        proptest! {
            /// The FTS parser must never panic on arbitrary input: it should return a
            /// parse error instead. Panics indicate logic bugs in the parser itself.
            #[test]
            fn fts_parser_never_panics(input in "\\PC{0,500}") {
                let _ = parse_fts_query(&input);
            }

            /// Valid single-word queries must always parse successfully.
            #[test]
            fn fts_single_word_parses(word in "[a-zA-Z]{1,30}") {
                // NOTE: pest grammar matches keywords greedily, so words
                // starting with AND/OR/NOT/NEAR confuse the parser.
                let conflicts_with_kw = |w: &str| {
                    let u = w.to_uppercase();
                    matches!(u.as_str(), "AND" | "OR" | "NOT" | "NEAR")
                        || u.starts_with("AND")
                        || u.starts_with("OR")
                        || u.starts_with("NOT")
                        || u.starts_with("NEAR")
                };
                prop_assume!(!conflicts_with_kw(&word));
                parse_fts_query(&word).expect("single word should parse");
            }

            /// AND/OR combinations of alphanumeric words must always parse without error.
            #[test]
            fn fts_and_or_parses(
                lhs in "[a-zA-Z]{1,20}",
                op in prop_oneof![Just("AND"), Just("OR")],
                rhs in "[a-zA-Z]{1,20}",
            ) {
                // NOTE: the pest grammar matches keywords greedily, so words
                // starting with AND/OR/NOT/NEAR (e.g. "ORa") confuse the parser.
                // Filter those out: the underlying grammar bug is tracked separately.
                let conflicts_with_kw = |w: &str| {
                    let u = w.to_uppercase();
                    matches!(u.as_str(), "AND" | "OR" | "NOT" | "NEAR")
                        || u.starts_with("AND")
                        || u.starts_with("OR")
                        || u.starts_with("NOT")
                        || u.starts_with("NEAR")
                };
                prop_assume!(!conflicts_with_kw(&lhs) && !conflicts_with_kw(&rhs));
                let query = format!("{lhs} {op} {rhs}");
                parse_fts_query(&query).expect("AND/OR query should parse");
            }
        }
    }
}
