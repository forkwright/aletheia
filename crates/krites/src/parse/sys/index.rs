//! Index creation parsing helpers for FTS, HNSW, and MinHash LSH indexes.
//!
//! Extracts the common tokenizer/filter/option parsing that was duplicated
//! across the three index types in the system command parser.
#![expect(
    clippy::pedantic,
    clippy::result_large_err,
    reason = "System command parser — InternalError is the crate-wide Result type, pedantic style deferred"
)]

use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::Expr;
use crate::data::relation::VecElementType;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fts::TokenizerConfig;
use crate::parse::error::InvalidQuerySnafu;
use crate::parse::expr::build_expr;
use crate::parse::{ExtractSpan, Pair};

use super::{FtsIndexConfig, HnswDistance, HnswIndexConfig, MinHashLshConfig, SysOp};

/// Parse an `index_drop` pair into a `SysOp::RemoveIndex`.
///
/// All index types (standard, FTS, HNSW, LSH) share the same drop syntax:
/// `::index drop <relation> <name>`.
pub(super) fn parse_index_drop(inner: Pair<'_>) -> Result<SysOp> {
    let mut inner = inner.into_inner();
    let rel = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected relation name in index drop".to_string(),
        }
        .build()
    })?;
    let name = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected index name in index drop".to_string(),
        }
        .build()
    })?;
    Ok(SysOp::RemoveIndex(
        Symbol::new(rel.as_str(), rel.extract_span()),
        Symbol::new(name.as_str(), name.extract_span()),
    ))
}

/// Parsed tokenizer and filter configuration, shared by FTS and LSH indexes.
pub(super) struct TextIndexConfig {
    pub(super) extractor: String,
    pub(super) tokenizer: TokenizerConfig,
    pub(super) filters: Vec<TokenizerConfig>,
}

/// Parse the common text-index options (extractor, extract_filter, tokenizer, filters)
/// from an advanced index creation node.
///
/// Returns the parsed config along with any remaining option pairs that the
/// caller (FTS or LSH) should process themselves.
pub(super) fn parse_text_index_options<'a>(
    options: impl Iterator<Item = Pair<'a>>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<(TextIndexConfig, Vec<(String, Pair<'a>)>)> {
    let mut extractor = String::new();
    let mut extract_filter = String::new();
    let mut tokenizer = TokenizerConfig {
        name: Default::default(),
        args: Default::default(),
    };
    let mut filters = vec![];
    let mut extra_options = vec![];

    for opt_pair in options {
        let mut opt_inner = opt_pair.into_inner();
        let opt_name = opt_inner.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected option name".to_string(),
            }
            .build()
        })?;
        let opt_val = opt_inner.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected option value".to_string(),
            }
            .build()
        })?;

        match opt_name.as_str() {
            "extractor" => {
                let mut ex = build_expr(opt_val, param_pool)?;
                ex.partial_eval()?;
                extractor = ex.to_string();
            }
            "extract_filter" => {
                let mut ex = build_expr(opt_val, param_pool)?;
                ex.partial_eval()?;
                extract_filter = ex.to_string();
            }
            "tokenizer" => {
                tokenizer = parse_tokenizer_value(opt_val, param_pool)?;
            }
            "filters" => {
                filters = parse_filter_list(opt_val, param_pool)?;
            }
            _ => {
                extra_options.push((opt_name.as_str().to_string(), opt_val));
            }
        }
    }

    if !extract_filter.is_empty() {
        extractor = format!("if({extract_filter}, {extractor})");
    }

    Ok((
        TextIndexConfig {
            extractor,
            tokenizer,
            filters,
        },
        extra_options,
    ))
}

