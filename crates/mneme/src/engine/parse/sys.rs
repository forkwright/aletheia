//! System command parsing.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::engine::error::InternalResult as Result;
use crate::engine::parse::error::InvalidQuerySnafu;
use compact_str::CompactString;
use itertools::Itertools;
use ordered_float::OrderedFloat;

use crate::engine::data::program::InputProgram;
use crate::engine::data::relation::VecElementType;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::fts::TokenizerConfig;
use crate::engine::parse::expr::{build_expr, parse_string};
use crate::engine::parse::query::parse_query;
use crate::engine::parse::{ExtractSpan, Pairs, Rule};
use crate::engine::runtime::relation::AccessLevel;
use crate::engine::{Expr, FixedRule};

#[derive(Debug)]
pub enum SysOp {
    Compact,
    ListColumns(Symbol),
    ListIndices(Symbol),
    ListRelations,
    ListRunning,
    ListFixedRules,
    KillRunning(u64),
    Explain(Box<InputProgram>),
    RemoveRelation(Vec<Symbol>),
    RenameRelation(Vec<(Symbol, Symbol)>),
    ShowTrigger(Symbol),
    SetTriggers(Symbol, Vec<String>, Vec<String>, Vec<String>),
    SetAccessLevel(Vec<Symbol>, AccessLevel),
    CreateIndex(Symbol, Symbol, Vec<Symbol>),
    CreateVectorIndex(HnswIndexConfig),
    CreateFtsIndex(FtsIndexConfig),
    CreateMinHashLshIndex(MinHashLshConfig),
    RemoveIndex(Symbol, Symbol),
    DescribeRelation(Symbol, CompactString),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FtsIndexConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub extractor: String,
    pub tokenizer: TokenizerConfig,
    pub filters: Vec<TokenizerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MinHashLshConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub extractor: String,
    pub tokenizer: TokenizerConfig,
    pub filters: Vec<TokenizerConfig>,
    pub n_gram: usize,
    pub n_perm: usize,
    pub false_positive_weight: OrderedFloat<f64>,
    pub false_negative_weight: OrderedFloat<f64>,
    pub target_threshold: OrderedFloat<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HnswIndexConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub vec_dim: usize,
    pub dtype: VecElementType,
    pub vec_fields: Vec<CompactString>,
    pub distance: HnswDistance,
    pub ef_construction: usize,
    pub m_neighbours: usize,
    pub index_filter: Option<String>,
    pub extend_candidates: bool,
    pub keep_pruned_connections: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HnswDistance {
    L2,
    InnerProduct,
    Cosine,
}

pub(crate) fn parse_sys(
    mut src: Pairs<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    algorithms: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<SysOp> {
    let inner = src.next().expect("pest guarantees sys op inner");
    Ok(match inner.as_rule() {
        Rule::compact_op => SysOp::Compact,
        Rule::running_op => SysOp::ListRunning,
        Rule::kill_op => {
            let i_expr = inner
                .into_inner()
                .next()
                .expect("pest guarantees kill op expr");
            let i_val = build_expr(i_expr, param_pool)?;
            let i_val = i_val.eval_to_const()?;
            let i_val = i_val.get_int().ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "Process ID must be an integer".to_string(),
                }
                .build()
            })?;
            SysOp::KillRunning(i_val as u64)
        }
        Rule::explain_op => {
            let prog = parse_query(
                inner
                    .into_inner()
                    .next()
                    .expect("pest guarantees explain op script")
                    .into_inner(),
                param_pool,
                algorithms,
                cur_vld,
            )?;
            SysOp::Explain(Box::new(prog))
        }
        Rule::describe_relation_op => {
            let mut inner = inner.into_inner();
            let rels_p = inner
                .next()
                .expect("pest guarantees describe relation name");
            let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
            let description = match inner.next() {
                None => Default::default(),
                Some(desc_p) => parse_string(desc_p)?,
            };
            SysOp::DescribeRelation(rel, description)
        }
        Rule::list_relations_op => SysOp::ListRelations,
        Rule::remove_relations_op => {
            let rel = inner
                .into_inner()
                .map(|rels_p| Symbol::new(rels_p.as_str(), rels_p.extract_span()))
                .collect_vec();

            SysOp::RemoveRelation(rel)
        }
        Rule::list_columns_op => {
            let rels_p = inner
                .into_inner()
                .next()
                .expect("pest guarantees column relation name");
            let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
            SysOp::ListColumns(rel)
        }
        Rule::list_indices_op => {
            let rels_p = inner
                .into_inner()
                .next()
                .expect("pest guarantees index relation name");
            let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
            SysOp::ListIndices(rel)
        }
        Rule::rename_relations_op => {
            let rename_pairs = inner
                .into_inner()
                .map(|pair| {
                    let mut src = pair.into_inner();
                    let rels_p = src.next().expect("pest guarantees rename source name");
                    let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
                    let rels_p = src.next().expect("pest guarantees rename target name");
                    let new_rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
                    (rel, new_rel)
                })
                .collect_vec();
            SysOp::RenameRelation(rename_pairs)
        }
        Rule::access_level_op => {
            let mut ps = inner.into_inner();
            let access_level = match ps.next().expect("pest guarantees access level").as_str() {
                "normal" => AccessLevel::Normal,
                "protected" => AccessLevel::Protected,
                "read_only" => AccessLevel::ReadOnly,
                "hidden" => AccessLevel::Hidden,
                _ => unreachable!(),
            };
            let mut rels = vec![];
            for rel_p in ps {
                let rel = Symbol::new(rel_p.as_str(), rel_p.extract_span());
                rels.push(rel)
            }
            SysOp::SetAccessLevel(rels, access_level)
        }
        Rule::trigger_relation_show_op => {
            let rels_p = inner
                .into_inner()
                .next()
                .expect("pest guarantees trigger relation name");
            let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
            SysOp::ShowTrigger(rel)
        }
        Rule::trigger_relation_op => {
            let mut src = inner.into_inner();
            let rels_p = src.next().expect("pest guarantees trigger relation name");
            let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
            let mut puts = vec![];
            let mut rms = vec![];
            let mut replaces = vec![];
            for clause in src {
                let mut clause_inner = clause.into_inner();
                let op = clause_inner.next().expect("pest guarantees trigger op");
                let script = clause_inner.next().expect("pest guarantees trigger script");
                let script_str = script.as_str();
                parse_query(
                    script.into_inner(),
                    &Default::default(),
                    algorithms,
                    cur_vld,
                )?;
                match op.as_rule() {
                    Rule::trigger_put => puts.push(script_str.to_string()),
                    Rule::trigger_rm => rms.push(script_str.to_string()),
                    Rule::trigger_replace => replaces.push(script_str.to_string()),
                    r => unreachable!("{:?}", r),
                }
            }
            SysOp::SetTriggers(rel, puts, rms, replaces)
        }
        Rule::lsh_idx_op => {
            let inner = inner
                .into_inner()
                .next()
                .expect("pest guarantees lsh idx inner");
            match inner.as_rule() {
                Rule::index_create_adv => {
                    let mut inner = inner.into_inner();
                    let rel = inner.next().expect("pest guarantees lsh relation name");
                    let name = inner.next().expect("pest guarantees lsh index name");
                    let mut filters = vec![];
                    let mut tokenizer = TokenizerConfig {
                        name: Default::default(),
                        args: Default::default(),
                    };
                    let mut extractor = "".to_string();
                    let mut extract_filter = "".to_string();
                    let mut n_gram = 1;
                    let mut n_perm = 200;
                    let mut target_threshold = 0.9;
                    let mut false_positive_weight = 1.0;
                    let mut false_negative_weight = 1.0;
                    for opt_pair in inner {
                        let mut opt_inner = opt_pair.into_inner();
                        let opt_name = opt_inner.next().expect("pest guarantees option name");
                        let opt_val = opt_inner.next().expect("pest guarantees option value");
                        match opt_name.as_str() {
                            "false_positive_weight" => {
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                let v = expr.eval_to_const()?;
                                false_positive_weight = v.get_float().ok_or_else(|| {
                                    InvalidQuerySnafu {
                                        message: "false_positive_weight must be a float"
                                            .to_string(),
                                    }
                                    .build()
                                })?;
                            }
                            "false_negative_weight" => {
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                let v = expr.eval_to_const()?;
                                false_negative_weight = v.get_float().ok_or_else(|| {
                                    InvalidQuerySnafu {
                                        message: "false_negative_weight must be a float"
                                            .to_string(),
                                    }
                                    .build()
                                })?;
                            }
                            "n_gram" => {
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                let v = expr.eval_to_const()?;
                                n_gram = v.get_int().ok_or_else(|| {
                                    InvalidQuerySnafu {
                                        message: "n_gram must be an integer".to_string(),
                                    }
                                    .build()
                                })? as usize;
                            }
                            "n_perm" => {
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                let v = expr.eval_to_const()?;
                                n_perm = v.get_int().ok_or_else(|| {
                                    InvalidQuerySnafu {
                                        message: "n_perm must be an integer".to_string(),
                                    }
                                    .build()
                                })? as usize;
                            }
                            "target_threshold" => {
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                let v = expr.eval_to_const()?;
                                target_threshold = v.get_float().ok_or_else(|| {
                                    InvalidQuerySnafu {
                                        message: "target_threshold must be a float".to_string(),
                                    }
                                    .build()
                                })?;
                            }
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
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                match expr {
                                    Expr::UnboundApply { op, args, .. } => {
                                        let mut targs = vec![];
                                        for arg in args.iter() {
                                            let v = arg.clone().eval_to_const()?;
                                            targs.push(v);
                                        }
                                        tokenizer.name = op;
                                        tokenizer.args = targs;
                                    }
                                    Expr::Binding { var, .. } => {
                                        tokenizer.name = var.name;
                                        tokenizer.args = vec![];
                                    }
                                    _ => return Err(InvalidQuerySnafu {
                                        message: "Tokenizer must be a symbol or a call for an existing tokenizer".to_string()
                                    }.build().into()),
                                }
                            }
                            "filters" => {
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                match expr {
                                    Expr::Apply { op, args, .. } => {
                                        if op.name != "OP_LIST" {
                                            return Err(InvalidQuerySnafu {
                                                message: "Filters must be a list of filters"
                                                    .to_string(),
                                            }
                                            .build()
                                            .into());
                                        }
                                        for arg in args.iter() {
                                            match arg {
                                                Expr::UnboundApply { op, args, .. } => {
                                                    let mut targs = vec![];
                                                    for arg in args.iter() {
                                                        let v = arg.clone().eval_to_const()?;
                                                        targs.push(v);
                                                    }
                                                    filters.push(TokenizerConfig {
                                                        name: op.clone(),
                                                        args: targs,
                                                    })
                                                }
                                                Expr::Binding { var, .. } => {
                                                    filters.push(TokenizerConfig {
                                                        name: var.name.clone(),
                                                        args: vec![],
                                                    })
                                                }
                                                _ => return Err(InvalidQuerySnafu {
                                                    message: "Tokenizer must be a symbol or a call for an existing tokenizer".to_string()
                                                }
                                                .build().into()),
                                            }
                                        }
                                    }
                                    _ => {
                                        return Err(InvalidQuerySnafu {
                                            message: "Filters must be a list of filters"
                                                .to_string(),
                                        }
                                        .build()
                                        .into());
                                    }
                                }
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

                    if !extract_filter.is_empty() {
                        extractor = format!("if({extract_filter}, {extractor})");
                    }

                    let config = MinHashLshConfig {
                        base_relation: CompactString::from(rel.as_str()),
                        index_name: CompactString::from(name.as_str()),
                        extractor,
                        tokenizer,
                        filters,
                        n_gram,
                        n_perm,
                        false_positive_weight: false_positive_weight.into(),
                        false_negative_weight: false_negative_weight.into(),
                        target_threshold: target_threshold.into(),
                    };
                    SysOp::CreateMinHashLshIndex(config)
                }
                Rule::index_drop => {
                    let mut inner = inner.into_inner();
                    let rel = inner
                        .next()
                        .expect("pest guarantees lsh drop relation name");
                    let name = inner.next().expect("pest guarantees lsh drop index name");
                    SysOp::RemoveIndex(
                        Symbol::new(rel.as_str(), rel.extract_span()),
                        Symbol::new(name.as_str(), name.extract_span()),
                    )
                }
                r => unreachable!("{:?}", r),
            }
        }
        Rule::fts_idx_op => {
            let inner = inner
                .into_inner()
                .next()
                .expect("pest guarantees fts idx inner");
            match inner.as_rule() {
                Rule::index_create_adv => {
                    let mut inner = inner.into_inner();
                    let rel = inner.next().expect("pest guarantees fts relation name");
                    let name = inner.next().expect("pest guarantees fts index name");
                    let mut filters = vec![];
                    let mut tokenizer = TokenizerConfig {
                        name: Default::default(),
                        args: Default::default(),
                    };
                    let mut extractor = "".to_string();
                    let mut extract_filter = "".to_string();
                    for opt_pair in inner {
                        let mut opt_inner = opt_pair.into_inner();
                        let opt_name = opt_inner.next().expect("pest guarantees option name");
                        let opt_val = opt_inner.next().expect("pest guarantees option value");
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
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                match expr {
                                    Expr::UnboundApply { op, args, .. } => {
                                        let mut targs = vec![];
                                        for arg in args.iter() {
                                            let v = arg.clone().eval_to_const()?;
                                            targs.push(v);
                                        }
                                        tokenizer.name = op;
                                        tokenizer.args = targs;
                                    }
                                    Expr::Binding { var, .. } => {
                                        tokenizer.name = var.name;
                                        tokenizer.args = vec![];
                                    }
                                    _ => return Err(InvalidQuerySnafu {
                                        message: "Tokenizer must be a symbol or a call for an existing tokenizer".to_string()
                                    }
                                    .build().into()),
                                }
                            }
                            "filters" => {
                                let mut expr = build_expr(opt_val, param_pool)?;
                                expr.partial_eval()?;
                                match expr {
                                    Expr::Apply { op, args, .. } => {
                                        if op.name != "OP_LIST" {
                                            return Err(InvalidQuerySnafu {
                                                message: "Filters must be a list of filters"
                                                    .to_string(),
                                            }
                                            .build()
                                            .into());
                                        }
                                        for arg in args.iter() {
                                            match arg {
                                                Expr::UnboundApply { op, args, .. } => {
                                                    let mut targs = vec![];
                                                    for arg in args.iter() {
                                                        let v = arg.clone().eval_to_const()?;
                                                        targs.push(v);
                                                    }
                                                    filters.push(TokenizerConfig {
                                                        name: op.clone(),
                                                        args: targs,
                                                    })
                                                }
                                                Expr::Binding { var, .. } => {
                                                    filters.push(TokenizerConfig {
                                                        name: var.name.clone(),
                                                        args: vec![],
                                                    })
                                                }
                                                _ => return Err(InvalidQuerySnafu {
                                                    message: "Tokenizer must be a symbol or a call for an existing tokenizer".to_string()
                                                }
                                                .build().into()),
                                            }
                                        }
                                    }
                                    _ => {
                                        return Err(InvalidQuerySnafu {
                                            message: "Filters must be a list of filters"
                                                .to_string(),
                                        }
                                        .build()
                                        .into());
                                    }
                                }
                            }
                            s => {
                                return Err(InvalidQuerySnafu {
                                    message: format!("Unknown option {s} for FTS index"),
                                }
                                .build()
                                .into());
                            }
                        }
                    }
                    if !extract_filter.is_empty() {
                        extractor = format!("if({extract_filter}, {extractor})");
                    }
                    let config = FtsIndexConfig {
                        base_relation: CompactString::from(rel.as_str()),
                        index_name: CompactString::from(name.as_str()),
                        extractor,
                        tokenizer,
                        filters,
                    };
                    SysOp::CreateFtsIndex(config)
                }
                Rule::index_drop => {
                    let mut inner = inner.into_inner();
                    let rel = inner
                        .next()
                        .expect("pest guarantees fts drop relation name");
                    let name = inner.next().expect("pest guarantees fts drop index name");
                    SysOp::RemoveIndex(
                        Symbol::new(rel.as_str(), rel.extract_span()),
                        Symbol::new(name.as_str(), name.extract_span()),
                    )
                }
                r => unreachable!("{:?}", r),
            }
        }
        Rule::vec_idx_op => {
            let inner = inner
                .into_inner()
                .next()
                .expect("pest guarantees vec idx inner");
            match inner.as_rule() {
                Rule::index_create_adv => {
                    let mut inner = inner.into_inner();
                    let rel = inner.next().expect("pest guarantees vec relation name");
                    let name = inner.next().expect("pest guarantees vec index name");
                    // options
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
                        let opt_name = opt_inner.next().expect("pest guarantees option name");
                        let opt_val = opt_inner.next().expect("pest guarantees option value");
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
                                vec_dim = v as usize;
                            }
                            "ef_construction" | "ef" => {
                                let v = build_expr(opt_val, param_pool)?
                                    .eval_to_const()?
                                    .get_int()
                                    .ok_or_else(|| {
                                        InvalidQuerySnafu {
                                            message: format!(
                                                "Invalid ef_construction: {opt_val_str}"
                                            ),
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
                                ef_construction = v as usize;
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
                                m_neighbours = v as usize;
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
                    SysOp::CreateVectorIndex(HnswIndexConfig {
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
                    })
                }
                Rule::index_drop => {
                    let mut inner = inner.into_inner();
                    let rel = inner
                        .next()
                        .expect("pest guarantees vec drop relation name");
                    let name = inner.next().expect("pest guarantees vec drop index name");
                    SysOp::RemoveIndex(
                        Symbol::new(rel.as_str(), rel.extract_span()),
                        Symbol::new(name.as_str(), name.extract_span()),
                    )
                }
                r => unreachable!("{:?}", r),
            }
        }
        Rule::index_op => {
            let inner = inner
                .into_inner()
                .next()
                .expect("pest guarantees index op inner");
            match inner.as_rule() {
                Rule::index_create => {
                    let _span = inner.extract_span();
                    let mut inner = inner.into_inner();
                    let rel = inner.next().expect("pest guarantees index relation name");
                    let name = inner.next().expect("pest guarantees index name");
                    let cols = inner
                        .map(|p| Symbol::new(p.as_str(), p.extract_span()))
                        .collect_vec();

                    if cols.is_empty() {
                        return Err(InvalidQuerySnafu {
                            message: "index must have at least one column specified".to_string(),
                        }
                        .build()
                        .into());
                    }
                    SysOp::CreateIndex(
                        Symbol::new(rel.as_str(), rel.extract_span()),
                        Symbol::new(name.as_str(), name.extract_span()),
                        cols,
                    )
                }
                Rule::index_drop => {
                    let mut inner = inner.into_inner();
                    let rel = inner
                        .next()
                        .expect("pest guarantees index drop relation name");
                    let name = inner.next().expect("pest guarantees index drop name");
                    SysOp::RemoveIndex(
                        Symbol::new(rel.as_str(), rel.extract_span()),
                        Symbol::new(name.as_str(), name.extract_span()),
                    )
                }
                _ => unreachable!(),
            }
        }
        Rule::list_fixed_rules => SysOp::ListFixedRules,
        r => unreachable!("{:?}", r),
    })
}
