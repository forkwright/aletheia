//! System command parsing.

use compact_str::CompactString;
use ordered_float::OrderedFloat;

use crate::data::program::InputProgram;
use crate::data::relation::VecElementType;
use crate::data::symb::Symbol;
use crate::fts::TokenizerConfig;
use crate::runtime::relation::AccessLevel;

#[derive(Debug)]
#[non_exhaustive]
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
#[non_exhaustive]
pub enum HnswDistance {
    L2,
    InnerProduct,
    Cosine,
}

mod parse;

pub(crate) use parse::parse_sys;