/// Parse a tokenizer value (either a bare symbol or a function call).
fn parse_tokenizer_value(
    opt_val: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<TokenizerConfig> {
    let mut expr = build_expr(opt_val, param_pool)?;
    expr.partial_eval()?;
    match expr {
        Expr::UnboundApply { op, args, .. } => {
            let mut targs = Vec::with_capacity(args.len());
            for arg in args.iter() {
                targs.push(arg.clone().eval_to_const()?);
            }
            Ok(TokenizerConfig {
                name: op,
                args: targs,
            })
        }
        Expr::Binding { var, .. } => Ok(TokenizerConfig {
            name: var.name,
            args: vec![],
        }),
        _ => Err(InvalidQuerySnafu {
            message: "Tokenizer must be a symbol or a call for an existing tokenizer".to_string(),
        }
        .build()
        .into()),
    }
}

/// Parse a filter list (`[filter1, filter2(...), ...]`).
fn parse_filter_list(
    opt_val: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<Vec<TokenizerConfig>> {
    let mut expr = build_expr(opt_val, param_pool)?;
    expr.partial_eval()?;
    match expr {
        Expr::Apply { op, args, .. } => {
            if op.name != "OP_LIST" {
                return Err(InvalidQuerySnafu {
                    message: "Filters must be a list of filters".to_string(),
                }
                .build()
                .into());
            }
            let mut filters = Vec::with_capacity(args.len());
            for arg in args.iter() {
                match arg {
                    Expr::UnboundApply { op, args, .. } => {
                        let mut targs = Vec::with_capacity(args.len());
                        for arg in args.iter() {
                            targs.push(arg.clone().eval_to_const()?);
                        }
                        filters.push(TokenizerConfig {
                            name: op.clone(),
                            args: targs,
                        });
                    }
                    Expr::Binding { var, .. } => {
                        filters.push(TokenizerConfig {
                            name: var.name.clone(),
                            args: vec![],
                        });
                    }
                    _ => {
                        return Err(InvalidQuerySnafu {
                            message:
                                "Tokenizer must be a symbol or a call for an existing tokenizer"
                                    .to_string(),
                        }
                        .build()
                        .into());
                    }
                }
            }
            Ok(filters)
        }
        _ => Err(InvalidQuerySnafu {
            message: "Filters must be a list of filters".to_string(),
        }
        .build()
        .into()),
    }
}

/// Parse an FTS index creation from an `index_create_adv` pair.
pub(super) fn parse_fts_index_create(
    inner: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<SysOp> {
    let mut inner = inner.into_inner();
    let rel = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected relation name".to_string(),
        }
        .build()
    })?;
    let name = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected index name".to_string(),
        }
        .build()
    })?;

    let (text_config, extra) = parse_text_index_options(inner, param_pool)?;
    if let Some((opt_name, _)) = extra.first() {
        return Err(InvalidQuerySnafu {
            message: format!("Unknown option {opt_name} for FTS index"),
        }
        .build()
        .into());
    }

    Ok(SysOp::CreateFtsIndex(FtsIndexConfig {
        base_relation: CompactString::from(rel.as_str()),
        index_name: CompactString::from(name.as_str()),
        extractor: text_config.extractor,
        tokenizer: text_config.tokenizer,
        filters: text_config.filters,
    }))
}

