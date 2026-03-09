// Originally derived from CozoDB v0.7.6 (MPL-2.0).
// Copyright 2023, The Cozo Project Authors — see NOTICE for details.

use crate::engine::error::DbResult as Result;
use crate::engine::fts::ast::{FtsExpr, FtsLiteral, FtsNear};
use crate::engine::parse::expr::parse_string;
use crate::engine::parse::{DatalogParser, Pair, Rule};
use compact_str::CompactString;
use itertools::Itertools;
use pest::Parser;
use pest::pratt_parser::{Op, PrattParser};
use std::sync::LazyLock;

pub(crate) fn parse_fts_query(q: &str) -> Result<FtsExpr> {
    let mut pairs = DatalogParser::parse(Rule::fts_doc, q)
        .map_err(|e| crate::engine::error::AdhocError(e.to_string()))?;
    let pairs = pairs.next().unwrap().into_inner();
    let pairs: Vec<_> = pairs
        .filter(|r| r.as_rule() != Rule::EOI)
        .map(parse_fts_expr)
        .try_collect()?;
    Ok(if pairs.len() == 1 {
        pairs.into_iter().next().unwrap()
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
                collected.into_iter().next().unwrap()
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
                        let i = pair
                            .as_str()
                            .replace('_', "")
                            .parse::<i64>()
                            .map_err(|e| crate::engine::error::AdhocError(e.to_string()))?;
                        distance = i as u32;
                    }
                    _ => literals.push(build_phrase(pair)?),
                }
            }
            FtsExpr::Near(FtsNear { literals, distance })
        }
        Rule::fts_phrase => FtsExpr::Literal(build_phrase(pair)?),
        r => panic!("unexpected rule: {:?}", r),
    })
}

fn build_phrase(pair: Pair<'_>) -> Result<FtsLiteral> {
    let mut inner = pair.into_inner();
    let kernel = inner.next().unwrap();
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
                let boosted = pair.into_inner().next().unwrap();
                match boosted.as_rule() {
                    Rule::dot_float => {
                        let f = boosted
                            .as_str()
                            .replace('_', "")
                            .parse::<f64>()
                            .map_err(|e| crate::engine::error::AdhocError(e.to_string()))?;
                        booster = f;
                    }
                    Rule::int => {
                        let i = boosted
                            .as_str()
                            .replace('_', "")
                            .parse::<i64>()
                            .map_err(|e| crate::engine::error::AdhocError(e.to_string()))?;
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
mod tests {
    use crate::engine::fts::ast::{FtsExpr, FtsNear};
    use crate::engine::parse::fts::parse_fts_query;

    #[test]
    fn test_parse() {
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
}
