//! Schema definition parsing.
use std::collections::BTreeSet;

use crate::bail;
use crate::engine::error::DbResult as Result;
use crate::engine::parse::error::InvalidQuerySnafu;
use compact_str::CompactString;
use itertools::Itertools;

use crate::engine::data::relation::{
    ColType, ColumnDef, NullableColType, StoredRelationMetadata, VecElementType,
};
use crate::engine::data::symb::Symbol;
use crate::engine::parse::expr::build_expr;
use crate::engine::parse::{ExtractSpan, Pair, Rule};

pub(crate) fn parse_schema(
    pair: Pair<'_>,
) -> Result<(StoredRelationMetadata, Vec<Symbol>, Vec<Symbol>)> {
    let mut src = pair.into_inner();
    let mut keys = vec![];
    let mut dependents = vec![];
    let mut key_bindings = vec![];
    let mut dep_bindings = vec![];
    let mut seen_names = BTreeSet::new();

    for p in src.next().expect("pest guarantees schema keys section").into_inner() {
        let _span = p.extract_span();
        let (col, ident) = parse_col(p)?;
        if !seen_names.insert(col.name.clone()) {
            bail!(InvalidQuerySnafu {
                message: "Column is defined multiple times".to_string()
            }
            .build());
        }
        keys.push(col);
        key_bindings.push(ident)
    }
    if let Some(ps) = src.next() {
        for p in ps.into_inner() {
            let _span = p.extract_span();
            let (col, ident) = parse_col(p)?;
            if !seen_names.insert(col.name.clone()) {
                bail!(InvalidQuerySnafu {
                    message: "Column is defined multiple times".to_string()
                }
                .build());
            }
            dependents.push(col);
            dep_bindings.push(ident)
        }
    }

    Ok((
        StoredRelationMetadata {
            keys,
            non_keys: dependents,
        },
        key_bindings,
        dep_bindings,
    ))
}

fn parse_col(pair: Pair<'_>) -> Result<(ColumnDef, Symbol)> {
    let mut src = pair.into_inner();
    let name_p = src.next().expect("pest guarantees column name");
    let name = CompactString::from(name_p.as_str());
    let mut typing = NullableColType {
        coltype: ColType::Any,
        nullable: true,
    };
    let mut default_gen = None;
    let mut binding_candidate = None;
    for nxt in src {
        match nxt.as_rule() {
            Rule::col_type => typing = parse_nullable_type(nxt)?,
            Rule::expr => default_gen = Some(build_expr(nxt, &Default::default())?),
            Rule::out_arg => {
                binding_candidate = Some(Symbol::new(nxt.as_str(), nxt.extract_span()))
            }
            r => unreachable!("{:?}", r),
        }
    }
    let binding =
        binding_candidate.unwrap_or_else(|| Symbol::new(&name as &str, name_p.extract_span()));
    Ok((
        ColumnDef {
            name,
            typing,
            default_gen,
        },
        binding,
    ))
}

pub(crate) fn parse_nullable_type(pair: Pair<'_>) -> Result<NullableColType> {
    let nullable = pair.as_str().ends_with('?');
    let coltype = parse_type_inner(pair.into_inner().next().expect("pest guarantees col type inner"))?;
    Ok(NullableColType { coltype, nullable })
}

fn parse_type_inner(pair: Pair<'_>) -> Result<ColType> {
    Ok(match pair.as_rule() {
        Rule::any_type => ColType::Any,
        Rule::bool_type => ColType::Bool,
        Rule::int_type => ColType::Int,
        Rule::float_type => ColType::Float,
        Rule::string_type => ColType::String,
        Rule::bytes_type => ColType::Bytes,
        Rule::uuid_type => ColType::Uuid,
        Rule::json_type => ColType::Json,
        Rule::validity_type => ColType::Validity,
        Rule::list_type => {
            let mut inner = pair.into_inner();
            let eltype = parse_nullable_type(inner.next().expect("pest guarantees list element type"))?;
            let len = match inner.next() {
                None => None,
                Some(len_p) => {
                    let _span = len_p.extract_span();
                    let expr = build_expr(len_p, &Default::default())?;
                    let dv = expr.eval_to_const()?;

                    let n = dv.get_int().ok_or(crate::engine::error::AdhocError(
                        "Bad specification of list length in type".to_string(),
                    ))?;
                    if n < 0 {
                        bail!(InvalidQuerySnafu {
                            message: "Bad specification of list length in type: negative length"
                                .to_string()
                        }
                        .build());
                    }
                    Some(n as usize)
                }
            };
            ColType::List {
                eltype: eltype.into(),
                len,
            }
        }
        Rule::vec_type => {
            let mut inner = pair.into_inner();
            let eltype = match inner.next().expect("pest guarantees vec element type").as_str() {
                "F32" | "Float" => VecElementType::F32,
                "F64" | "Double" => VecElementType::F64,
                _ => unreachable!(),
            };
            let len = inner.next().expect("pest guarantees vec length");
            let len = len
                .as_str()
                .replace('_', "")
                .parse::<usize>()
                .map_err(|e| crate::engine::error::AdhocError(e.to_string()))?;
            ColType::Vec { eltype, len }
        }
        Rule::tuple_type => {
            ColType::Tuple(pair.into_inner().map(parse_nullable_type).try_collect()?)
        }
        _ => unreachable!(),
    })
}