/// Parse a MinHash LSH index creation from an `index_create_adv` pair.
pub(super) fn parse_lsh_index_create(
    inner: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<SysOp> {
    let mut inner_pairs = inner.into_inner();
    let rel = inner_pairs.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected relation name".to_string(),
        }
        .build()
    })?;
    let name = inner_pairs.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected index name".to_string(),
        }
        .build()
    })?;

    let (text_config, extra) = parse_text_index_options(inner_pairs, param_pool)?;

    let mut n_gram: usize = 1;
    let mut n_perm: usize = 200;
    let mut target_threshold: f64 = 0.9;
    let mut false_positive_weight: f64 = 1.0;
    let mut false_negative_weight: f64 = 1.0;

    for (opt_name, opt_val) in extra {
        match opt_name.as_str() {
            "false_positive_weight" => {
                let mut expr = build_expr(opt_val, param_pool)?;
                expr.partial_eval()?;
                false_positive_weight = expr.eval_to_const()?.get_float().ok_or_else(|| {
                    InvalidQuerySnafu {
                        message: "false_positive_weight must be a float".to_string(),
                    }
                    .build()
                })?;
            }
            "false_negative_weight" => {
                let mut expr = build_expr(opt_val, param_pool)?;
                expr.partial_eval()?;
                false_negative_weight = expr.eval_to_const()?.get_float().ok_or_else(|| {
                    InvalidQuerySnafu {
                        message: "false_negative_weight must be a float".to_string(),
                    }
                    .build()
                })?;
            }
            "n_gram" => {
                let mut expr = build_expr(opt_val, param_pool)?;
                expr.partial_eval()?;
                let v_int = expr.eval_to_const()?.get_int().ok_or_else(|| {
                    InvalidQuerySnafu {
                        message: "n_gram must be an integer".to_string(),
                    }
                    .build()
                })?;
                n_gram = usize::try_from(v_int).map_err(|_e| {
                    InvalidQuerySnafu {
                        message: "n_gram must be a non-negative integer".to_string(),
                    }
                    .build()
                })?;
            }
            "n_perm" => {
                let mut expr = build_expr(opt_val, param_pool)?;
                expr.partial_eval()?;
                let v_int = expr.eval_to_const()?.get_int().ok_or_else(|| {
                    InvalidQuerySnafu {
                        message: "n_perm must be an integer".to_string(),
                    }
                    .build()
                })?;
                n_perm = usize::try_from(v_int).map_err(|_e| {
                    InvalidQuerySnafu {
                        message: "n_perm must be a non-negative integer".to_string(),
                    }
                    .build()
                })?;
            }
            "target_threshold" => {
                let mut expr = build_expr(opt_val, param_pool)?;
                expr.partial_eval()?;
                target_threshold = expr.eval_to_const()?.get_float().ok_or_else(|| {
                    InvalidQuerySnafu {
                        message: "target_threshold must be a float".to_string(),
                    }
                    .build()
                })?;
            }
            s => {
                return Err(InvalidQuerySnafu {
                    message: format!("Unknown option {s} for LSH index"),
                }
                .build()
                .into());
            }
        }
    }

    // Validate LSH-specific constraints
    if false_positive_weight <= 0. {
        return Err(InvalidQuerySnafu {
            message: "false_positive_weight must be positive".to_string(),
        }
        .build()
        .into());
    }
    if false_negative_weight <= 0. {
        return Err(InvalidQuerySnafu {
            message: "false_negative_weight must be positive".to_string(),
        }
        .build()
        .into());
    }
    if n_gram == 0 {
        return Err(InvalidQuerySnafu {
            message: "n_gram must be positive".to_string(),
        }
        .build()
        .into());
    }
    if n_perm == 0 {
        return Err(InvalidQuerySnafu {
            message: "n_perm must be positive".to_string(),
        }
        .build()
        .into());
    }
    if target_threshold <= 0. || target_threshold >= 1. {
        return Err(InvalidQuerySnafu {
            message: "target_threshold must be between 0 and 1".to_string(),
        }
        .build()
        .into());
    }

    let total_weights = false_positive_weight + false_negative_weight;
    false_positive_weight /= total_weights;
    false_negative_weight /= total_weights;

    Ok(SysOp::CreateMinHashLshIndex(MinHashLshConfig {
        base_relation: CompactString::from(rel.as_str()),
        index_name: CompactString::from(name.as_str()),
        extractor: text_config.extractor,
        tokenizer: text_config.tokenizer,
        filters: text_config.filters,
        n_gram,
        n_perm,
        false_positive_weight: false_positive_weight.into(),
        false_negative_weight: false_negative_weight.into(),
        target_threshold: target_threshold.into(),
    }))
}

/// Parse an HNSW vector index creation from an `index_create_adv` pair.
pub(super) fn parse_hnsw_index_create(
    inner: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<SysOp> {
    let mut inner = inner.into_inner();
    let rel = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected relation name".to_string(),
        }
        .build()
    })?;
    let name = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected index name".to_string(),
        }
        .build()
    })?;

    let mut vec_dim = 0;
    let mut dtype = VecElementType::F32;
    let mut vec_fields = vec![];
    let mut distance = HnswDistance::L2;
    let mut ef_construction = 0;
    let mut m_neighbours = 0;
    let mut index_filter = None;
    let mut extend_candidates = false;
    let mut keep_pruned_connections = false;

    for opt_pair in inner {
        let mut opt_inner = opt_pair.into_inner();
        let opt_name = opt_inner.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected option name".to_string(),
            }
            .build()
        })?;
        let opt_val = opt_inner.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected option value".to_string(),
            }
            .build()
        })?;
        let opt_val_str = opt_val.as_str();
        match opt_name.as_str() {
            "dim" => {
                let v = build_expr(opt_val, param_pool)?
                    .eval_to_const()?
                    .get_int()
                    .ok_or_else(|| {
                        InvalidQuerySnafu {
                            message: format!("Invalid vec_dim: {opt_val_str}"),
                        }
                        .build()
                    })?;
                if v <= 0 {
                    return Err(InvalidQuerySnafu {
                        message: format!("Invalid vec_dim: {v}"),
                    }
                    .build()
                    .into());
                }
                // INVARIANT: range-checked > 0 above.
                vec_dim = usize::try_from(v).unwrap_or(usize::MAX);
            }
            "ef_construction" | "ef" => {
                let v = build_expr(opt_val, param_pool)?
                    .eval_to_const()?
                    .get_int()
                    .ok_or_else(|| {
                        InvalidQuerySnafu {
                            message: format!("Invalid ef_construction: {opt_val_str}"),
                        }
                        .build()
                    })?;
                if v <= 0 {
                    return Err(InvalidQuerySnafu {
                        message: format!("Invalid ef_construction: {v}"),
                    }
                    .build()
                    .into());
                }
                // INVARIANT: range-checked > 0 above.
                ef_construction = usize::try_from(v).unwrap_or(usize::MAX);
            }
            "m_neighbours" | "m" => {
                let v = build_expr(opt_val, param_pool)?
                    .eval_to_const()?
                    .get_int()
                    .ok_or_else(|| {
                        InvalidQuerySnafu {
                            message: format!("Invalid m_neighbours: {opt_val_str}"),
                        }
                        .build()
                    })?;
                if v <= 0 {
                    return Err(InvalidQuerySnafu {
                        message: format!("Invalid m_neighbours: {v}"),
                    }
                    .build()
                    .into());
                }
                // INVARIANT: range-checked > 0 above.
                m_neighbours = usize::try_from(v).unwrap_or(usize::MAX);
            }
            "dtype" => {
                dtype = match opt_val.as_str() {
                    "F32" | "Float" => VecElementType::F32,
                    "F64" | "Double" => VecElementType::F64,
                    s => {
                        return Err(InvalidQuerySnafu {
                            message: format!("Invalid dtype: {s}"),
                        }
                        .build()
                        .into());
                    }
                }
            }
            "fields" => {
                let fields = build_expr(opt_val, &Default::default())?;
                vec_fields = fields.to_var_list()?;
            }
            "distance" | "dist" => {
                distance = match opt_val.as_str().trim() {
                    "L2" => HnswDistance::L2,
                    "IP" => HnswDistance::InnerProduct,
                    "Cosine" => HnswDistance::Cosine,
                    s => {
                        return Err(InvalidQuerySnafu {
                            message: format!("Invalid distance: {s}"),
                        }
                        .build()
                        .into());
                    }
                }
            }
            "filter" => {
                index_filter = Some(opt_val.as_str().to_string());
            }
            "extend_candidates" => {
                extend_candidates = opt_val.as_str().trim() == "true";
            }
            "keep_pruned_connections" => {
                keep_pruned_connections = opt_val.as_str().trim() == "true";
            }
            s => {
                return Err(InvalidQuerySnafu {
                    message: format!("Invalid option: {s}"),
                }
                .build()
                .into());
            }
        }
    }

    if ef_construction == 0 {
        return Err(InvalidQuerySnafu {
            message: "ef_construction must be set".to_string(),
        }
        .build()
        .into());
    }
    if m_neighbours == 0 {
        return Err(InvalidQuerySnafu {
            message: "m_neighbours must be set".to_string(),
        }
        .build()
        .into());
    }

    Ok(SysOp::CreateVectorIndex(HnswIndexConfig {
        base_relation: CompactString::from(rel.as_str()),
        index_name: CompactString::from(name.as_str()),
        vec_dim,
        dtype,
        vec_fields,
        distance,
        ef_construction,
        m_neighbours,
        index_filter,
        extend_candidates,
        keep_pruned_connections,
    }))
}
